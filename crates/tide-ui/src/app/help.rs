//! Help pages rendered from the live workbench, and the Help commands.
use super::*;
use crate::help::{Context, Host, Live, Page};

impl App {
    /// `page` rendered from the current keybindings, settings and environment.
    pub(crate) fn render_help(&self, page: &Page) -> String {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let context = Context {
            keybindings: &self.keybindings,
            user_keybindings: &self.user_keybindings,
            commands: &self.commands,
            settings: &self.settings,
            themes: &self.themes,
            host: Host::Live(Live {
                paths: &self.paths,
                home: home.as_deref(),
                install: &self.install,
                agents: &self.ai.cli_status,
            }),
        };
        let rendered = crate::help::render(page, &context);
        for error in &rendered.errors {
            tracing::warn!(page = page.id, %error, "help placeholder did not resolve");
        }
        rendered.text
    }

    pub(super) fn help_editor(&self, page: &'static Page) -> EditorState {
        EditorState::help(page, &self.render_help(page))
    }

    /// Opens `page`, or re-renders and focuses it when it is already open.
    pub(crate) fn open_help(&mut self, page: &'static Page) {
        let mut fresh = self.help_editor(page);
        let open = self.workbench.pane_ids().into_iter().find_map(|id| {
            let pane = self.workbench.pane(id)?;
            let index = pane.tabs.iter().position(|tab| {
                tab.editor()
                    .and_then(|editor| editor.help)
                    .is_some_and(|open| open.id == page.id)
            })?;
            Some((id, index))
        });
        self.focus = Focus::Main;
        self.mark_session_dirty();
        let Some((id, index)) = open else {
            self.workbench.insert_tab_focused(Tab::Editor(fresh));
            return;
        };
        self.workbench.set_focused(id);
        if let Some(pane) = self.workbench.pane_mut(id) {
            pane.active = index;
            if let Some(editor) = pane.tabs.get_mut(index).and_then(Tab::editor_mut) {
                fresh.scroll_top = editor.scroll_top.min(fresh.document.len_lines());
                *editor = fresh;
            }
        }
    }

    pub(super) fn open_url(&mut self, url: &str) {
        match (self.url_opener)(url) {
            Ok(()) => self.set_status(format!("Opened {url} in your browser")),
            Err(error) => {
                self.set_status(format!("Could not open a browser ({error}); visit {url}"))
            }
        }
    }

    /// Checks GitHub for a newer release: Help: Check for Updates, and the daily check.
    fn fetch_latest_release(&mut self, automatic: bool) {
        let fetch = self.latest_release;
        let sender = self.events_tx.clone();
        self.jobs.spawn(async move {
            let result = fetch(crate::help::latest_release_url()).await;
            let _ = sender
                .send(AppEvent::LatestRelease(result, automatic))
                .await;
        });
    }

    pub(super) fn check_for_updates(&mut self) {
        self.set_status("Checking for updates…");
        self.fetch_latest_release(false);
    }

    /// The startup check: at most once a day, unless `update.checkAutomatically` is off.
    /// The attempt is recorded before the request, so an offline start does not retry.
    pub fn check_for_updates_automatically(&mut self) {
        if !self.setting_bool("update.checkAutomatically", true)
            || !crate::help::update_check_due(&self.paths, std::time::SystemTime::now())
        {
            return;
        }
        crate::help::record_update_check(&self.paths, std::time::SystemTime::now());
        self.fetch_latest_release(true);
    }

    pub(super) fn show_latest_release(&mut self, result: Result<String, String>, automatic: bool) {
        let current = env!("CARGO_PKG_VERSION");
        match result {
            Err(error) if automatic => tracing::debug!(%error, "automatic update check failed"),
            Err(error) => self.set_status(format!("Update check failed: {error}")),
            Ok(latest) if automatic => {
                if crate::help::is_newer(&latest, current) {
                    self.set_status(format!(
                        "Tide {} is available; run “Help: Check for Updates” for the update step",
                        latest.trim_start_matches('v')
                    ));
                }
            }
            Ok(latest) if crate::help::is_newer(&latest, current) => {
                let latest = latest.trim_start_matches('v');
                let message = format!(
                    "Tide {latest} is available; this is {current}.\n\n{}",
                    crate::help::update_instruction(&self.install)
                );
                self.set_status(format!("Tide {latest} is available"));
                if self.overlay.is_none() {
                    self.overlay = Some(modal::info("Update Available", &message));
                }
            }
            Ok(latest) => self.set_status(format!(
                "Tide {current} is up to date (latest release: {})",
                latest.trim_start_matches('v')
            )),
        }
    }

    pub(super) fn run_help(&mut self, command: Command) {
        match command {
            Command::HelpCheckForUpdates => self.check_for_updates(),
            Command::HelpDocumentation => self.open_url(&crate::help::documentation_url()),
            Command::HelpReportIssue => self.open_url(&crate::help::new_issue_url(&self.install)),
            _ => {
                if let Some(page) = crate::help::for_command(command) {
                    self.open_help(page);
                }
            }
        }
    }
}
