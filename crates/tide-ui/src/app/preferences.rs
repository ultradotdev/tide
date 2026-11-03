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

    /// Applies the current value of `key` to the running workbench, for a setting that
    /// is not simply read where it is used.
    fn apply_setting(&mut self, key: &str) -> SettingEffect {
        let value = self.settings.get(key).cloned().unwrap_or_default();
        match key {
            "workbench.colorTheme" => {
                let id = value.as_str().unwrap_or_default();
                let Some(theme) = self.themes.get(id).cloned() else {
                    return SettingEffect::Rejected(format!("there is no theme named “{id}”"));
                };
                self.themes.select(id);
                self.palette = Palette::from_theme(&theme);
                self.syntax.set_theme(&theme);
            }
            "tui.sidebar.width" => self.sidebar_width = self.setting_cells(key, 36),
            "tui.sidebar.visible" => self.sidebar_visible = self.setting_bool(key, true),
            "tui.sidebar.location" => self.sidebar_left = value.as_str() != Some("right"),
            "tui.agentsSidebar.visible" => self.ai.agents_visible = self.setting_bool(key, true),
            "tui.agentsSidebar.width" => self.ai.agents_width = self.setting_cells(key, 36),
            "tui.terminal.height" => self.dock.height = self.setting_cells(key, 10),
            "session.databaseQueryHistory" => {
                self.db.history.set_enabled(self.setting_bool(key, true));
            }
            "tui.timeline.mode" | "tui.timeline.commitCount" => {
                self.git.timeline.file = self
                    .settings
                    .get("tui.timeline.mode")
                    .and_then(serde_json::Value::as_str)
                    == Some("file");
                with_ui!(self, |ui| self.git.refresh_timeline(ui));
            }
            "tui.diffViewer.showDiagnostics" | "tui.diffViewer.autoRefresh" => {
                with_ui!(self, |ui| crate::controllers::git::sync_diff_diagnostics(
                    ui,
                    &self.lsp.diagnostics
                ));
                with_ui!(self, |ui| self.git.refresh_diff_views(ui));
            }
            "tui.fileTree.showGitStatus" => {
                if value == serde_json::Value::Bool(false) {
                    self.file_tree.set_decorations(Vec::new());
                } else {
                    with_ui!(self, |ui| self.git.refresh(ui));
                }
            }
            "ai.cli.workspaceAliases" => self.with_ai(|ai, ui| ai.rebuild_sessions(ui)),
            "tui.terminal.scrollback" | "terminal.shell" => return SettingEffect::NewTerminals,
            _ if NEXT_START.contains(&key) => return SettingEffect::NextStart,
            _ => {}
        }
        SettingEffect::Now
    }

    /// Shows problems with the user's configuration files in one dialog, or in the
    /// status bar while another dialog is open.
    pub(super) fn show_config_problems(&mut self, problems: &[String]) {
        let message = format!(
            "{}\n\nFix the file and save it: Tide applies it as soon as it is valid. Until then Tide keeps the settings it already had. Changes made from the Settings and Keyboard Shortcuts lists leave a file with errors unchanged.",
            problems.join("\n")
        );
        if self.overlay.is_none()
            || self
                .overlay
                .as_ref()
                .is_some_and(|overlay| overlay.base().title == "Configuration Problems")
        {
            self.overlay = Some(modal::info("Configuration Problems", &message));
        }
        self.set_status(format!(
            "Configuration problem: {}",
            problems.first().map_or("", String::as_str)
        ));
    }

    /// Re-reads settings.jsonc and keybindings.jsonc when either changed on disk since
    /// Tide last read or wrote it.
    pub(super) fn reload_config_if_changed(&mut self) -> bool {
        let stamps = config_stamps(&self.paths);
        if stamps == self.config_stamps {
            return false;
        }
        self.config_stamps = stamps;
        self.reload_config()
    }

    /// Applies the settings and keybindings files as they are now. A file that does not
    /// parse is reported and the configuration Tide already had is kept.
    pub(super) fn reload_config(&mut self) -> bool {
        let mut problems = Vec::new();
        let mut applied = Vec::new();
        let mut new_terminals = Vec::new();
        let mut next_start = Vec::new();
        if let Some(settings) = load_settings(&self.paths, &mut problems, "settings it had") {
            let mut keys = settings
                .iter()
                .map(|(key, _)| key.clone())
                .chain(self.settings.iter().map(|(key, _)| key.clone()))
                .filter(|key| settings.get(key) != self.settings.get(key))
                .collect::<Vec<_>>();
            keys.sort();
            keys.dedup();
            let previous = std::mem::replace(&mut self.settings, settings);
            for key in keys {
                match self.apply_setting(&key) {
                    SettingEffect::Now => applied.push(key),
                    SettingEffect::NewTerminals => new_terminals.push(key),
                    SettingEffect::NextStart => next_start.push(key),
                    SettingEffect::Rejected(problem) => {
                        problems.push(format!(
                            "{}: “{key}”: {problem}, so Tide kept its previous value.",
                            self.paths.settings_file.display()
                        ));
                        if let Some(value) = previous.get(&key) {
                            self.settings.set(key, value.clone());
                        }
                    }
                }
            }
        }
        let mut keys_changed = false;
        if let Some(user) = load_user_keybindings(&self.paths, &mut problems, "keys it had")
            && user != self.user_keybindings
        {
            self.keybindings = merged_keybindings(&user);
            self.user_keybindings = user;
            self.install_alternate_bindings();
            self.rebuild_keymap();
            keys_changed = true;
        }
        if !problems.is_empty() {
            self.show_config_problems(&problems);
            return true;
        }
        if self
            .overlay
            .as_ref()
            .is_some_and(|overlay| overlay.base().title == "Configuration Problems")
        {
            self.overlay = None;
        }
        let mut parts = Vec::new();
        if !applied.is_empty() || !new_terminals.is_empty() || !next_start.is_empty() {
            parts.push("Settings reloaded".to_owned());
        }
        if keys_changed {
            parts.push("Keybindings reloaded".to_owned());
        }
        if parts.is_empty() {
            return false;
        }
        let mut message = parts.join("; ");
        if !new_terminals.is_empty() {
            message.push_str(&format!(
                "; {} applies to new terminals",
                new_terminals.join(", ")
            ));
        }
        if !next_start.is_empty() {
            message.push_str(&format!(
                "; {} takes effect after restart",
                next_start.join(", ")
            ));
        }
        self.set_status(message);
        true
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
                self.config_stamps = config_stamps(&self.paths);
                self.settings = next;
                let effect = self.apply_setting(key);
                self.set_status(format!(
                    "Setting {key} updated{}",
                    match effect {
                        SettingEffect::NewTerminals => "; applies to new terminals",
                        SettingEffect::NextStart => "; takes effect after restart",
                        SettingEffect::Now | SettingEffect::Rejected(_) => "",
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
        self.config_stamps = config_stamps(&self.paths);
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
        self.config_stamps = config_stamps(&self.paths);
        self.themes.select(id);
        self.settings = next;
        self.palette = Palette::from_theme(&theme);
        self.syntax.set_theme(&theme);
        self.set_status(format!("Theme: {}", theme.name));
    }

    /// The enabled `files.exclude` globs.
    pub(super) fn file_excludes(&self) -> Vec<String> {
        self.settings
            .get("files.exclude")
            .and_then(serde_json::Value::as_object)
            .map(|patterns| {
                patterns
                    .iter()
                    .filter(|(_, enabled)| enabled.as_bool() == Some(true))
                    .map(|(pattern, _)| pattern.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether icons use Nerd Font glyphs: `tui.iconStyle`, where `auto` uses them only
    /// in terminals that bundle the glyphs.
    pub(crate) fn nerd_icons(&self) -> bool {
        match self
            .settings
            .get("tui.iconStyle")
            .and_then(serde_json::Value::as_str)
        {
            Some("nerd") => true,
            Some("ascii") => false,
            _ => self.terminal.nerd_font,
        }
    }

    /// The colours sent to the terminal: `tui.colorMode`, where `auto` uses 24-bit
    /// colour only in terminals known to show it.
    pub(crate) fn color_depth(&self) -> crate::display::ColorDepth {
        use crate::display::ColorDepth;
        match self
            .settings
            .get("tui.colorMode")
            .and_then(serde_json::Value::as_str)
        {
            Some("truecolor") => ColorDepth::TrueColor,
            Some("256") => ColorDepth::Indexed,
            _ if self.terminal.truecolor => ColorDepth::TrueColor,
            _ => ColorDepth::Indexed,
        }
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

/// How a changed setting reaches the running workbench.
enum SettingEffect {
    Now,
    NewTerminals,
    /// Read only while Tide starts.
    NextStart,
    Rejected(String),
}

/// Settings read only while Tide starts.
const NEXT_START: &[&str] = &[
    "session.restoreOnStartup",
    "workbench.startupEditor",
    "update.checkAutomatically",
    "tui.outline.collapsedOnStartup",
    "tui.timeline.collapsedOnStartup",
];

/// A configuration file's modification time and length, or `None` when it is missing.
pub(super) type ConfigStamp = (Option<std::time::SystemTime>, u64);

/// The current stamps of the settings and keybindings files.
pub(super) fn config_stamps(paths: &tide_config::Paths) -> [Option<ConfigStamp>; 2] {
    [&paths.settings_file, &paths.keybindings_file].map(|path| {
        std::fs::metadata(path)
            .ok()
            .map(|metadata| (metadata.modified().ok(), metadata.len()))
    })
}

/// The built-in settings with the user's file merged in. A value of the wrong type or
/// range is replaced by its default and reported. `None` when the file does not parse:
/// the caller keeps `fallback`, which the report names.
pub(super) fn load_settings(
    paths: &tide_config::Paths,
    problems: &mut Vec<String>,
    fallback: &str,
) -> Option<Settings> {
    let mut settings = crate::catalog::settings();
    if !paths.settings_file.exists() {
        return Some(settings);
    }
    let user = tide_config::load_jsonc::<Settings>(&paths.settings_file)
        .inspect_err(|error| {
            tracing::warn!(%error, "user settings were not loaded");
            problems.push(format!("{error}. Tide is using the {fallback}."));
        })
        .ok()?;
    let defaults = settings.clone();
    settings.merge(&user);
    let invalid = settings
        .iter()
        .filter(|(key, value)| {
            crate::preferences::supported(key) && crate::preferences::validate(key, value).is_err()
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    for key in invalid {
        problems.push(format!(
            "{}: “{key}” has an invalid value, so Tide is using its default.",
            paths.settings_file.display()
        ));
        if let Some(value) = defaults.get(&key) {
            settings.set(key, value.clone());
        }
    }
    Some(settings)
}

/// The entries of the user's keybindings file, or `None` when it does not parse.
pub(super) fn load_user_keybindings(
    paths: &tide_config::Paths,
    problems: &mut Vec<String>,
    fallback: &str,
) -> Option<Vec<Keybinding>> {
    if !paths.keybindings_file.exists() {
        return Some(Vec::new());
    }
    tide_config::load_jsonc::<Vec<Keybinding>>(&paths.keybindings_file)
        .inspect_err(|error| {
            tracing::warn!(%error, "user keybindings were not loaded");
            problems.push(format!("{error}. Tide is using the {fallback}."));
        })
        .ok()
}

/// The built-in keybindings with `user` entries replacing the binding for the same key
/// and context, or added after them.
pub(super) fn merged_keybindings(user: &[Keybinding]) -> Vec<Keybinding> {
    let mut keybindings = tide_config::builtin::keybindings();
    for binding in user {
        let canonical = canonical_key(&binding.key);
        if let Some(existing) = keybindings.iter_mut().find(|existing| {
            canonical_key(&existing.key) == canonical && existing.when == binding.when
        }) {
            *existing = binding.clone();
        } else {
            keybindings.push(binding.clone());
        }
    }
    keybindings
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
