//! Durable, provider-neutral chat sessions.
//!
//! Sessions never contain API keys or request headers. Interrupted turns are
//! persisted so the UI can offer recovery after a crash or restart.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::provider::{ChatTurn, Role, StopReason, StreamEvent};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Reasoning {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input_json: String,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
    Error {
        message: String,
    },
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn text_content(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub id: String,
    pub role: Role,
    pub blocks: Vec<ContentBlock>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
}

impl ChatMessage {
    pub fn new(role: Role, blocks: Vec<ContentBlock>) -> Self {
        Self {
            id: new_id(),
            role,
            blocks,
            created_at: now(),
            stop_reason: None,
        }
    }

    pub fn text(&self) -> String {
        let mut text = String::new();
        for block in &self.blocks {
            match block {
                ContentBlock::Text { text: value } => text.push_str(value),
                ContentBlock::Reasoning { text: value } => {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str("[reasoning] ");
                    text.push_str(value);
                }
                ContentBlock::ToolUse {
                    name, input_json, ..
                } => {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&format!("[tool call {name}: {input_json}]"));
                }
                ContentBlock::ToolResult { content, .. } => {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str("[tool result] ");
                    text.push_str(content);
                }
                ContentBlock::Error { message } => {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str("[error] ");
                    text.push_str(message);
                }
            }
        }
        text
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatSession {
    pub id: String,
    pub title: String,
    pub provider: String,
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub created_at: String,
    pub updated_at: String,
    /// Present while a turn was in flight when the app stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interrupted: Option<InterruptedTurn>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterruptedTurn {
    pub message_id: String,
    pub text: String,
    pub started_at: String,
}

impl ChatSession {
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        let now = now();
        Self {
            id: new_id(),
            title: "New chat".to_owned(),
            provider: provider.into(),
            model: model.into(),
            messages: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
            interrupted: None,
        }
    }

    pub fn push_user_text(&mut self, text: impl Into<String>) {
        let text = text.into();
        if self.title == "New chat" {
            self.title = title_from_prompt(&text);
        }
        self.messages
            .push(ChatMessage::new(Role::User, vec![ContentBlock::text(text)]));
        self.touch();
    }

    /// Convert the transcript into provider turns. Reasoning and tool blocks are
    /// flattened to text so every provider can replay a conversation.
    pub fn provider_turns(&self) -> Vec<ChatTurn> {
        self.messages
            .iter()
            .map(|message| ChatTurn {
                role: message.role,
                text: message.text(),
            })
            .filter(|turn| !turn.text.is_empty())
            .collect()
    }

    pub fn last_assistant(&self) -> Option<&ChatMessage> {
        self.messages
            .iter()
            .rev()
            .find(|message| message.role == Role::Assistant)
    }

    pub fn touch(&mut self) {
        self.updated_at = now();
    }

    pub fn finish_turn(&mut self, message: &mut ChatMessage, stop_reason: StopReason) {
        message.stop_reason = Some(stop_reason);
        self.interrupted = None;
        self.touch();
    }
}

/// Assemble streamed events into one assistant message.
#[derive(Debug, Default)]
pub struct TurnAccumulator {
    pub blocks: Vec<ContentBlock>,
    text: String,
    reasoning: String,
    tool_input: String,
    stop_reason: StopReason,
    usage: Option<(u64, u64)>,
}

impl TurnAccumulator {
    pub fn new() -> Self {
        Self {
            stop_reason: StopReason::EndTurn,
            ..Self::default()
        }
    }

    pub fn apply(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::MessageStart { .. } => {}
            StreamEvent::TextDelta { text } => {
                self.flush_reasoning();
                self.text.push_str(&text);
            }
            StreamEvent::ReasoningDelta { text } => self.reasoning.push_str(&text),
            StreamEvent::ToolUse {
                id,
                name,
                input_json,
            } => {
                self.flush_text();
                self.flush_reasoning();
                if !name.is_empty() {
                    self.flush_tool();
                    self.block_tool_start(id, name);
                }
                self.tool_input.push_str(&input_json);
            }
            StreamEvent::Usage {
                input_tokens,
                output_tokens,
            } => self.usage = Some((input_tokens, output_tokens)),
            StreamEvent::MessageStop { stop_reason } => {
                self.stop_reason = stop_reason;
            }
        }
    }

    fn block_tool_start(&mut self, id: String, name: String) {
        self.blocks.push(ContentBlock::ToolUse {
            id,
            name,
            input_json: String::new(),
        });
    }

    fn flush_text(&mut self) {
        if !self.text.is_empty() {
            self.blocks.push(ContentBlock::Text {
                text: std::mem::take(&mut self.text),
            });
        }
    }

    fn flush_reasoning(&mut self) {
        if !self.reasoning.is_empty() {
            self.blocks.push(ContentBlock::Reasoning {
                text: std::mem::take(&mut self.reasoning),
            });
        }
    }

    fn flush_tool(&mut self) {
        if self.tool_input.is_empty() {
            return;
        }
        let input = std::mem::take(&mut self.tool_input);
        if let Some(ContentBlock::ToolUse {
            input_json: stored, ..
        }) = self
            .blocks
            .iter_mut()
            .rev()
            .find(|block| matches!(block, ContentBlock::ToolUse { .. }))
        {
            stored.push_str(&input);
        }
    }

    pub fn finish(mut self) -> (ChatMessage, StopReason, Option<(u64, u64)>) {
        self.flush_text();
        self.flush_reasoning();
        self.flush_tool();
        let mut message = ChatMessage::new(Role::Assistant, self.blocks);
        message.stop_reason = Some(self.stop_reason);
        (message, self.stop_reason, self.usage)
    }

    pub fn usage(&self) -> Option<(u64, u64)> {
        self.usage
    }

    pub fn mark_error(&mut self, message: impl Into<String>) {
        self.flush_text();
        self.flush_reasoning();
        self.blocks.push(ContentBlock::Error {
            message: message.into(),
        });
        self.stop_reason = StopReason::Error;
    }
}

#[derive(Debug, Clone)]
pub struct ChatStore {
    dir: PathBuf,
}

impl ChatStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn save(&self, session: &ChatSession) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|error| error.to_string())?;
        let path = self.path_for(&session.id);
        let text = serde_json::to_string_pretty(session).map_err(|error| error.to_string())?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, text).map_err(|error| error.to_string())?;
        std::fs::rename(&temp, &path).map_err(|error| error.to_string())
    }

    pub fn load(&self, id: &str) -> Result<ChatSession, String> {
        let text = std::fs::read_to_string(self.path_for(id)).map_err(|error| error.to_string())?;
        serde_json::from_str(&text).map_err(|error| error.to_string())
    }

    pub fn delete(&self, id: &str) -> bool {
        std::fs::remove_file(self.path_for(id)).is_ok()
    }

    pub fn list(&self) -> Vec<ChatSessionSummary> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut sessions = entries
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .filter_map(|entry| {
                let text = std::fs::read_to_string(entry.path()).ok()?;
                let session: ChatSession = serde_json::from_str(&text).ok()?;
                Some(ChatSessionSummary {
                    id: session.id,
                    title: session.title,
                    provider: session.provider,
                    model: session.model,
                    updated_at: session.updated_at,
                    message_count: session.messages.len(),
                })
            })
            .collect::<Vec<_>>();
        sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        sessions
    }

    fn path_for(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatSessionSummary {
    pub id: String,
    pub title: String,
    pub provider: String,
    pub model: String,
    pub updated_at: String,
    pub message_count: usize,
}

pub fn title_from_prompt(prompt: &str) -> String {
    let first_line = prompt.lines().next().unwrap_or_default().trim();
    let mut title = first_line.chars().take(60).collect::<String>();
    if first_line.chars().count() > 60 {
        title.push('…');
    }
    if title.is_empty() {
        "New chat".to_owned()
    } else {
        title
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn new_id() -> String {
    use rand::RngCore as _;
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    let hex = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("chat-{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::StreamEvent;

    #[test]
    fn accumulator_assembles_text_reasoning_and_tools() {
        let mut accumulator = TurnAccumulator::new();
        accumulator.apply(StreamEvent::MessageStart {
            id: "m".to_owned(),
            model: "gpt".to_owned(),
        });
        accumulator.apply(StreamEvent::ReasoningDelta {
            text: "thinking".to_owned(),
        });
        accumulator.apply(StreamEvent::TextDelta {
            text: "Hello ".to_owned(),
        });
        accumulator.apply(StreamEvent::TextDelta {
            text: "world".to_owned(),
        });
        accumulator.apply(StreamEvent::ToolUse {
            id: "call-1".to_owned(),
            name: "read_file".to_owned(),
            input_json: "{\"path\":".to_owned(),
        });
        accumulator.apply(StreamEvent::ToolUse {
            id: String::new(),
            name: String::new(),
            input_json: "\"a\"}".to_owned(),
        });
        accumulator.apply(StreamEvent::Usage {
            input_tokens: 5,
            output_tokens: 9,
        });
        accumulator.apply(StreamEvent::MessageStop {
            stop_reason: StopReason::ToolUse,
        });
        let (message, stop, usage) = accumulator.finish();
        let replay = message.text();
        assert!(replay.contains("Hello world"), "{replay}");
        assert!(replay.contains("[tool call read_file"), "{replay}");
        assert_eq!(stop, StopReason::ToolUse);
        assert_eq!(usage, Some((5, 9)));
        assert!(message.blocks.contains(&ContentBlock::Reasoning {
            text: "thinking".to_owned()
        }));
        assert!(message.blocks.contains(&ContentBlock::ToolUse {
            id: "call-1".to_owned(),
            name: "read_file".to_owned(),
            input_json: "{\"path\":\"a\"}".to_owned(),
        }));
    }

    #[test]
    fn errors_are_recorded_in_the_transcript() {
        let mut accumulator = TurnAccumulator::new();
        accumulator.apply(StreamEvent::TextDelta {
            text: "partial".to_owned(),
        });
        accumulator.mark_error("network down");
        let (message, stop, _) = accumulator.finish();
        assert_eq!(stop, StopReason::Error);
        assert!(message.blocks.iter().any(
            |block| matches!(block, ContentBlock::Error { message } if message == "network down")
        ));
    }

    #[test]
    fn sessions_persist_and_list_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let store = ChatStore::new(dir.path());
        let mut first = ChatSession::new("anthropic", "claude-sonnet");
        first.push_user_text("Explain the build system");
        store.save(&first).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let mut second = ChatSession::new("openai", "gpt-4o");
        second.push_user_text("Write a test");
        store.save(&second).unwrap();

        let loaded = store.load(&first.id).unwrap();
        assert_eq!(loaded.title, "Explain the build system");
        assert_eq!(loaded.provider_turns().len(), 1);

        let list = store.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, second.id);
        assert!(store.delete(&first.id));
        assert_eq!(store.list().len(), 1);
    }

    #[test]
    fn tool_results_are_replayed_to_providers() {
        let message = ChatMessage::new(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: "call-1".to_owned(),
                content: "file contents".to_owned(),
                is_error: false,
            }],
        );
        assert_eq!(message.text(), "[tool result] file contents");
    }

    #[test]
    fn titles_are_truncated_and_blank_inputs_are_replaced() {
        assert_eq!(title_from_prompt("  hi there\nsecond"), "hi there");
        assert_eq!(title_from_prompt("   "), "New chat");
        let long = "a".repeat(80);
        assert_eq!(title_from_prompt(&long).chars().count(), 61);
    }

    #[test]
    fn api_keys_are_not_serialized_into_sessions() {
        let mut session = ChatSession::new("openai", "gpt-4o");
        session.push_user_text("hello");
        let text = serde_json::to_string(&session).unwrap();
        assert!(!text.contains("api_key"));
        assert!(!text.contains("authorization"));
    }
}
