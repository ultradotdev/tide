//! AI: the built-in chat and its tool calls and change sets, and the Agents sidebar over
//! native AI CLI sessions.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tide_ai::chat::{
    ChatMessage, ChatSession, ChatStore, ContentBlock, InterruptedTurn, TurnAccumulator,
};
use tide_ai::cli::{
    self as ai_cli, CliProvider, CliSession, CliStatus, DiscoveryReport, DiscoveryState,
    ProviderDiscovery,
};
use tide_ai::models::{ModelInfo, ModelPreferences, ModelRegistry};
use tide_ai::provider::{
    ChatRequest, Provider, ProviderConfig, ProviderKind, Role, StopReason, StreamEvent,
};
use tide_ai::secrets::{SecretOptions, SecretService};
use tide_config::Paths;
use tide_tools::{ChangeSet, MapBuffers, ToolCall, Workspace as ToolWorkspace, execute_tool};
use tokio::sync::mpsc::Sender;

use super::Ui;
use crate::app::{AppEvent, Focus};
use crate::jobs::Jobs;
use crate::modal::{self, ConfirmAction, ModalItem, ModalKind, PromptAction, Submitted};
use crate::terminal_tab::{SpawnSpec, TerminalTab};
use crate::workbench::Tab;

#[derive(Debug)]
pub enum AiEvent {
    Stream {
        generation: u64,
        event: StreamEvent,
    },
    Finished {
        generation: u64,
    },
    Failed {
        generation: u64,
        message: String,
    },
    ToolResults {
        generation: u64,
        results: Vec<ChatToolResult>,
        change_set: Option<ChangeSet>,
    },
    Sessions(DiscoveryReport),
    Provider(ProviderDiscovery),
    CliPrepared {
        provider: CliProvider,
        executable: PathBuf,
        result: Result<(Vec<String>, Option<String>), String>,
    },
    CliRenamed {
        provider: String,
        session: String,
        title: String,
        error: Option<String>,
    },
    SessionAction {
        provider: String,
        session: String,
        archive: bool,
        error: Option<String>,
    },
}

const DEFAULT_AI_SYSTEM_PROMPT: &str = "You are Tide, a terminal-based coding assistant. Be concise, reference workspace paths, and never invent files or command output.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentRow {
    Provider(CliProvider),
    Session(usize),
    Status(CliProvider),
}

pub(crate) struct AiController {
    pub secrets: Arc<SecretService>,
    models: ModelRegistry,
    preferences: ModelPreferences,
    pub chat: ChatSession,
    pub chat_store: ChatStore,
    pub chat_draft: String,
    pub chat_scroll: usize,
    pub chat_turn: Option<TurnAccumulator>,
    pub chat_streaming: bool,
    pub chat_tasks: Vec<tokio::task::AbortHandle>,
    pub chat_generation: u64,
    pub chat_enabled: bool,
    tool_rounds: usize,
    pub pending_change_set: Option<ChangeSet>,
    pub pending_tool_results: Vec<ChatToolResult>,
    pub cli_status: Vec<CliStatus>,
    pub agent_sessions: Vec<CliSession>,
    pub agent_candidates: Vec<CliSession>,
    pub agent_reports: HashMap<CliProvider, ProviderDiscovery>,
    pub agent_query: String,
    pub agent_search: bool,
    next_cli_id: usize,
    last_catalog_refresh: Instant,
    native_tasks: Vec<tokio::task::JoinHandle<()>>,
    native_cancel: tide_ai::native_cli::Cancellation,
    pub agent_row: usize,
    pub agent_scroll: usize,
    pub agent_collapsed: HashSet<String>,
    pub agents_visible: bool,
    pub agents_width: u16,
    pub agents_loading: bool,
    pub agents_error: Option<String>,
    preferences_file: PathBuf,
    discovery: AgentDiscovery,
    jobs: Jobs,
    events: Sender<AppEvent>,
}

impl AiController {
    pub fn model_info(&self) -> Option<&ModelInfo> {
        self.models.model(&self.chat.model)
    }

    pub fn load(
        paths: &Paths,
        secrets: Arc<SecretService>,
        chat_enabled: bool,
        discovery: AgentDiscovery,
        jobs: &Jobs,
        events: Sender<AppEvent>,
    ) -> Self {
        let models = ModelRegistry::load_or_bundled(&paths.models_file).unwrap_or_default();
        let preferences = ModelPreferences::load(&paths.model_preferences_file);
        let (provider, model) = preferences
            .chat
            .as_deref()
            .and_then(|id| models.model(id))
            .map_or_else(
                || {
                    let model = models
                        .default_model("balanced")
                        .or_else(|| models.models().first().map(|model| model.id.as_str()))
                        .unwrap_or("claude-sonnet-4-5-20250929")
                        .to_owned();
                    let provider = models
                        .model(&model)
                        .map(|model| model.provider.clone())
                        .unwrap_or_else(|| "anthropic".to_owned());
                    (provider, model)
                },
                |model| (model.provider.clone(), model.id.clone()),
            );
        let mut chat = ChatSession::new(provider, model);
        let store = ChatStore::new(paths.conversations_dir.clone());
        if let Some(summary) = store.list().into_iter().next()
            && let Ok(loaded) = store.load(&summary.id)
        {
            chat = loaded;
        }
        let cli_status = ai_cli::detect_all();
        Self {
            secrets,
            models,
            preferences,
            chat,
            chat_store: ChatStore::new(paths.conversations_dir.clone()),
            chat_draft: String::new(),
            chat_scroll: 0,
            chat_turn: None,
            chat_streaming: false,
            chat_tasks: Vec::new(),
            chat_generation: 0,
            chat_enabled,
            tool_rounds: 0,
            pending_change_set: None,
            pending_tool_results: Vec::new(),
            cli_status,
            agent_sessions: Vec::new(),
            agent_candidates: Vec::new(),
            agent_reports: HashMap::new(),
            agent_query: String::new(),
            agent_search: false,
            next_cli_id: 1,
            last_catalog_refresh: Instant::now(),
            native_tasks: Vec::new(),
            native_cancel: tide_ai::native_cli::Cancellation::default(),
            agent_row: 0,
            agent_scroll: 0,
            agent_collapsed: HashSet::new(),
            agents_visible: false,
            agents_width: 34,
            agents_loading: false,
            agents_error: None,
            preferences_file: paths.model_preferences_file.clone(),
            discovery,
            jobs: jobs.clone(),
            events,
        }
    }

    /// Drops finished CLI work and refreshes a visible Agents sidebar every 15 seconds;
    /// true when it started a refresh.
    pub fn tick(&mut self, ui: &mut Ui<'_>) -> bool {
        self.native_tasks.retain(|task| !task.is_finished());
        if !self.agents_loading
            && self.agents_visible
            && self.last_catalog_refresh.elapsed() > Duration::from_secs(15)
        {
            self.refresh_sessions(ui);
            return true;
        }
        false
    }

    /// Cancels native CLI work and waits briefly for it to stop.
    pub async fn stop_native_tasks(&mut self) {
        self.native_cancel.cancel();
        let native = std::mem::take(&mut self.native_tasks);
        let _ = tokio::time::timeout(Duration::from_secs(10), async {
            for task in native {
                let _ = task.await;
            }
        })
        .await;
    }

    pub fn abort_chat_tasks(&mut self) {
        for task in self.chat_tasks.drain(..) {
            task.abort();
        }
    }

    pub fn handle_event(&mut self, event: AiEvent, ui: &mut Ui<'_>) {
        match event {
            AiEvent::Stream { generation, event } => {
                if generation == self.chat_generation
                    && let Some(turn) = self.chat_turn.as_mut()
                {
                    turn.apply(event);
                    self.chat_scroll = 0;
                }
            }
            AiEvent::Finished { generation } => {
                if generation == self.chat_generation {
                    self.finish_chat_turn(ui);
                }
            }
            AiEvent::Failed {
                generation,
                message,
            } => {
                if generation == self.chat_generation {
                    if let Some(turn) = self.chat_turn.as_mut() {
                        turn.mark_error(message.clone());
                    }
                    self.finish_chat_turn(ui);
                    ui.status(format!("AI: {message}"));
                }
            }
            AiEvent::ToolResults {
                generation,
                results,
                change_set,
            } => {
                if generation == self.chat_generation {
                    self.chat_tasks.clear();
                    if let Some(set) = change_set {
                        let preview = set.preview();
                        self.pending_change_set = Some(set);
                        self.pending_tool_results = results;
                        *ui.overlay = Some(modal::confirm(
                            ConfirmAction::ApplyAiChanges,
                            "Approve AI Changes",
                            &preview.chars().take(900).collect::<String>(),
                        ));
                    } else {
                        self.apply_tool_results(results, ui);
                    }
                }
            }
            AiEvent::CliPrepared {
                provider,
                executable,
                result,
            } => match result {
                Ok((args, id)) => self.spawn_prepared_cli(provider, executable, args, id, ui),
                Err(error) => ui.status(format!("Agents: {error}")),
            },
            AiEvent::CliRenamed {
                provider,
                session,
                title,
                error,
            } => {
                if let Some(error) = error {
                    ui.status(format!("Agents: {error}"));
                } else {
                    for item in &mut self.agent_candidates {
                        if item.provider_id == provider && item.session_id == session {
                            item.title = title.clone();
                        }
                    }
                    self.rebuild_sessions(ui);
                    self.refresh_sessions(ui);
                    ui.status("Agents: session renamed");
                }
            }
            AiEvent::SessionAction {
                provider,
                session,
                archive,
                error,
            } => {
                if let Some(error) = error {
                    ui.status(format!("Agents: {error}"));
                } else {
                    ui.workbench.close_ai_cli(&provider, &session);
                    ui.status(format!(
                        "Agents: {} {session}",
                        if archive { "archived" } else { "deleted" }
                    ));
                    self.refresh_sessions(ui);
                }
            }
            AiEvent::Sessions(report) => {
                self.agents_loading = false;
                if !report.statuses.is_empty() {
                    self.cli_status = report.statuses;
                }
                if report.providers.is_empty() {
                    if !report.sessions.is_empty() || report.errors.is_empty() {
                        self.agent_candidates = report.sessions;
                        self.rebuild_sessions(ui);
                    }
                } else {
                    for provider in report.providers {
                        self.merge_provider(provider, ui);
                    }
                }
                self.agents_error = (!report.errors.is_empty()).then(|| report.errors.join("; "));
                self.update_picker(ui);
            }
            AiEvent::Provider(report) => self.merge_provider(report, ui),
        }
    }

    /// Handles the AI parts of a modal submission and hands back anything else.
    pub fn submit(&mut self, submitted: Submitted, ui: &mut Ui<'_>) -> Option<Submitted> {
        match submitted {
            Submitted::Model(model) => self.select_model(&model, ui),
            Submitted::ChatSession(session) => self.load_chat_session(&session, ui),
            Submitted::Prompt(PromptAction::RenameAgent(provider, id), input) => {
                self.rename_session(&provider, &id, &input, ui)
            }
            Submitted::AgentSession { provider, session } => {
                self.launch_cli(provider, Some(&session), ui);
            }
            Submitted::AiCli(provider) => self.launch_cli(provider, None, ui),
            Submitted::Prompt(PromptAction::AiApiKey(provider), input) => {
                let key = input;
                self.save_api_key(&provider, &key, ui);
            }
            Submitted::Prompt(PromptAction::UnlockSecrets, input) => {
                let password = input;
                self.unlock_secrets(&password, ui);
            }
            Submitted::Confirm(ConfirmAction::ApplyAiChanges) => {
                self.resolve_pending_changes(true, ui);
            }
            Submitted::Confirm(ConfirmAction::DeleteAiSession {
                provider,
                session,
                archive,
            }) => {
                self.run_session_action(&provider, &session, archive, ui);
            }
            other => return Some(other),
        }
        None
    }

    pub fn chat_available(&mut self, ui: &mut Ui<'_>) -> bool {
        if self.chat_enabled {
            return true;
        }
        ui.status(
            "Built-in AI Chat is not included in this build; use Agents for external CLI sessions",
        );
        false
    }

    pub fn open_chat_tab(&mut self, fresh: bool, ui: &mut Ui<'_>) {
        if !self.chat_available(ui) {
            return;
        }
        if fresh {
            self.start_new_chat(ui);
        }
        if !ui.workbench.focus_ai_chat() {
            ui.workbench.insert_tab_focused(Tab::AiChat);
        }
        *ui.focus = Focus::Main;
        ui.mark_session_dirty();
    }

    pub fn start_new_chat(&mut self, ui: &mut Ui<'_>) {
        if self.chat_streaming {
            self.cancel_chat(ui);
        }
        let provider = self.chat.provider.clone();
        let model = self.chat.model.clone();
        self.chat = ChatSession::new(provider, model);
        self.chat_draft.clear();
        self.chat_scroll = 0;
        let _ = self.chat_store.save(&self.chat);
        ui.mark_session_dirty();
    }

    pub fn send_chat_message(&mut self, ui: &mut Ui<'_>) {
        if !self.chat_available(ui) {
            return;
        }
        if self.chat_streaming {
            ui.status("AI: a turn is already streaming; Ctrl+Alt+C cancels it");
            return;
        }
        let prompt = self.chat_draft.trim().to_owned();
        if prompt.is_empty() {
            return;
        }
        if self.provider_config(ui).is_none() {
            return;
        }
        self.chat.push_user_text(&prompt);
        self.chat.interrupted = Some(InterruptedTurn {
            message_id: self
                .chat
                .messages
                .last()
                .map(|message| message.id.clone())
                .unwrap_or_default(),
            text: prompt.clone(),
            started_at: crate::app::timestamp_millis().to_string(),
        });
        self.chat_draft.clear();
        let _ = self.chat_store.save(&self.chat);
        self.tool_rounds = 0;
        self.start_chat_turn(ui);
    }

    pub fn provider_config(&mut self, ui: &mut Ui<'_>) -> Option<ProviderConfig> {
        let kind = ProviderKind::parse(&self.chat.provider).unwrap_or(ProviderKind::Anthropic);
        let mut config = ProviderConfig::new(kind, self.chat.model.clone()).with_max_tokens(4096);
        if kind.requires_api_key() {
            let Some(api_key) = self.secrets.provider_api_key(kind.as_str()) else {
                let canonical = match kind {
                    ProviderKind::Anthropic => "ai.claude.api-key",
                    ProviderKind::OpenAi => "ai.openai.api-key",
                    ProviderKind::Gemini => "ai.gemini.api-key",
                    ProviderKind::Ollama => "",
                };
                ui.status(format!(
                    "AI: no API key for {}. Use Secrets: Set ... API Key (stored as {canonical})",
                    kind.as_str()
                ));
                return None;
            };
            config = config.with_api_key(api_key);
        }
        if let Some(temperature) = ui
            .settings
            .get("ai.chat.temperature")
            .and_then(serde_json::Value::as_f64)
            // A temperature outside the f32 range is invalid; keep the provider default.
            .and_then(|temperature| num_traits::ToPrimitive::to_f32(&temperature))
        {
            config = config.with_temperature(temperature);
        }
        Some(config)
    }

    pub fn start_chat_turn(&mut self, ui: &mut Ui<'_>) {
        let Some(config) = self.provider_config(ui) else {
            return;
        };
        let provider = match Provider::new(config) {
            Ok(provider) => provider,
            Err(error) => {
                ui.status(format!("AI: {error:#}"));
                return;
            }
        };
        let system = ui
            .settings
            .get("ai.panel.initialPrompt")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(DEFAULT_AI_SYSTEM_PROMPT)
            .to_owned();
        let request = ChatRequest {
            system: Some(system),
            messages: self.chat.provider_turns(),
            max_tokens: Some(4096),
            temperature: None,
            tools: tide_tools::canonical_tools()
                .iter()
                .map(|tool| tide_ai::provider::ToolSchema {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    input_schema: tool.input_schema.clone(),
                })
                .collect(),
        };
        self.chat_generation = self.chat_generation.wrapping_add(1);
        let generation = self.chat_generation;
        self.chat_turn = Some(TurnAccumulator::new());
        self.chat_streaming = true;
        self.chat_scroll = 0;
        for task in self.chat_tasks.drain(..) {
            task.abort();
        }
        let sender = self.events.clone();
        let (stream_tx, mut stream_rx) = tokio::sync::mpsc::unbounded_channel();
        let provider_task = self.jobs.spawn(async move {
            provider
                .stream(&request, |event| {
                    let _ = stream_tx.send(event);
                })
                .await
        });
        self.chat_tasks.push(provider_task.abort_handle());
        let forward = self.jobs.spawn(async move {
            let mut provider_task = provider_task;
            loop {
                tokio::select! {
                    event = stream_rx.recv() => {
                        match event {
                            Some(event) => {
                                if sender
                                    .send(AppEvent::Ai(AiEvent::Stream { generation, event }))
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            None => break,
                        }
                    }
                    result = &mut provider_task => {
                        while let Ok(event) = stream_rx.try_recv() {
                            let _ = sender
                                .send(AppEvent::Ai(AiEvent::Stream { generation, event }))
                                .await;
                        }
                        match result {
                            Ok(Ok(_)) => {
                                let _ = sender
                                    .send(AppEvent::Ai(AiEvent::Finished { generation }))
                                    .await;
                            }
                            Ok(Err(error)) => {
                                let _ = sender
                                    .send(AppEvent::Ai(AiEvent::Failed {
                                        generation,
                                        message: format!("{error:#}"),
                                    }))
                                    .await;
                            }
                            Err(error) => {
                                let _ = sender
                                    .send(AppEvent::Ai(AiEvent::Failed {
                                        generation,
                                        message: error.to_string(),
                                    }))
                                    .await;
                            }
                        }
                        return;
                    }
                }
            }
        });
        self.chat_tasks.push(forward.abort_handle());
        ui.status(format!("AI: waiting for {}", self.chat.model));
    }

    pub fn finish_chat_turn(&mut self, ui: &mut Ui<'_>) {
        self.chat_streaming = false;
        self.chat_tasks.clear();
        if let Some(turn) = self.chat_turn.take() {
            let (message, stop, _usage) = turn.finish();
            let wants_tools = message
                .blocks
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolUse { .. }));
            self.chat.messages.push(message);
            self.chat.interrupted = None;
            self.chat.touch();
            let _ = self.chat_store.save(&self.chat);
            ui.mark_session_dirty();
            self.chat_scroll = 0;
            if wants_tools && stop == StopReason::ToolUse {
                self.handle_tool_calls(ui);
            } else {
                self.tool_rounds = 0;
            }
        }
    }

    pub fn cancel_chat(&mut self, ui: &mut Ui<'_>) {
        if !self.chat_streaming && self.chat_tasks.is_empty() {
            return;
        }
        for task in self.chat_tasks.drain(..) {
            task.abort();
        }
        self.chat_generation = self.chat_generation.wrapping_add(1);
        if let Some(mut turn) = self.chat_turn.take() {
            turn.mark_error("cancelled");
            let (message, _stop, _usage) = turn.finish();
            self.chat.messages.push(message);
            self.chat.touch();
            let _ = self.chat_store.save(&self.chat);
        }
        self.chat_streaming = false;
        self.pending_change_set = None;
        self.pending_tool_results.clear();
        ui.status("AI: turn cancelled");
        ui.mark_session_dirty();
    }

    pub fn buffer_snapshot(&self, ui: &Ui<'_>) -> MapBuffers {
        let mut buffers = MapBuffers::new();
        for tab in ui.workbench.tabs() {
            if let Some(editor) = tab.editor()
                && let Some(path) = editor.path()
            {
                let key = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
                buffers.insert(key, editor.document.text());
            }
        }
        buffers
    }

    pub fn handle_tool_calls(&mut self, ui: &mut Ui<'_>) {
        let Some(message) = self.chat.last_assistant() else {
            return;
        };
        let calls = message
            .blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolUse {
                    id,
                    name,
                    input_json,
                } => Some(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    input: if input_json.trim().is_empty() {
                        serde_json::json!({})
                    } else {
                        serde_json::from_str(input_json).unwrap_or_else(|_| serde_json::json!({}))
                    },
                }),
                _ => None,
            })
            .collect::<Vec<_>>();
        if calls.is_empty() {
            self.tool_rounds = 0;
            return;
        }
        let Ok(workspace) = ToolWorkspace::new(ui.workspace) else {
            ui.status("AI: workspace is unavailable for tools");
            return;
        };
        let buffers = self.buffer_snapshot(ui);
        let allow_commands = ui
            .settings
            .get("ai.allowValidationCommands")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let sender = self.events.clone();
        let generation = self.chat_generation;
        ui.status(format!("AI: running {} tool call(s)", calls.len()));
        let task = self.jobs.spawn(async move {
            let mut results = Vec::new();
            let mut change_set: Option<ChangeSet> = None;
            for call in calls {
                match execute_tool(&workspace, &call, &buffers, allow_commands).await {
                    Ok(outcome) => {
                        if let Some(set) = outcome.change_set {
                            if let Some(existing) = change_set.as_mut() {
                                existing.merge(set);
                            } else {
                                change_set = Some(set);
                            }
                        }
                        results.push(ChatToolResult {
                            tool_use_id: call.id,
                            content: outcome.content,
                            is_error: outcome.is_error,
                        });
                    }
                    Err(error) => results.push(ChatToolResult {
                        tool_use_id: call.id,
                        content: error.to_string(),
                        is_error: true,
                    }),
                }
            }
            let _ = sender
                .send(AppEvent::Ai(AiEvent::ToolResults {
                    generation,
                    results,
                    change_set,
                }))
                .await;
        });
        self.chat_tasks.push(task.abort_handle());
    }

    pub fn apply_tool_results(&mut self, results: Vec<ChatToolResult>, ui: &mut Ui<'_>) {
        if results.is_empty() {
            return;
        }
        let blocks = results
            .into_iter()
            .map(|result| ContentBlock::ToolResult {
                tool_use_id: result.tool_use_id,
                content: result.content,
                is_error: result.is_error,
            })
            .collect::<Vec<_>>();
        self.chat
            .messages
            .push(ChatMessage::new(Role::User, blocks));
        self.chat.touch();
        let _ = self.chat_store.save(&self.chat);
        self.tool_rounds += 1;
        if self.tool_rounds > 4 {
            self.tool_rounds = 0;
            ui.status("AI: stopped after 4 tool rounds");
            return;
        }
        self.start_chat_turn(ui);
    }

    pub fn resolve_pending_changes(&mut self, approve: bool, ui: &mut Ui<'_>) {
        let Some(mut set) = self.pending_change_set.take() else {
            return;
        };
        let mut results = std::mem::take(&mut self.pending_tool_results);
        if approve {
            match ToolWorkspace::new(ui.workspace).and_then(|workspace| set.apply(&workspace)) {
                Ok(report) => {
                    for result in &mut results {
                        if result.content.starts_with("Pending approval") {
                            (result.content, result.is_error) =
                                report.resolve_pending(&result.content);
                        }
                    }
                    let reads = match report.reads.len() {
                        0 => String::new(),
                        1 => "1 approved read".to_owned(),
                        count => format!("{count} approved reads"),
                    };
                    ui.status(match (report.applied.len(), reads.is_empty()) {
                        (0, false) => format!("AI: shared {reads}"),
                        (applied, false) => {
                            format!("AI: applied {applied} file change(s) and shared {reads}")
                        }
                        (applied, true) => format!("AI: applied {applied} file change(s)"),
                    });
                    ui.mark_session_dirty();
                }
                Err(error) => {
                    for result in &mut results {
                        if result.content.starts_with("Pending approval") {
                            result.content = format!("Change set failed to apply: {error}");
                            result.is_error = true;
                        }
                    }
                    ui.status(format!("AI: change set failed: {error}"));
                }
            }
        } else {
            for result in &mut results {
                if result.content.starts_with("Pending approval") {
                    result.content = "The user rejected the change set".to_owned();
                    result.is_error = true;
                }
            }
            ui.status("AI: changes rejected");
        }
        self.apply_tool_results(results, ui);
    }

    pub fn open_model_picker(&mut self, ui: &mut Ui<'_>) {
        if !self.chat_available(ui) {
            return;
        }
        let items = self
            .models
            .models()
            .iter()
            .filter(|model| model.available)
            .map(|model| {
                modal::ModalItem::new(
                    model.name.clone(),
                    format!("{} · {}", model.provider, model.id),
                    model.id.clone(),
                )
            })
            .collect();
        *ui.overlay = Some(modal::model_picker(items));
    }

    pub fn select_model(&mut self, model_id: &str, ui: &mut Ui<'_>) {
        let Some((provider, id, name)) = self
            .models
            .model(model_id)
            .map(|model| (model.provider.clone(), model.id.clone(), model.name.clone()))
        else {
            return;
        };
        self.chat.provider = provider.clone();
        self.chat.model = id.clone();
        self.chat.touch();
        self.preferences.chat = Some(id.clone());
        if let Err(error) = self.preferences.save(&self.preferences_file) {
            ui.status(format!("AI: could not save model preference: {error}"));
        }
        let _ = self.chat_store.save(&self.chat);
        ui.status(format!("AI model: {name} ({provider})"));
        ui.mark_session_dirty();
    }

    pub fn open_chat_sessions(&mut self, ui: &mut Ui<'_>) {
        if !self.chat_available(ui) {
            return;
        }
        let items = self
            .chat_store
            .list()
            .into_iter()
            .map(|session| {
                modal::ModalItem::new(
                    session.title.clone(),
                    format!(
                        "{} · {} · {} messages",
                        session.provider, session.model, session.message_count
                    ),
                    session.id,
                )
            })
            .collect::<Vec<_>>();
        if items.is_empty() {
            ui.status("AI: no saved conversations");
            return;
        }
        *ui.overlay = Some(modal::chat_sessions(items));
    }

    pub fn load_chat_session(&mut self, id: &str, ui: &mut Ui<'_>) {
        match self.chat_store.load(id) {
            Ok(session) => {
                if self.chat_streaming {
                    self.cancel_chat(ui);
                }
                self.chat = session;
                self.chat_draft.clear();
                self.chat_scroll = 0;
                self.open_chat_tab(false, ui);
                ui.status(format!("AI: opened {}", self.chat.title));
            }
            Err(error) => ui.status(format!("AI: could not load conversation: {error}")),
        }
    }

    pub fn save_api_key(&mut self, provider: &str, key: &str, ui: &mut Ui<'_>) {
        let key = key.trim();
        if key.is_empty() {
            return;
        }
        let name = match provider {
            "anthropic" => tide_ai::secrets::SECRET_KEY_CLAUDE,
            "openai" => tide_ai::secrets::SECRET_KEY_OPENAI,
            "gemini" => tide_ai::secrets::SECRET_KEY_GEMINI,
            _ => return,
        };
        match self.secrets.set(name, key, &SecretOptions::default()) {
            Ok(store) => ui.status(format!("Stored {provider} API key in {store}")),
            Err(error) => ui.status(format!("Secrets: {error}")),
        }
    }

    pub fn unlock_secrets(&mut self, password: &str, ui: &mut Ui<'_>) {
        match self.secrets.unlock(password) {
            Ok(()) => ui.status("Secrets: encrypted store unlocked"),
            Err(error) => ui.status(format!("Secrets: {error}")),
        }
    }

    pub fn manage_secrets(&mut self, ui: &mut Ui<'_>) {
        if self.secrets.is_unlocked() {
            ui.status(format!(
                "Secrets: unlocked with {} stored keys",
                self.secrets.list(None).len()
            ));
        } else {
            *ui.overlay = Some(modal::prompt(
                PromptAction::UnlockSecrets,
                "Unlock Secrets",
                "",
            ));
        }
    }

    pub fn open_cli_picker(&mut self, ui: &mut Ui<'_>) {
        let items = self
            .cli_status
            .iter()
            .filter(|status| status.installed)
            .map(|status| {
                modal::ModalItem::new(
                    status.provider.display_name(),
                    status
                        .executable
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_default(),
                    status.provider.id(),
                )
            })
            .collect::<Vec<_>>();
        if items.is_empty() {
            ui.status("AI CLI: no supported agents found in PATH");
            return;
        }
        let mut picker = modal::ai_cli_picker(items);
        if let Some(index) = ui
            .settings
            .get("ai.cli.defaultProvider")
            .and_then(|v| v.as_str())
            .and_then(|id| picker.base().items.iter().position(|item| item.value == id))
        {
            picker.base_mut().selected = index;
        }
        *ui.overlay = Some(picker);
    }

    pub fn open_session_picker(&mut self, ui: &mut Ui<'_>) {
        self.refresh_sessions(ui);
        *ui.overlay = Some(modal::agent_sessions(self.session_items()));
        self.update_picker(ui);
    }

    pub fn launch_cli(&mut self, provider: CliProvider, session_id: Option<&str>, ui: &mut Ui<'_>) {
        if let Some(session) = session_id
            && ui.workbench.focus_ai_cli(provider.id(), session)
        {
            *ui.focus = Focus::Main;
            ui.status(format!(
                "Agents: focused {} session",
                provider.display_name()
            ));
            return;
        }
        let status = self
            .cli_status
            .iter()
            .find(|status| status.provider == provider);
        let Some(executable) = status.and_then(|status| status.executable.clone()) else {
            ui.status(format!(
                "AI CLI: {} is not installed (expected `{}`)",
                provider.display_name(),
                provider.executable()
            ));
            return;
        };
        if session_id.is_none() && matches!(provider, CliProvider::Codex | CliProvider::OpenCode) {
            let sender = self.events.clone();
            let workspace = ui.workspace.to_path_buf();
            ui.status(format!(
                "Agents: creating native {} session...",
                provider.display_name()
            ));
            let cancellation = self.native_cancel.clone();
            let task = self.jobs.spawn_blocking(move || {
                let _scope = tide_ai::native_cli::CancellationScope::enter(cancellation);
                let result = tide_ai::native_cli::create_session(provider, &executable, &workspace);
                let _ = sender.blocking_send(AppEvent::Ai(AiEvent::CliPrepared {
                    provider,
                    executable,
                    result,
                }));
            });
            self.native_tasks.push(task);
            return;
        }
        let (args, captured_id) = session_id.map_or_else(
            || provider.prepare_new(ui.workspace),
            |id| {
                (
                    provider.launch_args(ui.workspace, Some(id)),
                    Some(id.to_owned()),
                )
            },
        );
        self.spawn_prepared_cli(provider, executable, args, captured_id, ui);
    }

    pub fn spawn_prepared_cli(
        &mut self,
        provider: CliProvider,
        executable: PathBuf,
        args: Vec<String>,
        session_id: Option<String>,
        ui: &mut Ui<'_>,
    ) {
        if let Some(id) = &session_id
            && ui.workbench.focus_ai_cli(provider.id(), id)
        {
            *ui.focus = Focus::Main;
            return;
        }
        let terminal_id = self.next_cli_id;
        self.next_cli_id += 1;
        let scrollback = ui
            .settings
            .get("tui.terminal.scrollback")
            .and_then(serde_json::Value::as_u64)
            .map_or(5_000, tide_core::num::saturating_usize);
        let mut spec = SpawnSpec::new(executable.display().to_string(), args);
        spec.scrollback = scrollback;
        match TerminalTab::spawn_configured(
            terminal_id,
            ui.workspace,
            spec,
            ui.setting_bool("ai.autoStartCli", true),
        ) {
            Ok(mut terminal) => {
                terminal.title = session_id.as_deref().map_or_else(
                    || provider.display_name().to_owned(),
                    |id| {
                        format!(
                            "{} {}",
                            provider.display_name(),
                            crate::display::short_id(id)
                        )
                    },
                );
                terminal.ai_provider = Some(provider.id().to_owned());
                terminal.ai_session_id = session_id;
                ui.workbench.insert_tab_focused(Tab::Terminal(terminal));
                *ui.focus = Focus::Main;
                ui.mark_session_dirty();
            }
            Err(error) => ui.status(format!("AI CLI: failed to launch: {error:#}")),
        }
    }

    pub fn selected_session(&self, ui: &Ui<'_>) -> Option<(CliProvider, String)> {
        if *ui.focus == Focus::Agents {
            let rows = self.rows();
            let AgentRow::Session(index) = rows.get(self.agent_row)? else {
                return None;
            };
            let session = self.agent_sessions.get(*index)?;
            return Some((
                CliProvider::parse(&session.provider_id)?,
                session.session_id.clone(),
            ));
        }
        match ui.workbench.active_tab() {
            Some(Tab::Terminal(terminal)) => Some((
                CliProvider::parse(terminal.ai_provider.as_deref()?)?,
                terminal.ai_session_id.clone()?,
            )),
            _ => None,
        }
    }

    pub fn delete_selected_session(&mut self, archive: bool, ui: &mut Ui<'_>) {
        let Some((provider, session)) = self.selected_session(ui) else {
            ui.status("Agents: select a session first");
            return;
        };
        if archive && !provider.supports_archive() {
            ui.status(format!(
                "Agents: {} does not support archiving",
                provider.display_name()
            ));
            return;
        }
        if !archive && !provider.supports_delete() {
            ui.status(format!(
                "Agents: {} does not support deleting sessions",
                provider.display_name()
            ));
            return;
        }
        *ui.overlay = Some(modal::confirm(
            ConfirmAction::DeleteAiSession {
                provider: provider.id().to_owned(),
                session,
                archive,
            },
            if archive {
                "Archive Session"
            } else {
                "Delete Session"
            },
            if archive {
                "Archive this agent session?"
            } else {
                "Permanently delete this agent session? This cannot be undone."
            },
        ));
    }

    pub fn run_session_action(
        &mut self,
        provider_id: &str,
        session: &str,
        archive: bool,
        ui: &mut Ui<'_>,
    ) {
        let Some(provider) = CliProvider::parse(provider_id) else {
            return;
        };
        let args = if archive {
            provider.archive_args(session)
        } else {
            provider.delete_args(session)
        };
        let Some(args) = args else {
            ui.status("Agents: unsupported provider action");
            return;
        };
        let sender = self.events.clone();
        let workspace = ui.workspace.to_path_buf();
        let provider_id = provider_id.to_owned();
        let session = session.to_owned();
        let cancellation = self.native_cancel.clone();
        let task = self.jobs.spawn_blocking(move || {
            let _scope = tide_ai::native_cli::CancellationScope::enter(cancellation);
            let error = ai_cli::run_session_action(provider, &workspace, &args).err();
            let _ = sender.blocking_send(AppEvent::Ai(AiEvent::SessionAction {
                provider: provider_id,
                session,
                archive,
                error,
            }));
        });
        self.native_tasks.push(task);
    }

    pub fn rename_selected_session(&mut self, ui: &mut Ui<'_>) {
        let Some((provider, id)) = self.selected_session(ui) else {
            ui.status("Agents: select a session first");
            return;
        };
        if !provider.supports_rename() {
            ui.status(format!(
                "Agents: {} does not support native rename",
                provider.display_name()
            ));
            return;
        }
        let title = self
            .agent_sessions
            .iter()
            .find(|session| session.provider_id == provider.id() && session.session_id == id)
            .map(|s| s.title.as_str())
            .unwrap_or("");
        *ui.overlay = Some(modal::prompt(
            PromptAction::RenameAgent(provider.id().into(), id),
            "Rename Agent Session",
            title,
        ));
    }

    pub fn rename_session(&mut self, provider_id: &str, id: &str, title: &str, ui: &mut Ui<'_>) {
        let Some(provider) = CliProvider::parse(provider_id) else {
            return;
        };
        let Some(executable) = self
            .cli_status
            .iter()
            .find(|s| s.provider == provider)
            .and_then(|s| s.executable.clone())
        else {
            ui.status("Agents: provider is unavailable");
            return;
        };
        let sender = self.events.clone();
        let workspace = ui.workspace.to_path_buf();
        let id = id.to_owned();
        let title = title.to_owned();
        let cancellation = self.native_cancel.clone();
        let task = self.jobs.spawn_blocking(move || {
            let _scope = tide_ai::native_cli::CancellationScope::enter(cancellation);
            let error =
                tide_ai::native_cli::rename_session(provider, &executable, &workspace, &id, &title)
                    .err();
            let _ = sender.blocking_send(AppEvent::Ai(AiEvent::CliRenamed {
                provider: provider.id().into(),
                session: id,
                title,
                error,
            }));
        });
        self.native_tasks.push(task);
    }

    pub fn merge_provider(&mut self, report: ProviderDiscovery, ui: &mut Ui<'_>) {
        if report.state == DiscoveryState::Ok {
            self.agent_candidates
                .retain(|s| s.provider_id != report.provider.id());
        }
        self.agent_candidates.extend(report.sessions.clone());
        ai_cli::normalize_sessions(&mut self.agent_candidates);
        self.agent_reports.insert(report.provider, report);
        self.rebuild_sessions(ui);
    }

    pub fn rebuild_sessions(&mut self, ui: &mut Ui<'_>) {
        let selected = self.rows().get(self.agent_row).and_then(|row| match row {
            AgentRow::Provider(provider) | AgentRow::Status(provider) => {
                Some((provider.id().to_owned(), None))
            }
            AgentRow::Session(index) => self
                .agent_sessions
                .get(*index)
                .map(|s| (s.provider_id.clone(), Some(s.session_id.clone()))),
        });
        let mut roots = vec![ui.workspace.to_path_buf()];
        if let Some(aliases) = ui
            .settings
            .get("ai.cli.workspaceAliases")
            .and_then(serde_json::Value::as_array)
        {
            roots.extend(
                aliases
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute() && p.parent().is_some()),
            );
        }
        let roots = roots
            .iter()
            .map(|p| canonical_session_path(p))
            .collect::<Vec<_>>();
        let mut sessions = self.agent_candidates.clone();
        if roots.len() == 1 {
            normalize_workspace_sessions(&mut sessions, ui.workspace);
        }
        sessions.retain(|session| {
            session.cwd.as_deref().is_some_and(|cwd| {
                Path::new(cwd).is_absolute()
                    && roots.iter().any(|root| {
                        ai_cli::normalize_session_path(Path::new(cwd)).starts_with(root)
                    })
            })
        });
        ai_cli::normalize_sessions(&mut sessions);
        sort_agent_sessions(&mut sessions);
        self.agent_sessions = sessions;
        if let Some((provider, id)) = selected {
            self.agent_row = self
                .rows()
                .iter()
                .position(|row| match row {
                    AgentRow::Provider(p) | AgentRow::Status(p) => {
                        id.is_none() && p.id() == provider
                    }
                    AgentRow::Session(index) => self.agent_sessions.get(*index).is_some_and(|s| {
                        s.provider_id == provider && Some(&s.session_id) == id.as_ref()
                    }),
                })
                .unwrap_or(0);
        }
        self.sync_selection();
        self.update_picker(ui);
    }

    pub fn update_picker(&mut self, ui: &mut Ui<'_>) {
        let items = self.session_items();
        if let Some(overlay) = &mut ui.overlay
            && *overlay.kind() == ModalKind::AgentSessions
        {
            let old = overlay.selected_item().map(|item| item.value.clone());
            overlay.base_mut().items = items;
            overlay.base_mut().refilter();
            if let Some(old) = old
                && let Some(index) = overlay
                    .base()
                    .filtered
                    .iter()
                    .position(|i| overlay.base().items[*i].value == old)
            {
                overlay.base_mut().selected = index;
            }
            overlay.base_mut().hints = Some(
                if self.agents_loading {
                    "Refreshing providers..."
                } else {
                    "Enter open · Esc cancel"
                }
                .into(),
            );
        }
    }

    pub fn session_items(&self) -> Vec<ModalItem> {
        self.agent_sessions
            .iter()
            .map(|session| {
                ModalItem::new(
                    session.title.clone(),
                    format!(
                        "{} · {}",
                        session.provider_id,
                        session.cwd.as_deref().unwrap_or("")
                    ),
                    format!("{}\t{}", session.provider_id, session.session_id),
                )
            })
            .collect()
    }

    pub fn open_workspaces(&mut self, ui: &mut Ui<'_>) {
        let mut paths = BTreeMap::<String, usize>::new();
        for session in &self.agent_candidates {
            if let Some(cwd) = &session.cwd
                && Path::new(cwd).is_absolute()
                && Path::new(cwd).parent().is_some()
            {
                *paths.entry(cwd.clone()).or_default() += 1;
            }
        }
        *ui.overlay = Some(modal::agent_workspaces(
            paths
                .into_iter()
                .map(|(path, count)| {
                    ModalItem::new(path.clone(), format!("{count} sessions"), path)
                })
                .collect(),
        ));
    }

    pub fn refresh_sessions(&mut self, ui: &mut Ui<'_>) {
        if self.agents_loading {
            return;
        }
        self.agents_loading = true;
        self.last_catalog_refresh = Instant::now();
        self.agents_error = None;
        let sender = self.events.clone();
        let workspace = ui.workspace.to_path_buf();
        let cancellation = self.native_cancel.clone();
        let discover = self.discovery;
        let task = self.jobs.spawn_blocking(move || {
            let _scope = tide_ai::native_cli::CancellationScope::enter(cancellation);
            let updates = sender.clone();
            let report = discover(&workspace, &move |provider| {
                let _ = updates.blocking_send(AppEvent::Ai(AiEvent::Provider(provider)));
            });

            let _ = sender.blocking_send(AppEvent::Ai(AiEvent::Sessions(report)));
        });
        self.native_tasks.push(task);
    }

    pub fn rows(&self) -> Vec<AgentRow> {
        let mut rows = Vec::new();
        for provider in ai_cli::CLI_PROVIDERS {
            let sessions = self
                .agent_sessions
                .iter()
                .enumerate()
                .filter(|(_, session)| {
                    session.provider_id == provider.id()
                        && (self.agent_query.is_empty()
                            || format!(
                                "{} {} {}",
                                session.title,
                                session.session_id,
                                session.model.as_deref().unwrap_or("")
                            )
                            .to_lowercase()
                            .contains(&self.agent_query.to_lowercase()))
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if sessions.is_empty()
                && !self
                    .cli_status
                    .iter()
                    .any(|status| status.provider == provider)
                && !self.agent_reports.contains_key(&provider)
            {
                continue;
            }
            rows.push(AgentRow::Provider(provider));
            if !self.agent_collapsed.contains(provider.id()) {
                if sessions.is_empty()
                    || self
                        .agent_reports
                        .get(&provider)
                        .is_some_and(|report| report.error.is_some())
                {
                    rows.push(AgentRow::Status(provider));
                }
                rows.extend(sessions.into_iter().map(AgentRow::Session));
            }
        }
        rows
    }

    pub fn sync_selection(&mut self) {
        let rows = self.rows();
        self.agent_row = self.agent_row.min(rows.len().saturating_sub(1));
    }

    pub fn toggle_provider(&mut self, provider: CliProvider, ui: &mut Ui<'_>) {
        *ui.focus = Focus::Agents;
        if !self.agent_collapsed.remove(provider.id()) {
            self.agent_collapsed.insert(provider.id().to_owned());
        }
        self.agent_row = self
            .rows()
            .iter()
            .position(|row| *row == AgentRow::Provider(provider))
            .unwrap_or(0);
        ui.mark_session_dirty();
    }

    pub fn open_session(&mut self, index: usize, ui: &mut Ui<'_>) {
        if let Some(session) = self.agent_sessions.get(index).cloned()
            && let Some(provider) = CliProvider::parse(&session.provider_id)
        {
            self.agent_row = self
                .rows()
                .iter()
                .position(|row| *row == AgentRow::Session(index))
                .unwrap_or(0);
            self.launch_cli(provider, Some(&session.session_id), ui);
        }
    }

    pub fn handle_agents_key(&mut self, key: KeyEvent, ui: &mut Ui<'_>) {
        if self.agent_search {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.agent_search = false,
                KeyCode::Backspace => {
                    let end = tide_core::grapheme_boundaries(&self.agent_query)
                        .into_iter()
                        .rev()
                        .nth(1)
                        .unwrap_or(0);
                    let byte = self
                        .agent_query
                        .char_indices()
                        .nth(end)
                        .map_or(self.agent_query.len(), |(byte, _)| byte);
                    self.agent_query.truncate(byte);
                    self.agent_row = 0;
                }
                KeyCode::Char(ch)
                    if !key.modifiers.intersects(
                        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                    ) =>
                {
                    if self.agent_query.chars().count() < 512 {
                        self.agent_query.push(ch);
                    }
                    self.agent_row = 0;
                }
                _ => {}
            }
            self.sync_selection();
            return;
        }
        let rows = self.rows();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.agent_row = self.agent_row.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                self.agent_row = (self.agent_row + 1).min(rows.len().saturating_sub(1))
            }
            KeyCode::Home => self.agent_row = 0,
            KeyCode::End => self.agent_row = rows.len().saturating_sub(1),
            KeyCode::PageUp => self.agent_row = self.agent_row.saturating_sub(10),
            KeyCode::PageDown => {
                self.agent_row = (self.agent_row + 10).min(rows.len().saturating_sub(1))
            }
            KeyCode::Enter | KeyCode::Char('o') | KeyCode::Char(' ') => {
                match rows.get(self.agent_row) {
                    Some(AgentRow::Provider(provider)) => self.toggle_provider(*provider, ui),
                    Some(AgentRow::Session(index)) => self.open_session(*index, ui),
                    Some(AgentRow::Status(provider)) => self.show_status(*provider, ui),
                    None => {}
                }
            }
            KeyCode::Left | KeyCode::Right => {
                let provider = match rows.get(self.agent_row) {
                    Some(AgentRow::Provider(provider) | AgentRow::Status(provider)) => {
                        Some(*provider)
                    }
                    Some(AgentRow::Session(index)) => {
                        CliProvider::parse(&self.agent_sessions[*index].provider_id)
                    }
                    None => None,
                };
                if let Some(provider) = provider {
                    let collapsed = self.agent_collapsed.contains(provider.id());
                    if key.code == KeyCode::Left && !collapsed
                        || key.code == KeyCode::Right && collapsed
                    {
                        self.toggle_provider(provider, ui);
                    }
                }
            }
            KeyCode::Char('/') => self.agent_search = true,
            KeyCode::Char('w') => self.open_workspaces(ui),
            KeyCode::Char('n') => self.open_cli_picker(ui),
            KeyCode::Char('d') => self.delete_selected_session(false, ui),
            KeyCode::Char('a') => self.delete_selected_session(true, ui),
            KeyCode::Char('R') => self.rename_selected_session(ui),
            KeyCode::Char('r') => self.refresh_sessions(ui),
            KeyCode::Esc => *ui.focus = Focus::Main,
            _ => {}
        }
        self.sync_selection();
    }

    pub fn show_status(&mut self, provider: CliProvider, ui: &mut Ui<'_>) {
        let installed = self
            .cli_status
            .iter()
            .any(|status| status.provider == provider && status.installed);
        let count = self
            .agent_sessions
            .iter()
            .filter(|session| session.provider_id == provider.id())
            .count();
        let discovered = self
            .agent_candidates
            .iter()
            .filter(|session| session.provider_id == provider.id())
            .count();
        let status = self
            .agent_reports
            .get(&provider)
            .and_then(|report| report.error.as_deref())
            .unwrap_or(if installed {
                "Discovery completed"
            } else {
                "Provider is not installed or is not on PATH"
            });
        *ui.overlay = Some(modal::info(
            provider.display_name(),
            &format!(
                "{status}\n\n{count} sessions associated with this workspace; {discovered} discovered overall.\n\nPress r in Agents to retry. Press w to associate a previous workspace after moving a project. No unrelated sessions are included automatically."
            ),
        ));
    }

    pub fn handle_chat_key(&mut self, key: KeyEvent, ui: &mut Ui<'_>) {
        match key.code {
            KeyCode::Enter => self.send_chat_message(ui),
            KeyCode::Backspace => {
                self.chat_draft.pop();
            }
            KeyCode::Esc => *ui.focus = Focus::Main,
            KeyCode::PageUp => self.chat_scroll = self.chat_scroll.saturating_add(10),
            KeyCode::PageDown => self.chat_scroll = self.chat_scroll.saturating_sub(10),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cancel_chat(ui);
            }
            KeyCode::Char(ch)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.chat_draft.push(ch);
            }
            _ => {}
        }
    }

    pub fn show_pending_changes(&mut self, ui: &mut Ui<'_>) {
        if let Some(set) = self.pending_change_set.as_ref() {
            let preview = set.preview();
            *ui.overlay = Some(modal::info(
                "Pending AI Changes",
                &preview.chars().take(4000).collect::<String>(),
            ));
        } else {
            ui.status("AI: no pending changes");
        }
    }

    pub fn retry_chat_turn(&mut self, ui: &mut Ui<'_>) {
        if let Some(message) = self
            .chat
            .messages
            .iter()
            .rev()
            .find(|message| message.role == tide_ai::provider::Role::User)
            .map(tide_ai::chat::ChatMessage::text)
        {
            self.chat_draft = message;
            self.send_chat_message(ui);
        } else {
            ui.status("AI: nothing to retry");
        }
    }

    pub fn show_context_usage(&mut self, ui: &mut Ui<'_>) {
        if !self.chat_available(ui) {
            return;
        }
        let messages = self.chat.messages.len();
        let context = self
            .model_info()
            .and_then(|model| model.context_window)
            .map_or_else(|| "unknown".to_owned(), |window| window.to_string());
        let usage = self
            .chat_turn
            .as_ref()
            .and_then(TurnAccumulator::usage)
            .map_or_else(
                || "0".to_owned(),
                |(input, output)| format!("{input} in / {output} out"),
            );
        ui.status(format!(
            "AI context: {messages} messages · window {context} · last turn {usage}"
        ));
    }
}

fn sort_agent_sessions(sessions: &mut [CliSession]) {
    let provider_order = |provider: &str| {
        CliProvider::parse(provider)
            .and_then(|provider| {
                ai_cli::CLI_PROVIDERS
                    .iter()
                    .position(|candidate| *candidate == provider)
            })
            .unwrap_or(usize::MAX)
    };
    sessions.sort_by(|left, right| {
        provider_order(&left.provider_id)
            .cmp(&provider_order(&right.provider_id))
            .then_with(|| ai_cli::session_updated_at(right).cmp(&ai_cli::session_updated_at(left)))
    });
}

pub(crate) fn canonical_session_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| {
        let mut result = PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    result.pop();
                }
                std::path::Component::CurDir => {}
                _ => result.push(component.as_os_str()),
            }
        }
        result
    })
}

pub(crate) fn normalize_workspace_sessions(sessions: &mut Vec<CliSession>, workspace: &Path) {
    let workspace = canonical_session_path(workspace);
    sessions.retain(|session| {
        session.cwd.as_deref().is_some_and(|cwd| {
            let cwd = Path::new(cwd);
            cwd.is_absolute() && ai_cli::normalize_session_path(cwd).starts_with(&workspace)
        })
    });
    ai_cli::normalize_sessions(sessions);
    sort_agent_sessions(sessions);
}

/// Finds native AI CLI sessions for the Agents sidebar, reporting each provider as it finishes.
pub type AgentDiscovery = fn(&Path, &(dyn Fn(ProviderDiscovery) + Sync)) -> DiscoveryReport;

/// Discovers sessions from the installed AI CLIs and their stores under `HOME`.
pub fn discover_agent_sessions(
    workspace: &Path,
    update: &(dyn Fn(ProviderDiscovery) + Sync),
) -> DiscoveryReport {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return DiscoveryReport {
            state: Some(DiscoveryState::Error),
            errors: vec!["HOME is not set".to_owned()],
            ..DiscoveryReport::default()
        };
    };
    ai_cli::discover_all_with_updates(&home, workspace, update)
}

#[derive(Debug, Clone)]
pub struct ChatToolResult {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
}
