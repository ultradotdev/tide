//! Settings, keybinding and theme changes.
use super::*;
use tide_core::num::saturating_u16_from_u64;

impl App {
    pub(super) fn setting_bool(&self, key: &str, default: bool) -> bool {
        self.settings
            .get(key)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(default)
    }

    pub(super) fn setting_u64(&self, key: &str, default: u64) -> u64 {
        self.settings
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(default)
    }

    /// A size setting in terminal cells. Out-of-range values clamp to the
    /// widest cell count instead of wrapping.
    pub(super) fn setting_cells(&self, key: &str, default: u16) -> u16 {
        saturating_u16_from_u64(self.setting_u64(key, u64::from(default)))
    }

    pub(super) fn apply_preferences(&mut self) {
        self.sidebar_width = self.setting_cells("tui.sidebar.width", 36);
        self.sidebar_visible = self.setting_bool("tui.sidebar.visible", true);
        self.sidebar_left = self
            .settings
            .get("tui.sidebar.location")
            .and_then(serde_json::Value::as_str)
            != Some("right");
        self.ai.agents_visible = self.setting_bool("tui.agentsSidebar.visible", true);
        self.ai.agents_width = self.setting_cells("tui.agentsSidebar.width", 36);
        self.dock.height = self.setting_cells("tui.terminal.height", 10);
        self.db
            .history
            .set_enabled(self.setting_bool("session.databaseQueryHistory", true));
    }

    pub(super) fn update_setting(&mut self, key: &str, input: &str) {
        let value = serde_json::from_str::<serde_json::Value>(input.trim())
            .unwrap_or_else(|_| serde_json::Value::String(input.trim().to_owned()));
        if let Err(error) = crate::preferences::validate(key, &value) {
            self.set_status(error);
            return;
        }
        if key == "workbench.colorTheme" && self.themes.get(value.as_str().unwrap()).is_none() {
            self.set_status("Unknown theme");
            return;
        }
        let mut next = self.settings.clone();
        next.set(key, value.clone());
        let settings_file = self.paths.settings_file.clone();
        match edit_config_file(&settings_file, |text| {
            tide_config::set_jsonc_member(text, key, &value)
        }) {
            Ok(()) => {
                self.settings = next;
                if key == "workbench.colorTheme" {
                    self.select_theme(value.as_str().unwrap());
                } else if key.starts_with("tui.sidebar.")
                    || key.starts_with("tui.agentsSidebar.")
                    || key == "tui.terminal.height"
                    || key == "session.databaseQueryHistory"
                {
                    match key {
                        "tui.sidebar.width" => {
                            self.sidebar_width = saturating_u16_from_u64(value.as_u64().unwrap())
                        }
                        "tui.sidebar.visible" => self.sidebar_visible = value.as_bool().unwrap(),
                        "tui.sidebar.location" => {
                            self.sidebar_left = value.as_str() == Some("left")
                        }
                        "tui.agentsSidebar.visible" => {
                            self.ai.agents_visible = value.as_bool().unwrap()
                        }
                        "tui.agentsSidebar.width" => {
                            self.ai.agents_width = saturating_u16_from_u64(value.as_u64().unwrap())
                        }
                        "tui.terminal.height" => {
                            self.dock.height = saturating_u16_from_u64(value.as_u64().unwrap())
                        }
                        "session.databaseQueryHistory" => {
                            self.db.history.set_enabled(value.as_bool().unwrap())
                        }
                        _ => {}
                    }
                }
                if matches!(key, "tui.timeline.mode" | "tui.timeline.commitCount") {
                    self.git.timeline.file = self
                        .settings
                        .get("tui.timeline.mode")
                        .and_then(serde_json::Value::as_str)
                        == Some("file");
                    with_ui!(self, |ui| self.git.refresh_timeline(ui));
                }
                if matches!(
                    key,
                    "tui.diffViewer.showDiagnostics" | "tui.diffViewer.autoRefresh"
                ) {
                    with_ui!(self, |ui| crate::controllers::git::sync_diff_diagnostics(
                        ui,
                        &self.lsp.diagnostics
                    ));
                    with_ui!(self, |ui| self.git.refresh_diff_views(ui));
                }
                if key == "tui.fileTree.showGitStatus" {
                    if value == serde_json::Value::Bool(false) {
                        self.file_tree.set_decorations(Vec::new());
                    } else {
                        with_ui!(self, |ui| self.git.refresh(ui));
                    }
                }
                self.set_status(format!(
                    "Setting {key} updated{}",
                    if matches!(key, "tui.terminal.scrollback" | "terminal.shell") {
                        "; applies to new terminals"
                    } else {
                        ""
                    }
                ));
            }
            Err(error) => self.set_status(format!("Setting not saved: {error}")),
        }
    }

    /// Moves `selected`, one of the active bindings, to `new_key`.
    pub(super) fn update_keybinding(&mut self, selected: &Keybinding, new_key: &str) {
        let new_key = new_key.trim();
        if new_key.is_empty() {
            return;
        }
        let canonical = canonical_key(&selected.key);
        let Some(binding) = self.keybindings.iter().find(|binding| {
            canonical_key(&binding.key) == canonical
                && binding.command == selected.command
                && binding.when == selected.when
        }) else {
            return;
        };
        let removed = Keybinding {
            key: selected.key.clone(),
            command: String::new(),
            when: binding.when.clone(),
            args: None,
        };
        let replacement = Keybinding {
            key: new_key.to_owned(),
            ..binding.clone()
        };
        let added = [&removed, &replacement]
            .map(|binding| serde_json::to_value(binding).unwrap_or_default());
        if let Err(error) = edit_config_file(&self.paths.keybindings_file, |text| {
            tide_config::append_jsonc_elements(text, &added)
        }) {
            self.set_status(format!("Keybinding not saved: {error}"));
            return;
        }
        self.keybindings.retain(|binding| {
            !((canonical_key(&binding.key) == canonical
                || canonical_key(&binding.key) == canonical_key(new_key))
                && binding.when == replacement.when)
        });
        self.user_keybindings.push(removed.clone());
        self.user_keybindings.push(replacement.clone());
        self.keybindings.push(removed);
        self.keybindings.push(replacement);
        self.rebuild_keymap();
        self.set_status(format!("Keybinding updated to {new_key}"));
    }

    pub(super) fn cycle_theme(&mut self, delta: isize, toggle: bool) {
        let current = self.themes.selected_id().to_owned();
        let kind = self.themes.selected().theme_type.clone();
        let ids = self
            .themes
            .iter()
            .filter(|theme| !toggle || theme.theme_type != kind)
            .map(|theme| theme.id.clone())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return;
        }
        let index = if toggle {
            0
        } else {
            ids.iter()
                .position(|id| *id == current)
                .unwrap_or(0)
                .checked_add_signed(delta)
                .unwrap_or(ids.len() - 1)
                % ids.len()
        };
        self.select_theme(&ids[index]);
    }

    pub(super) fn select_theme(&mut self, id: &str) {
        let Some(theme) = self.themes.get(id).cloned() else {
            self.set_status(format!("Unknown theme: {id}"));
            return;
        };
        let mut next = self.settings.clone();
        next.set("workbench.colorTheme", serde_json::json!(id));
        if let Err(error) = edit_config_file(&self.paths.settings_file, |text| {
            tide_config::set_jsonc_member(text, "workbench.colorTheme", &serde_json::json!(id))
        }) {
            self.set_status(format!("Theme not saved: {error}"));
            return;
        }
        self.themes.select(id);
        self.settings = next;
        self.palette = Palette::from_theme(&theme);
        self.syntax.set_theme(&theme);
        self.set_status(format!("Theme: {}", theme.name));
    }

    pub(super) fn show_line_numbers(&self) -> bool {
        self.settings
            .get("editor.lineNumbers")
            .and_then(serde_json::Value::as_str)
            != Some("off")
    }

    pub(super) fn tab_size(&self) -> usize {
        crate::controllers::tab_size(&self.settings)
    }
}

/// Applies `edit` to the user's JSONC file at `path`, which may not exist yet. Only the
/// edited value changes: the user's comments, ordering and other values are kept, and a
/// file that does not parse is left as it is.
fn edit_config_file(
    path: &Path,
    edit: impl FnOnce(&str) -> Result<String, tide_config::EditError>,
) -> Result<(), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("could not read {}: {error}", path.display())),
    };
    let edited = edit(&text).map_err(|error| {
        format!(
            "{} has an error, so it was left unchanged: {error}",
            path.display()
        )
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    write_file_atomic(path, &edited).map_err(|error| error.to_string())
}
