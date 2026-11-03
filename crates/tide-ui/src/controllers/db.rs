//! Databases: saved connections, cached PostgreSQL sessions, query history, the SQL tabs'
//! requests, row edits and the confirmations around them.
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tide_ai::secrets::{SecretOptions, SecretService};
use tide_config::{Paths, Settings};
use tide_core::truncate_to_width;
use tide_db::{
    ConnectionConfig, ConnectionScope, ConnectionStore, DbError, PostgresSession, QueryHistory,
    QueryResult, QueryStatus, TableInfo, classify_sql_risk, complete_sql, generate_id,
    parse_postgres_url, result_to_csv,
};
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;

use super::{Effect, Ui};
use crate::app::{AppEvent, Focus};
use crate::command::Command;
use crate::connection_form::{ConnectionAction, Submission};
use crate::jobs::Jobs;
use crate::modal::{
    self, ConfirmAction, HistoryControl, Modal, ModalItem, PromptAction, Submitted,
};
use crate::sql_tab::SqlTab;
use crate::workbench::Tab;

type Sessions = Arc<tokio::sync::Mutex<HashMap<String, Arc<PostgresSession>>>>;

type InsertOutcome = Result<
    (
        BTreeMap<String, serde_json::Value>,
        Result<QueryResult, String>,
    ),
    String,
>;

#[derive(Debug)]
pub enum DbEvent {
    InsertPrepared {
        connection_id: String,
        query_id: String,
        result: Result<tide_db::InsertTarget, String>,
    },
    RowInserted {
        request: tide_db::RowInsert,
        result: Box<InsertOutcome>,
    },
    RowSaved {
        request: tide_db::RowMutation,
        result: Result<Option<BTreeMap<String, serde_json::Value>>, String>,
    },
    Completed {
        generation: u64,
        connection_id: String,
        result: Box<QueryResult>,
    },
    Failed {
        generation: u64,
        connection_id: String,
        message: String,
    },
    Catalog {
        generation: u64,
        connection_id: String,
        items: Vec<tide_db::CatalogItem>,
    },
    Ddl {
        generation: u64,
        title: String,
        ddl: String,
    },
    Tested {
        name: String,
        success: bool,
        error: Option<String>,
        latency_ms: u64,
        server_version: Option<String>,
        /// For example, `sslmode=prefer` falling back to plaintext.
        warning: Option<String>,
    },
}

pub(crate) struct DbController {
    pub store: ConnectionStore,
    pub history: QueryHistory,
    sessions: Sessions,
    generation: u64,
    schema_connection: Option<String>,
    schema_tables: Vec<TableInfo>,
    /// The schema browser describes the chosen table instead of querying it.
    pub describe_mode: bool,
    pub tasks: Vec<(String, JoinHandle<()>)>,
    /// A connection waiting for the secret store to unlock.
    pending_connection: Option<Submission>,
    history_favorites: bool,
    pub history_connection: Option<String>,
    pub secrets: Arc<SecretService>,
    /// The libpq password file consulted when Tide has no password stored.
    pub pgpass: Option<PathBuf>,
    jobs: Jobs,
    events: Sender<AppEvent>,
}

impl DbController {
    pub fn load(
        paths: &Paths,
        workspace: &Path,
        settings: &Settings,
        secrets: Arc<SecretService>,
        jobs: &Jobs,
        events: Sender<AppEvent>,
    ) -> Self {
        let mut store = ConnectionStore::new(
            paths.connections_file.clone(),
            Some(paths.workspace_connections_file(workspace)),
        );
        if let Err(error) = store.load() {
            tracing::warn!(%error, "could not load database connections");
        }
        let history_enabled = settings
            .get("session.databaseQueryHistory")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        let history =
            QueryHistory::new(paths.query_history_dir.clone()).with_enabled(history_enabled);
        if let Err(error) = history.init() {
            tracing::warn!(%error, "could not initialize query history");
        }
        Self {
            store,
            history,
            sessions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            generation: 0,
            schema_connection: None,
            schema_tables: Vec::new(),
            describe_mode: false,
            tasks: Vec::new(),
            pending_connection: None,
            history_favorites: false,
            history_connection: None,
            secrets,
            pgpass: tide_db::pgpass_path(),
            jobs: jobs.clone(),
            events,
        }
    }

    /// Stops outstanding requests; PostgreSQL rolls unfinished row transactions back when
    /// their client is dropped.
    pub async fn stop_requests(&mut self) {
        for (_, task) in self.tasks.drain(..) {
            task.abort();
            let _ = task.await;
        }
    }

    pub async fn close_sessions(&self) {
        let sessions = std::mem::take(&mut *self.sessions.lock().await);
        for session in sessions.into_values() {
            session.cancel().await;
        }
    }

    pub fn prune_tasks(&mut self) {
        self.tasks.retain(|(_, task)| !task.is_finished());
    }

    /// Reopens the connection form when the unlock prompt it was waiting on is dismissed.
    pub fn restore_pending_form(&mut self, ui: &mut Ui<'_>) {
        if let Some(submission) = self.pending_connection.take() {
            let mut form = crate::connection_form::ConnectionForm::new(submission.config);
            if let Some(password) = submission.password {
                form.set_password(&password);
            }
            *ui.overlay = Some(Box::new(form));
        }
    }

    /// The password for `config`: the one Tide stored for its destination, or else the
    /// one your libpq password file (`PGPASSFILE` or `~/.pgpass`) has for it.
    pub(crate) fn password(&self, config: &ConnectionConfig) -> Option<String> {
        self.store
            .credential_reference(config)
            .and_then(|key| self.secrets.get(key))
            .or_else(|| {
                self.pgpass
                    .as_deref()
                    .and_then(|path| tide_db::pgpass_password(path, config))
            })
    }

    pub fn handle_event(&mut self, event: DbEvent, ui: &mut Ui<'_>) {
        match event {
            DbEvent::InsertPrepared {
                connection_id,
                query_id,
                result,
            } => {
                let current = matches!(ui.workbench.active_tab(), Some(Tab::Sql(sql)) if sql.result.as_ref().is_some_and(|r| r.query_id == query_id && r.connection_id == connection_id));
                if current {
                    match result {
                        Ok(target) => {
                            *ui.overlay =
                                Some(Box::new(crate::row_insert_form::RowInsertForm::new(
                                    connection_id,
                                    query_id,
                                    target,
                                )))
                        }
                        Err(error) => ui.status(format!("Database: {error}")),
                    }
                }
            }
            DbEvent::RowInserted { request, result } => {
                let result = *result;
                let owns_form = ui
                    .overlay
                    .as_ref()
                    .and_then(|o| o.submitted_insert())
                    .is_some_and(|r| {
                        r.query_id == request.query_id && r.connection_id == request.connection_id
                    });
                for tab in ui.workbench.tabs_mut() {
                    if let Tab::Sql(sql) = tab
                        && sql.result.as_ref().is_some_and(|r| {
                            r.query_id == request.query_id
                                && r.connection_id == request.connection_id
                        })
                    {
                        sql.running = false;
                        if let Ok((inserted, Ok(refreshed))) = &result {
                            let primary_key = sql
                                .result
                                .as_ref()
                                .and_then(|r| r.row_target.as_ref())
                                .map(|t| t.primary_key.clone())
                                .unwrap_or_default();
                            sql.set_result(refreshed.clone());
                            if !primary_key.is_empty()
                                && let Some(index) = sql.result.as_ref().and_then(|r| {
                                    r.rows.iter().position(|row| {
                                        primary_key
                                            .iter()
                                            .all(|key| row.get(key) == inserted.get(key))
                                    })
                                })
                            {
                                sql.selected_row = index;
                            }
                        }
                    }
                }
                match result {
                    Ok((_, refreshed)) => {
                        if owns_form {
                            *ui.overlay = None;
                        }
                        ui.status(match refreshed { Ok(_) => "Database: row added".into(), Err(e) => format!("Database: row added; refresh failed: {e}. Refresh manually; do not retry the insert.") });
                    }
                    Err(error) => {
                        if owns_form {
                            *ui.overlay =
                                Some(Box::new(crate::row_insert_form::RowInsertForm::restore(
                                    request,
                                    error.clone(),
                                )));
                        }
                        ui.status(format!("Database: add row failed: {error}"));
                    }
                }
            }
            DbEvent::RowSaved { request, result } => match result {
                Ok(row) => {
                    for tab in ui.workbench.tabs_mut() {
                        if let Tab::Sql(sql) = tab
                            && sql.result.as_ref().is_some_and(|r| {
                                r.query_id == request.query_id
                                    && r.connection_id == request.connection_id
                            })
                        {
                            sql.apply_row_change(&request.original, row.clone());
                        }
                    }
                    ui.status(if request.delete {
                        "Database: row deleted"
                    } else {
                        "Database: row saved"
                    });
                }
                Err(error) => {
                    for tab in ui.workbench.tabs_mut() {
                        if let Tab::Sql(sql) = tab
                            && sql
                                .result
                                .as_ref()
                                .is_some_and(|r| r.query_id == request.query_id)
                        {
                            sql.running = false;
                        }
                    }
                    ui.status(format!("Database: {error}"));
                }
            },
            DbEvent::Completed {
                generation,
                connection_id,
                result,
            } => {
                let mut recorded = None;
                for tab in ui.workbench.tabs_mut() {
                    if let Tab::Sql(sql) = tab
                        && sql.generation == generation
                    {
                        recorded = Some((
                            connection_id.clone(),
                            result.sql.clone(),
                            result.duration_ms,
                            result.row_count,
                            result.affected_rows,
                            result.notices.first().cloned(),
                        ));
                        sql.set_result((*result).clone());
                    }
                }
                if let Some((connection_id, sql, duration_ms, row_count, affected, notice)) =
                    recorded
                {
                    let name = self
                        .store
                        .get(&connection_id)
                        .map(|config| config.name.clone())
                        .unwrap_or_default();
                    let _ = self.history.add_entry(
                        &connection_id,
                        &name,
                        &sql,
                        duration_ms.into(),
                        row_count,
                        QueryStatus::Success,
                        None,
                    );
                    ui.status(format!(
                        "Database: {row_count} row(s){} in {duration_ms} ms{}",
                        affected.map_or_else(String::new, |rows| format!(" ({rows} affected)")),
                        notice.map_or_else(String::new, |notice| format!(" · ⚠ {notice}"))
                    ));
                }
                ui.mark_session_dirty();
            }
            DbEvent::Failed {
                generation,
                connection_id,
                message,
            } => {
                for tab in ui.workbench.tabs_mut() {
                    if let Tab::Sql(sql) = tab
                        && sql.generation == generation
                    {
                        sql.set_error(message.clone());
                        let name = self
                            .store
                            .get(&connection_id)
                            .map(|config| config.name.clone())
                            .unwrap_or_default();
                        let _ = self.history.add_entry(
                            &connection_id,
                            &name,
                            &sql.sql(),
                            0,
                            0,
                            QueryStatus::Error,
                            Some(&message),
                        );
                    }
                }
                ui.status(format!("Database: {message}"));
            }
            DbEvent::Catalog {
                generation,
                connection_id,
                items,
            } => {
                if generation == self.generation {
                    self.schema_connection = Some(connection_id);
                    self.schema_tables = items
                        .iter()
                        .filter(|i| matches!(i.kind.as_str(), "table" | "view"))
                        .map(|i| TableInfo {
                            schema: i.schema.clone(),
                            name: i.name.clone(),
                            kind: if i.kind == "view" {
                                tide_db::TableType::View
                            } else {
                                tide_db::TableType::Table
                            },
                            row_count: None,
                            size_bytes: None,
                            comment: None,
                        })
                        .collect();
                    *ui.overlay = Some(Box::new(crate::schema_browser::SchemaBrowser::new(items)));
                }
            }
            DbEvent::Ddl {
                generation,
                title,
                ddl,
            } => {
                if generation == self.generation {
                    *ui.overlay = Some(modal::info(&title, &ddl));
                }
            }
            DbEvent::Tested {
                name,
                success,
                error,
                latency_ms,
                server_version,
                warning,
            } => {
                if success {
                    ui.status(format!(
                        "Database: {name} OK · {latency_ms} ms · {}{}",
                        server_version.unwrap_or_else(|| "unknown version".to_owned()),
                        warning.map_or_else(String::new, |warning| format!(" · ⚠ {warning}"))
                    ));
                } else {
                    ui.status(format!(
                        "Database: {name} failed · {}",
                        error.unwrap_or_else(|| "unknown error".to_owned())
                    ));
                }
            }
        }
    }

    /// Handles the database parts of a modal submission and hands back anything else.
    pub fn submit(
        &mut self,
        submitted: Submitted,
        mut overlay: Box<dyn Modal>,
        ui: &mut Ui<'_>,
    ) -> Option<(Submitted, Box<dyn Modal>)> {
        match submitted {
            Submitted::RowInsert(request) => {
                overlay.base_mut().loading = true;
                *ui.overlay = Some(overlay);
                self.execute_insert(*request, ui);
            }
            Submitted::Connection(submission) => self.submit_connection_form(*submission, ui),
            Submitted::DatabaseConnection(id) => self.select_connection(&id, ui),
            Submitted::CatalogItem(item) => {
                if matches!(item.kind.as_str(), "table" | "view") {
                    if self.describe_mode {
                        self.describe_mode = false;
                        self.describe_table(&item.schema, &item.table, ui);
                    } else {
                        let identifier = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
                        let sql = format!(
                            "SELECT * FROM {}.{} LIMIT 100;",
                            identifier(&item.schema),
                            identifier(&item.table)
                        );
                        self.open_tab(Some(&sql), ui);
                        if let Some(connection) = self.schema_connection.clone() {
                            self.select_connection(&connection, ui);
                        }
                    }
                } else {
                    *ui.overlay = Some(modal::info(
                        &format!("{} · {}", item.kind, item.name),
                        &item.detail,
                    ));
                }
            }
            Submitted::HistoryEntry(id) => self.load_history_entry(&id, ui),
            Submitted::HistoryControl { control, filter } => {
                match control {
                    HistoryControl::Favorite(id) => {
                        if let Some(id) = id
                            && let Ok(entries) = self.history.entries()
                            && let Some(entry) = entries.iter().find(|e| e.id == id)
                            && let Err(error) = self.history.set_favorite(&id, !entry.is_favorite)
                        {
                            ui.status(error);
                        }
                    }
                    HistoryControl::Favorites => {
                        self.history_favorites = !self.history_favorites;
                    }
                    HistoryControl::Connection => {
                        self.history_connection = if self.history_connection.is_some() {
                            None
                        } else {
                            self.active_connection(ui)
                        }
                    }
                    HistoryControl::Clear => {
                        *ui.overlay = Some(modal::confirm(
                            ConfirmAction::ClearQueryHistory(self.history_connection.clone()),
                            "Clear Query History",
                            "Clear the selected connection history, or all history when no connection filter is active? Favorites in that scope are also removed.",
                        ));
                        return None;
                    }
                }
                self.open_history(ui);
                if let Some(next) = ui.overlay.as_mut() {
                    next.base_mut().push_str(&filter);
                }
            }
            Submitted::RowCell {
                query,
                column,
                input,
                save,
            } => match serde_json::from_str::<serde_json::Value>(&input) {
                Ok(value) => {
                    if let Some(Tab::Sql(sql)) = ui.workbench.active_tab_mut()
                        && sql.result.as_ref().is_some_and(|r| r.query_id == query)
                    {
                        sql.row_edits.insert(column, value);
                        if save {
                            self.save_row(false, ui);
                        }
                    }
                }
                Err(error) => {
                    ui.status(format!("Database: invalid JSON value: {error}"));
                    if save {
                        *ui.overlay = Some(overlay);
                    }
                }
            },
            Submitted::Prompt(PromptAction::NewConnection, input) => {
                let dsn = input;
                self.add_connection(&dsn, ui);
            }
            Submitted::Prompt(PromptAction::EditConnection(id), input) => {
                let dsn = input;
                self.edit_connection(&id, &dsn, ui);
            }
            Submitted::Prompt(PromptAction::ExportResults, input) => {
                let path = input;
                self.export_results(&path, ui);
            }
            Submitted::Prompt(PromptAction::UnlockSecrets, input) => {
                match self.secrets.unlock(&input) {
                    Ok(()) => ui.status("Secrets: encrypted store unlocked"),
                    Err(error) => ui.status(format!("Secrets: {error}")),
                }
            }
            Submitted::Prompt(PromptAction::UnlockConnectionSecrets, input) => {
                match self.secrets.unlock(&input) {
                    Ok(()) => {
                        if let Some(submission) = self.pending_connection.take() {
                            self.submit_connection_form(submission, ui);
                        }
                    }
                    Err(error) => {
                        ui.status(format!("Database: {error}"));
                        *ui.overlay = Some(modal::prompt(
                            PromptAction::UnlockConnectionSecrets,
                            "Unlock Secrets to Save Connection",
                            "",
                        ));
                    }
                }
            }
            Submitted::Confirm(ConfirmAction::RunSql(sql)) => {
                self.execute_text(sql, false, ui);
            }
            Submitted::Confirm(ConfirmAction::DeleteRow(request)) => {
                self.execute_row(*request, ui);
            }
            Submitted::Confirm(ConfirmAction::ClearQueryHistory(connection)) => {
                match self.history.clear(connection.as_deref()) {
                    Ok(()) => {
                        ui.status("Database: history cleared");
                        self.open_history(ui);
                    }
                    Err(error) => ui.status(error),
                }
            }
            Submitted::Confirm(ConfirmAction::DeleteConnection(id)) => {
                self.delete_connection(&id, ui);
            }
            Submitted::Confirm(ConfirmAction::ExportResults(path)) => {
                self.export_results_to(path, true, ui);
            }
            other => return Some((other, overlay)),
        }
        None
    }

    /// Reports the unlocked secrets store, or asks for its master password.
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

    pub fn open_tab(&mut self, content: Option<&str>, ui: &mut Ui<'_>) {
        let mut tab = content.map_or_else(SqlTab::new, SqlTab::from_content);
        if let Some(connection) = self.active_connection(ui) {
            let name = self
                .store
                .get(&connection)
                .map(|config| config.name.clone());
            tab.read_only = self.store.get(&connection).is_none_or(|c| c.read_only);
            tab.set_connection(Some(connection), name);
        }
        ui.workbench.insert_tab_focused(Tab::Sql(tab));
        *ui.focus = Focus::Main;
        ui.mark_session_dirty();
    }

    pub fn active_connection(&self, ui: &Ui<'_>) -> Option<String> {
        match ui.workbench.active_tab() {
            Some(Tab::Sql(tab)) => tab.connection_id.clone(),
            _ => self
                .store
                .connections()
                .first()
                .map(|connection| connection.id.clone()),
        }
    }

    /// The active SQL tab's connection, else the first saved one.
    fn target_connection(&self, ui: &Ui<'_>) -> Option<String> {
        ui.workbench
            .active_tab()
            .and_then(|tab| match tab {
                Tab::Sql(tab) => tab.connection_id.clone(),
                _ => None,
            })
            .or_else(|| {
                self.store
                    .connections()
                    .first()
                    .map(|connection| connection.id.clone())
            })
    }

    pub fn has_capacity(&mut self, ui: &mut Ui<'_>) -> bool {
        self.tasks.retain(|(_, task)| !task.is_finished());
        if self.tasks.len() >= 64 {
            ui.status("Database request queue is full");
            false
        } else {
            true
        }
    }

    pub fn spawn(
        &mut self,
        connection: String,
        future: impl std::future::Future<Output = ()> + Send + 'static,
        ui: &mut Ui<'_>,
    ) -> bool {
        if !self.has_capacity(ui) {
            return false;
        }
        self.tasks.push((connection, self.jobs.spawn(future)));
        true
    }

    pub fn cancel_requests(&mut self, id: &str, ui: &mut Ui<'_>) {
        self.tasks.retain(|(owner, task)| {
            if owner == id {
                task.abort();
                false
            } else {
                true
            }
        });
        let session = self
            .sessions
            .try_lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(id));
        if let Some(session) = session {
            self.spawn(
                id.into(),
                async move {
                    let _ = tokio::time::timeout(Duration::from_secs(2), session.cancel()).await;
                },
                ui,
            );
        }
        self.generation = self.generation.wrapping_add(1);
        for tab in ui.workbench.tabs_mut() {
            if let Tab::Sql(sql) = tab
                && sql.connection_id.as_deref() == Some(id)
            {
                sql.generation = self.generation;
                sql.running = false;
            }
        }
    }

    pub fn open_connection_form(&mut self, config: Option<ConnectionConfig>, ui: &mut Ui<'_>) {
        *ui.overlay = Some(Box::new(crate::connection_form::ConnectionForm::new(
            config.unwrap_or_else(|| ConnectionConfig {
                database: "postgres".into(),
                username: "postgres".into(),
                ..ConnectionConfig::default()
            }),
        )));
    }

    pub fn submit_connection_form(&mut self, submission: Submission, ui: &mut Ui<'_>) {
        let Submission {
            mut config,
            password,
            action,
        } = submission;
        let existing = self.store.get(&config.id).cloned();
        if action == ConnectionAction::Delete {
            *ui.overlay = Some(modal::confirm(
                ConfirmAction::DeleteConnection(config.id),
                "Delete Connection",
                &format!("Delete {}?", config.name),
            ));
            return;
        }
        if let Err(error) = config.validate() {
            ui.status(error.to_string());
            return;
        }
        if config.scope == ConnectionScope::Project {
            // The project file is shared; its password goes to your own credentials below.
            config.project_path = Some(ui.workspace.display().to_string());
            config.password_secret = None;
        } else {
            config.project_path = None;
        }
        // Retain a stored password only while its approved destination is unchanged.
        config.password_secret = existing
            .as_ref()
            .filter(|old| {
                old.scope == ConnectionScope::Global
                    && config.scope == ConnectionScope::Global
                    && old.same_destination(&config)
            })
            .and_then(|old| old.password_secret.clone());
        if action == ConnectionAction::Test {
            let supplied = password.clone().or_else(|| self.password(&config));
            let mut form = crate::connection_form::ConnectionForm::new(config.clone());
            if let Some(password) = &password {
                form.set_password(password);
            }
            *ui.overlay = Some(Box::new(form));
            let sender = self.events.clone();
            ui.status(format!("Database: testing {}…", config.name));
            self.spawn(
                config.id.clone(),
                async move {
                    let result = tide_db::test_connection(&config, supplied.as_deref()).await;
                    let _ = sender
                        .send(AppEvent::Db(DbEvent::Tested {
                            name: config.name,
                            success: result.success,
                            error: result.error,
                            latency_ms: result.latency_ms,
                            server_version: result.server_version,
                            warning: result.warning,
                        }))
                        .await;
                },
                ui,
            );
            return;
        }
        if config.id.is_empty() {
            config.id = generate_id(&config);
        }
        let mut fresh_secret = None;
        if let Some(password) = password {
            static NEXT_SECRET: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let key = format!(
                "db.{}.{stamp}-{}.password",
                config.id,
                NEXT_SECRET.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            if let Err(error) = self.secrets.set(&key, &password, &SecretOptions::default()) {
                if !self.secrets.is_unlocked()
                    && matches!(
                        error,
                        tide_ai::secrets::SecretError::Locked
                            | tide_ai::secrets::SecretError::NoWritableProvider
                    )
                {
                    self.pending_connection = Some(Submission {
                        config,
                        password: Some(password),
                        action: ConnectionAction::Save,
                    });
                    *ui.overlay = Some(modal::prompt(
                        PromptAction::UnlockConnectionSecrets,
                        "Unlock Secrets to Save Connection",
                        "",
                    ));
                    ui.status("Database: unlock the encrypted store; a new store uses this encryption password");
                } else {
                    let mut form = crate::connection_form::ConnectionForm::new(config);
                    form.set_password(&password);
                    *ui.overlay = Some(Box::new(form));
                    ui.status(format!("Database: password was not saved: {error}"));
                }
                return;
            }
            if config.scope == ConnectionScope::Global {
                config.password_secret = Some(key.clone());
            }
            fresh_secret = Some(key);
        }
        let saved = if existing.is_some() {
            self.store.update(config.clone())
        } else {
            self.store.add(config.clone()).map(|_| ())
        };
        let saved = saved.and_then(|()| match &fresh_secret {
            Some(key) if config.scope == ConnectionScope::Project => self
                .store
                .save_project_credential(&config, key)
                .map(|replaced| {
                    if let Some(old) = replaced {
                        let _ = self.secrets.delete(&old);
                    }
                }),
            _ => Ok(()),
        });
        if let Err(error) = saved {
            if let Some(key) = fresh_secret {
                let _ = self.secrets.delete(&key);
            }
            ui.status(format!("Database: {error}"));
            return;
        }
        self.cancel_requests(&config.id, ui);
        self.select_connection(&config.id, ui);
        ui.status(format!("Database: saved {}", config.name));
    }

    pub fn open_connection_picker(&mut self, ui: &mut Ui<'_>) {
        let items = self
            .store
            .list(None)
            .into_iter()
            .map(|connection| {
                modal::ModalItem::new(
                    connection.name.clone(),
                    format!(
                        "{} · {}{}",
                        connection.summary(),
                        if connection.read_only {
                            "read-only"
                        } else {
                            "writable"
                        },
                        if connection.scope == ConnectionScope::Project {
                            " · project"
                        } else {
                            ""
                        }
                    ),
                    connection.id.clone(),
                )
            })
            .collect::<Vec<_>>();
        if items.is_empty() {
            self.open_connection_form(None, ui);
            return;
        }
        *ui.overlay = Some(modal::connection_picker(items));
    }

    pub fn select_connection(&mut self, id: &str, ui: &mut Ui<'_>) {
        let Some(config) = self.store.get(id) else {
            return;
        };
        let name = config.name.clone();
        let read_only = config.read_only;
        if !matches!(ui.workbench.active_tab(), Some(Tab::Sql(_))) {
            self.open_tab(None, ui);
        }
        self.generation = self.generation.wrapping_add(1);
        if let Some(Tab::Sql(tab)) = ui.workbench.active_tab_mut() {
            if tab.connection_id.as_deref() != Some(id) {
                tab.generation = self.generation;
                tab.clear_result();
                tab.row_edits.clear();
                tab.running = false;
            }
            tab.set_connection(Some(id.to_owned()), Some(name.clone()));
            tab.read_only = read_only;
        }
        ui.status(format!(
            "Database: {name} ({})",
            if read_only { "read-only" } else { "writable" }
        ));
        ui.mark_session_dirty();
    }

    pub fn add_connection(&mut self, dsn: &str, ui: &mut Ui<'_>) {
        match parse_postgres_url(dsn) {
            Ok((config, password)) => self.submit_connection_form(
                Submission {
                    config,
                    password,
                    action: ConnectionAction::Save,
                },
                ui,
            ),
            Err(error) => ui.status(format!("Database: {error}")),
        }
    }

    pub fn execute(&mut self, allow_destructive: bool, ui: &mut Ui<'_>) {
        let Some(sql) = ui.workbench.active_tab().and_then(|tab| match tab {
            Tab::Sql(tab) => Some(tab.sql()),
            _ => None,
        }) else {
            ui.status("Database: open a SQL query first");
            return;
        };
        self.execute_text(sql, allow_destructive, ui);
    }

    pub fn execute_selection(&mut self, allow_destructive: bool, ui: &mut Ui<'_>) {
        let Some((sql, selection)) = ui.workbench.active_tab().and_then(|tab| match tab {
            Tab::Sql(tab) => Some((tab.sql(), tab.editor.selected_text())),
            _ => None,
        }) else {
            ui.status("Database: open a SQL query first");
            return;
        };
        let sql = selection
            .filter(|text| !text.trim().is_empty())
            .unwrap_or(sql);
        self.execute_text(sql, allow_destructive, ui);
    }

    pub fn execute_text(&mut self, sql: String, allow_destructive: bool, ui: &mut Ui<'_>) {
        if matches!(ui.workbench.active_tab(),Some(Tab::Sql(sql)) if sql.running) {
            ui.status("Database: a query is already running; use Cancel first");
            return;
        }
        if !self.has_capacity(ui) {
            return;
        }
        if sql.trim().is_empty() {
            ui.status("Database: write a query first");
            return;
        }
        let Some(connection_id) = self.target_connection(ui) else {
            ui.status("Database: choose a connection first");
            self.open_connection_picker(ui);
            return;
        };
        let Some(config) = self.store.get(&connection_id).cloned() else {
            ui.status("Database: the selected connection no longer exists");
            return;
        };
        let risk = classify_sql_risk(&sql);
        if config.read_only && risk.is_mutation {
            ui.status("Database: connection is read-only; edit the connection to allow mutations");
            return;
        }
        if risk.requires_confirmation && !allow_destructive {
            let reasons = risk
                .statements
                .iter()
                .filter_map(|statement| statement.reason.clone())
                .collect::<Vec<_>>()
                .join(" ");
            let preview = sql.chars().take(700).collect::<String>();
            *ui.overlay = Some(modal::confirm(
                ConfirmAction::RunSql(sql),
                "Run Query",
                &format!("{reasons}\n\n{preview}"),
            ));
            return;
        }
        let password = self.password(&config);
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        if let Some(Tab::Sql(tab)) = ui.workbench.active_tab_mut() {
            tab.running = true;
            tab.generation = generation;
            tab.clear_result();
            tab.set_connection(Some(connection_id.clone()), Some(config.name.clone()));
        }
        let sessions = self.sessions.clone();
        let sender = self.events.clone();
        let connection_id_for_task = connection_id.clone();
        let mut stored = config.clone();
        stored.id = connection_id.clone();
        self.spawn(
            connection_id.clone(),
            async move {
                let session =
                    match open_session(&sessions, &stored, password.as_deref(), true).await {
                        Ok(session) => session,
                        Err(error) => {
                            let _ = sender
                                .send(AppEvent::Db(DbEvent::Failed {
                                    generation,
                                    connection_id: connection_id_for_task,
                                    message: error.to_string(),
                                }))
                                .await;
                            return;
                        }
                    };
                match session.execute(&sql, allow_destructive).await {
                    Ok(result) => {
                        let _ = sender
                            .send(AppEvent::Db(DbEvent::Completed {
                                generation,
                                connection_id: connection_id_for_task,
                                result: Box::new(result),
                            }))
                            .await;
                    }
                    Err(error) => {
                        if matches!(error, DbError::Query(_)) {
                            sessions.lock().await.remove(&connection_id_for_task);
                        }
                        let _ = sender
                            .send(AppEvent::Db(DbEvent::Failed {
                                generation,
                                connection_id: connection_id_for_task,
                                message: error.to_string(),
                            }))
                            .await;
                    }
                }
            },
            ui,
        );
    }

    pub fn test_connection(&mut self, ui: &mut Ui<'_>) {
        let Some(connection_id) = self.target_connection(ui) else {
            ui.status("Database: choose a connection first");
            return;
        };
        let Some(config) = self.store.get(&connection_id).cloned() else {
            return;
        };
        let password = self.password(&config);
        let sender = self.events.clone();
        let mut stored = config;
        stored.id = connection_id;
        ui.status(format!("Database: testing {}…", stored.name));
        self.spawn(
            stored.id.clone(),
            async move {
                let result = tide_db::test_connection(&stored, password.as_deref()).await;
                let _ = sender
                    .send(AppEvent::Db(DbEvent::Tested {
                        name: stored.name,
                        success: result.success,
                        error: result.error,
                        latency_ms: result.latency_ms,
                        server_version: result.server_version,
                        warning: result.warning,
                    }))
                    .await;
            },
            ui,
        );
    }

    pub fn disconnect(&mut self, ui: &mut Ui<'_>) {
        let connection = ui.workbench.active_tab().and_then(|tab| match tab {
            Tab::Sql(tab) => tab.connection_id.clone(),
            _ => None,
        });
        let Some(connection) = connection else {
            ui.status("Database: no connection selected");
            return;
        };
        self.cancel_requests(&connection, ui);
        ui.status("Database: connection closed");
    }

    pub fn browse_schema(&mut self, ui: &mut Ui<'_>) {
        let Some(connection_id) = self.target_connection(ui) else {
            ui.status("Database: choose a connection first");
            self.open_connection_picker(ui);
            return;
        };
        let Some(config) = self.store.get(&connection_id).cloned() else {
            return;
        };
        let password = self.password(&config);
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.schema_connection = Some(connection_id.clone());
        let sessions = self.sessions.clone();
        let sender = self.events.clone();
        let mut stored = config;
        stored.id = connection_id.clone();
        ui.status("Database: loading schema…");
        self.spawn(
            connection_id.clone(),
            async move {
                let session =
                    match open_session(&sessions, &stored, password.as_deref(), false).await {
                        Ok(session) => session,
                        Err(error) => {
                            let _ = sender
                                .send(AppEvent::Db(DbEvent::Failed {
                                    generation,
                                    connection_id,
                                    message: error.to_string(),
                                }))
                                .await;
                            return;
                        }
                    };
                match session.catalog().await {
                    Ok(items) => {
                        let _ = sender
                            .send(AppEvent::Db(DbEvent::Catalog {
                                generation,
                                connection_id,
                                items,
                            }))
                            .await;
                    }
                    Err(error) => {
                        let _ = sender
                            .send(AppEvent::Db(DbEvent::Failed {
                                generation,
                                connection_id,
                                message: error.to_string(),
                            }))
                            .await;
                    }
                }
            },
            ui,
        );
    }

    #[cfg(test)]
    pub fn connection_dsn(config: &ConnectionConfig) -> String {
        let mut url = url::Url::parse("postgres://localhost").expect("static URL");
        let _ = url.set_username(&config.username);
        let _ = url.set_host(Some(&config.host));
        let _ = url.set_port(Some(config.port));
        url.set_path(&format!("/{}", config.database));
        url.query_pairs_mut()
            .append_pair("name", &config.name)
            .append_pair("readOnly", if config.read_only { "1" } else { "0" });
        if config.ssl.is_some() {
            url.query_pairs_mut()
                .append_pair("sslmode", config.ssl_mode().as_str());
        }
        url.to_string()
    }

    pub fn edit_connection_prompt(&mut self, ui: &mut Ui<'_>) {
        let Some(connection) = ui.workbench.active_tab().and_then(|tab| match tab {
            Tab::Sql(tab) => tab.connection_id.clone(),
            _ => None,
        }) else {
            ui.status("Database: no connection selected");
            return;
        };
        let Some(config) = self.store.get(&connection) else {
            return;
        };
        self.open_connection_form(Some(config.clone()), ui);
    }

    pub fn edit_connection(&mut self, id: &str, dsn: &str, ui: &mut Ui<'_>) {
        let Some(existing) = self.store.get(id).cloned() else {
            return;
        };
        let (mut config, password) = match parse_postgres_url(dsn) {
            Ok(value) => value,
            Err(error) => {
                ui.status(format!("Database: {error}"));
                return;
            }
        };
        config.id = id.into();
        config.scope = existing.scope;
        config.project_path = existing.project_path.clone();
        config.connection_timeout = existing.connection_timeout;
        config.query_timeout = existing.query_timeout;
        let explicit = url::Url::parse(dsn)
            .ok()
            .is_some_and(|u| u.query_pairs().any(|(k, _)| k == "sslmode"));
        if !explicit || config.ssl_mode() == existing.ssl_mode() {
            config.ssl = existing.ssl;
        }
        self.submit_connection_form(
            Submission {
                config,
                password,
                action: ConnectionAction::Save,
            },
            ui,
        );
    }

    pub fn delete_connection_prompt(&mut self, ui: &mut Ui<'_>) {
        let Some(connection) = ui.workbench.active_tab().and_then(|tab| match tab {
            Tab::Sql(tab) => tab.connection_id.clone(),
            _ => None,
        }) else {
            ui.status("Database: no connection selected");
            return;
        };
        let name = self
            .store
            .get(&connection)
            .map_or_else(|| connection.clone(), |config| config.name.clone());
        *ui.overlay = Some(modal::confirm(
            ConfirmAction::DeleteConnection(connection),
            "Delete Connection",
            &format!(
                "Delete the database connection \"{name}\"? Stored secrets are left untouched."
            ),
        ));
    }

    pub fn delete_connection(&mut self, id: &str, ui: &mut Ui<'_>) {
        match self.store.remove(id) {
            Ok(true) => {
                self.cancel_requests(id, ui);
                for tab in ui.workbench.tabs_mut() {
                    if let Tab::Sql(sql) = tab
                        && sql.connection_id.as_deref() == Some(id)
                    {
                        sql.set_connection(None, None);
                        sql.clear_result();
                    }
                }
                ui.status("Database: connection deleted");
                ui.mark_session_dirty();
            }
            Ok(false) => ui.status("Database: connection not found"),
            Err(error) => ui.status(format!("Database: {error}")),
        }
    }

    pub fn describe_table(&mut self, schema: &str, table: &str, ui: &mut Ui<'_>) {
        let Some(connection_id) = self.schema_connection.clone() else {
            ui.status("Database: choose a connection first");
            return;
        };
        let Some(config) = self.store.get(&connection_id).cloned() else {
            return;
        };
        let password = self.password(&config);
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let sessions = self.sessions.clone();
        let sender = self.events.clone();
        let mut stored = config;
        stored.id = connection_id.clone();
        let schema = schema.to_owned();
        let table = table.to_owned();
        ui.status("Database: loading table definition…");
        self.spawn(
            connection_id.clone(),
            async move {
                let session =
                    match open_session(&sessions, &stored, password.as_deref(), false).await {
                        Ok(session) => session,
                        Err(error) => {
                            let _ = sender
                                .send(AppEvent::Db(DbEvent::Failed {
                                    generation,
                                    connection_id,
                                    message: error.to_string(),
                                }))
                                .await;
                            return;
                        }
                    };
                match session.ddl(&schema, &table).await {
                    Ok(ddl) => {
                        let _ = sender
                            .send(AppEvent::Db(DbEvent::Ddl {
                                generation,
                                title: format!("{schema}.{table}"),
                                ddl,
                            }))
                            .await;
                    }
                    Err(error) => {
                        let _ = sender
                            .send(AppEvent::Db(DbEvent::Failed {
                                generation,
                                connection_id,
                                message: error.to_string(),
                            }))
                            .await;
                    }
                }
            },
            ui,
        );
    }

    pub fn open_history(&mut self, ui: &mut Ui<'_>) {
        let entries = match self
            .history
            .get_history(self.history_connection.as_deref(), 10000, 0)
        {
            Ok(entries) => entries,
            Err(error) => {
                ui.status(error);
                return;
            }
        };
        let items = entries
            .into_iter()
            .filter(|e| !self.history_favorites || e.is_favorite)
            .map(|e| {
                ModalItem::new(
                    format!(
                        "{} {}",
                        if e.is_favorite { "★" } else { " " },
                        e.sql.lines().next().unwrap_or_default()
                    ),
                    format!(
                        "{} · {} rows · {} ms · {}",
                        e.connection_name, e.row_count, e.duration_ms, e.sql
                    ),
                    e.id,
                )
            })
            .collect();
        let mut dialog = modal::query_history(items);
        dialog.base_mut().title = format!(
            "Query History · {} · {}",
            if self.history_favorites {
                "favorites"
            } else {
                "all"
            },
            if self.history_connection.is_some() {
                "connection"
            } else {
                "all connections"
            }
        );
        *ui.overlay = Some(dialog);
    }

    pub fn load_history_entry(&mut self, id: &str, ui: &mut Ui<'_>) {
        let entry = self
            .history
            .entries()
            .ok()
            .and_then(|entries| entries.into_iter().find(|entry| entry.id == id));
        let Some(entry) = entry else {
            return;
        };
        self.open_tab(Some(&entry.sql), ui);
        if let Some(Tab::Sql(tab)) = ui.workbench.active_tab_mut() {
            tab.set_connection(Some(entry.connection_id), Some(entry.connection_name));
        }
        *ui.focus = Focus::Main;
        ui.mark_session_dirty();
    }

    pub fn export_results(&mut self, path: &str, ui: &mut Ui<'_>) {
        let path = crate::app::resolve_path(ui.workspace, path);
        self.export_results_to(path, false, ui);
    }

    pub fn export_results_to(&mut self, path: PathBuf, overwrite: bool, ui: &mut Ui<'_>) {
        let Some((result, name)) = ui.workbench.active_tab().and_then(|tab| match tab {
            Tab::Sql(tab) => tab.result.as_ref().map(|result| {
                (
                    result.clone(),
                    tab.connection_name.clone().unwrap_or_default(),
                )
            }),
            _ => None,
        }) else {
            ui.status("Database: no results to export");
            return;
        };
        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            ui.status(format!(
                "Database: export failed: cannot create {}: {error}",
                parent.display()
            ));
            return;
        }
        let csv = result_to_csv(&result);
        let written = if overwrite {
            tide_core::fs::write_atomic(&path, csv.as_bytes())
        } else {
            tide_core::fs::write_atomic_new(&path, csv.as_bytes())
        };
        match written {
            Ok(()) => ui.status(format!(
                "Database: exported {} rows to {}{}",
                result.row_count,
                path.display(),
                if name.is_empty() {
                    String::new()
                } else {
                    format!(" ({name})")
                }
            )),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                *ui.overlay = Some(modal::confirm(
                    ConfirmAction::ExportResults(path.clone()),
                    "Replace File",
                    &format!("Replace {} with the exported results?", path.display()),
                ));
            }
            Err(error) => ui.status(format!("Database: export failed: {error}")),
        }
    }

    pub fn open_completion(&mut self, ui: &mut Ui<'_>) {
        let Some(sql) = ui.workbench.active_tab().and_then(|tab| match tab {
            Tab::Sql(tab) => Some(tab.sql()),
            _ => None,
        }) else {
            return;
        };
        let tables = self
            .schema_tables
            .iter()
            .map(|table| table.name.clone())
            .collect::<Vec<_>>();
        let suggestions = complete_sql(&sql, &tables, &[]);
        let items = suggestions
            .into_iter()
            .map(|suggestion| {
                modal::ModalItem::new(
                    suggestion.label,
                    suggestion.detail.unwrap_or(suggestion.kind),
                    String::new(),
                )
            })
            .collect::<Vec<_>>();
        if items.is_empty() {
            ui.status("Database: no completions");
        } else {
            *ui.overlay = Some(modal::completion(items));
        }
    }

    pub fn action(&mut self, action: crate::sql_tab::SqlAction, ui: &mut Ui<'_>) {
        use crate::sql_tab::SqlAction;
        match action {
            SqlAction::Editor => {
                if let Some(Tab::Sql(sql)) = ui.workbench.active_tab_mut() {
                    sql.results_focused = false;
                    sql.details_mode = false;
                }
            }
            SqlAction::Back => {
                if let Some(Tab::Sql(sql)) = ui.workbench.active_tab_mut() {
                    sql.details_mode = false;
                    sql.row_edits.clear();
                }
            }
            SqlAction::View => {
                ui.effects
                    .push(Effect::Command(Command::QueryResultsToggleViewMode));
            }
            SqlAction::Details => {
                ui.effects
                    .push(Effect::Command(Command::QueryResultsShowDetails));
            }
            SqlAction::Export => {
                ui.effects
                    .push(Effect::Command(Command::DatabaseExportResults));
            }
            SqlAction::Refresh => {
                ui.effects
                    .push(Effect::Command(Command::QueryResultsRefresh));
            }
            SqlAction::Cancel => {
                if let Some(connection) = self.active_connection(ui) {
                    self.cancel_requests(&connection, ui);
                    ui.status("Database: operation canceled");
                }
            }
            SqlAction::Schema => {
                ui.effects
                    .push(Effect::Command(Command::DatabaseBrowseSchema));
            }
            SqlAction::Add => self.open_add_row(ui),
            SqlAction::Edit => self.edit_row_cell(ui),
            SqlAction::Save => self.save_row(false, ui),
            SqlAction::Delete => self.save_row(true, ui),
        }
    }

    pub fn open_add_row(&mut self, ui: &mut Ui<'_>) {
        if !self.has_capacity(ui) {
            return;
        }
        let Some(Tab::Sql(sql)) = ui.workbench.active_tab() else {
            return;
        };
        if sql.read_only || sql.running {
            ui.status("Database: Add Row needs an idle writable connection");
            return;
        }
        let Some(result) = &sql.result else {
            return;
        };
        let Some(target) = result.row_source.clone() else {
            ui.status("Database: Add Row needs results from one unambiguous base table");
            return;
        };
        let (connection_id, query_id) = (result.connection_id.clone(), result.query_id.clone());
        let Some(config) = self.store.get(&connection_id).cloned() else {
            return;
        };
        let password = self.password(&config);
        let sessions = self.sessions.clone();
        let sender = self.events.clone();
        ui.status("Database: loading Add Row fields…");
        self.spawn(
            connection_id.clone(),
            async move {
                let result = match open_session(&sessions, &config, password.as_deref(), false)
                    .await
                    .map_err(|e| e.to_string())
                {
                    Ok(session) => session
                        .insert_target(&target)
                        .await
                        .map_err(|e| e.to_string()),
                    Err(e) => Err(e),
                };
                let _ = sender
                    .send(AppEvent::Db(DbEvent::InsertPrepared {
                        connection_id,
                        query_id,
                        result,
                    }))
                    .await;
            },
            ui,
        );
    }

    pub fn execute_insert(&mut self, request: tide_db::RowInsert, ui: &mut Ui<'_>) {
        let source = ui.workbench.active_tab().and_then(|tab| match tab {
            Tab::Sql(sql) if !sql.read_only && !sql.running => sql
                .result
                .as_ref()
                .filter(|r| {
                    r.query_id == request.query_id && r.connection_id == request.connection_id
                })
                .map(|r| r.sql.clone()),
            _ => None,
        });
        let Some(query) = source else {
            *ui.overlay = Some(Box::new(crate::row_insert_form::RowInsertForm::restore(
                request,
                "Results changed or connection is read-only; reopen Add Row".into(),
            )));
            return;
        };
        if !self.has_capacity(ui) {
            *ui.overlay = Some(Box::new(crate::row_insert_form::RowInsertForm::restore(
                request,
                "Database busy; retry when idle".into(),
            )));
            return;
        }
        let Some(config) = self.store.get(&request.connection_id).cloned() else {
            *ui.overlay = None;
            ui.status("Database: connection no longer exists");
            return;
        };
        let password = self.password(&config);
        let sessions = self.sessions.clone();
        let sender = self.events.clone();
        if let Some(Tab::Sql(sql)) = ui.workbench.active_tab_mut() {
            sql.running = true;
        }
        self.spawn(
            config.id.clone(),
            async move {
                let result = match open_session(&sessions, &config, password.as_deref(), false)
                    .await
                    .map_err(|e| e.to_string())
                {
                    Ok(session) => match session.insert_row(&request).await {
                        Ok(row) => Ok((
                            row,
                            session
                                .execute(&query, false)
                                .await
                                .map_err(|e| e.to_string()),
                        )),
                        Err(e) => Err(e.to_string()),
                    },
                    Err(e) => Err(e),
                };
                let _ = sender
                    .send(AppEvent::Db(DbEvent::RowInserted {
                        request,
                        result: Box::new(result),
                    }))
                    .await;
            },
            ui,
        );
    }

    pub fn edit_row_cell(&mut self, ui: &mut Ui<'_>) {
        if matches!(ui.workbench.active_tab(),Some(Tab::Sql(sql)) if sql.running||sql.read_only) {
            ui.status("Database: row editing requires an idle writable connection");
            return;
        }
        let Some(Tab::Sql(sql)) = ui.workbench.active_tab() else {
            return;
        };
        let Some(result) = &sql.result else {
            return;
        };
        if result.row_target.is_none() {
            ui.status(
                "Database: row editing needs an unambiguous table and every primary-key column",
            );
            return;
        }
        let Some(field) = result.fields.get(sql.selected_column) else {
            return;
        };
        let Some(value) = sql
            .row_edits
            .get(&field.name)
            .or_else(|| sql.selected_record().and_then(|row| row.get(&field.name)))
        else {
            return;
        };
        *ui.overlay = Some(modal::prompt(
            PromptAction::EditRowCell(result.query_id.clone(), field.name.clone()),
            &format!("Edit {} · JSON value; null clears", field.name),
            &serde_json::to_string(value).unwrap_or_default(),
        ));
    }

    pub fn save_row(&mut self, delete: bool, ui: &mut Ui<'_>) {
        if matches!(ui.workbench.active_tab(),Some(Tab::Sql(sql)) if sql.running) {
            ui.status("Database: a row operation is already running");
            return;
        }
        let Some(Tab::Sql(sql)) = ui.workbench.active_tab() else {
            return;
        };
        let Some(result) = &sql.result else {
            return;
        };
        let Some(target) = &result.row_target else {
            ui.status(
                "Database: row editing needs an unambiguous table and every primary-key column",
            );
            return;
        };
        let Some(original) = sql.selected_record().cloned() else {
            return;
        };
        let request = tide_db::RowMutation {
            connection_id: result.connection_id.clone(),
            query_id: result.query_id.clone(),
            target: target.clone(),
            original,
            changes: sql.row_edits.clone(),
            delete,
        };
        if delete {
            *ui.overlay = Some(modal::confirm(
                ConfirmAction::DeleteRow(Box::new(request.clone())),
                "Delete Row",
                &format!(
                    "Delete {}.{} row ({})? Changed or missing rows are refused.",
                    request.target.schema,
                    request.target.table,
                    request
                        .target
                        .primary_key
                        .iter()
                        .map(|key| format!(
                            "{key}={}",
                            truncate_to_width(
                                &request
                                    .original
                                    .get(key)
                                    .map_or(String::new(), ToString::to_string),
                                120
                            )
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        } else {
            self.execute_row(request, ui);
        }
    }

    pub fn execute_row(&mut self, request: tide_db::RowMutation, ui: &mut Ui<'_>) {
        if !self.has_capacity(ui) {
            return;
        }
        let Some(config) = self.store.get(&request.connection_id).cloned() else {
            ui.status("Database: connection no longer exists");
            return;
        };
        if config.read_only {
            ui.status("Database: this connection is read-only");
            return;
        }
        let password = self.password(&config);
        let sessions = self.sessions.clone();
        let sender = self.events.clone();
        ui.status(if request.delete {
            "Database: deleting row…"
        } else {
            "Database: saving row…"
        });
        for tab in ui.workbench.tabs_mut() {
            if let Tab::Sql(sql) = tab
                && sql
                    .result
                    .as_ref()
                    .is_some_and(|r| r.query_id == request.query_id)
            {
                sql.running = true;
            }
        }
        self.spawn(
            config.id.clone(),
            async move {
                let session = open_session(&sessions, &config, password.as_deref(), false).await;
                let result = match session {
                    Ok(session) => session
                        .mutate_row(&request)
                        .await
                        .map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                if result.is_err() {
                    sessions.lock().await.remove(&config.id);
                }
                let _ = sender
                    .send(AppEvent::Db(DbEvent::RowSaved { request, result }))
                    .await;
            },
            ui,
        );
    }

    pub fn handle_key(&mut self, key: KeyEvent, ui: &mut Ui<'_>) {
        if matches!(ui.workbench.active_tab(),Some(Tab::Sql(sql)) if sql.results_focused) {
            let details =
                matches!(ui.workbench.active_tab(),Some(Tab::Sql(sql)) if sql.details_mode);
            let action = match key.code {
                KeyCode::Esc => Some(if details {
                    crate::sql_tab::SqlAction::Back
                } else {
                    crate::sql_tab::SqlAction::Editor
                }),
                KeyCode::Enter if details && key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(crate::sql_tab::SqlAction::Save)
                }
                KeyCode::Enter => Some(if details {
                    crate::sql_tab::SqlAction::Edit
                } else {
                    crate::sql_tab::SqlAction::Details
                }),
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) && details => {
                    Some(crate::sql_tab::SqlAction::Save)
                }
                KeyCode::Char('e') if details => Some(crate::sql_tab::SqlAction::Edit),
                KeyCode::Delete if details => Some(crate::sql_tab::SqlAction::Delete),
                KeyCode::Char('v') | KeyCode::Tab => Some(crate::sql_tab::SqlAction::View),
                _ => None,
            };
            if let Some(action) = action {
                self.action(action, ui);
                return;
            }
            if let Some(Tab::Sql(sql)) = ui.workbench.active_tab_mut() {
                match key.code {
                    KeyCode::Up => sql.move_selection(false),
                    KeyCode::Down => sql.move_selection(true),
                    KeyCode::Left => sql.move_column(false),
                    KeyCode::Right => sql.move_column(true),
                    KeyCode::PageUp => sql.scroll_results(-10),
                    KeyCode::PageDown => sql.scroll_results(10),
                    _ => {}
                }
            }
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Up => {
                    if let Some(Tab::Sql(tab)) = ui.workbench.active_tab_mut() {
                        tab.move_selection(false);
                    }
                    return;
                }
                KeyCode::Down => {
                    if let Some(Tab::Sql(tab)) = ui.workbench.active_tab_mut() {
                        tab.move_selection(true);
                    }
                    return;
                }
                KeyCode::PageUp => {
                    if let Some(Tab::Sql(tab)) = ui.workbench.active_tab_mut() {
                        tab.scroll_results(-10);
                    }
                    return;
                }
                KeyCode::PageDown => {
                    if let Some(Tab::Sql(tab)) = ui.workbench.active_tab_mut() {
                        tab.scroll_results(10);
                    }
                    return;
                }
                _ => {}
            }
        }
        let tab_size = super::tab_size(ui.settings);
        let spaces = ui.setting_bool("editor.insertSpaces", true);
        let indent = ui
            .settings
            .get("editor.autoIndent")
            .and_then(|v| v.as_str())
            .unwrap_or("full")
            .to_owned();
        let Some(Tab::Sql(tab)) = ui.workbench.active_tab_mut() else {
            return;
        };
        tab.editor.configure_editing(tab_size, spaces, false);
        tab.editor.auto_indent = indent;
        tab.editor.follow_cursor();
        match key.code {
            KeyCode::Char(ch)
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                tab.editor.insert_char(ch);
                ui.mark_session_dirty();
            }
            KeyCode::Enter => {
                tab.editor.newline();
                ui.mark_session_dirty();
            }
            KeyCode::Tab => {
                let text = if spaces {
                    " ".repeat(tab_size)
                } else {
                    "\t".into()
                };
                tab.editor.insert_text(&text);
                ui.mark_session_dirty();
            }
            KeyCode::Backspace => {
                tab.editor.backspace();
                ui.mark_session_dirty();
            }
            KeyCode::Delete => {
                tab.editor.delete_forward();
                ui.mark_session_dirty();
            }
            KeyCode::Left => tab
                .editor
                .move_left(key.modifiers.contains(KeyModifiers::SHIFT)),
            KeyCode::Right => tab
                .editor
                .move_right(key.modifiers.contains(KeyModifiers::SHIFT)),
            KeyCode::Up => tab
                .editor
                .move_up(key.modifiers.contains(KeyModifiers::SHIFT)),
            KeyCode::Down => tab
                .editor
                .move_down(key.modifiers.contains(KeyModifiers::SHIFT)),
            KeyCode::Home => tab.editor.home(key.modifiers.contains(KeyModifiers::SHIFT)),
            KeyCode::End => tab.editor.end(key.modifiers.contains(KeyModifiers::SHIFT)),
            KeyCode::PageUp => tab
                .editor
                .page_up(20, key.modifiers.contains(KeyModifiers::SHIFT)),
            KeyCode::PageDown => tab
                .editor
                .page_down(20, key.modifiers.contains(KeyModifiers::SHIFT)),
            KeyCode::Esc => tab.editor.clear_selection(),
            _ => {}
        }
    }

    pub fn refresh_results(&mut self, ui: &mut Ui<'_>) {
        if let Some(sql) = ui.workbench.active_tab().and_then(|tab| match tab {
            Tab::Sql(tab) => tab.result.as_ref().map(|result| result.sql.clone()),
            _ => None,
        }) {
            self.execute_text(sql, false, ui);
        }
    }
}

/// The cached session for `config`, or a new one. With `verify`, a cached session that fails
/// a health check is replaced. The lock is not held while connecting.
async fn open_session(
    sessions: &Sessions,
    config: &ConnectionConfig,
    password: Option<&str>,
    verify: bool,
) -> Result<Arc<PostgresSession>, DbError> {
    let cached = sessions
        .lock()
        .await
        .get(&config.id)
        .filter(|session| session.matches_config(config))
        .cloned();
    if let Some(session) = cached {
        if !verify || session.is_healthy().await {
            return Ok(session);
        }
        sessions.lock().await.remove(&config.id);
    }
    let session = Arc::new(PostgresSession::connect(config, password).await?);
    sessions
        .lock()
        .await
        .insert(config.id.clone(), session.clone());
    Ok(session)
}
