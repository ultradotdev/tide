//! Domain controllers. Each owns its state and background work; [`crate::app::App`] routes
//! input, commands and events to them and applies the [`Effect`]s they ask for.
use std::path::{Path, PathBuf};

use tide_config::Settings;

use crate::app::Focus;
use crate::command::Command;
use crate::editor::{DocumentHandle, DocumentRegistry};
use crate::modal::Modal;
use crate::workbench::{Tab, Workbench};

pub(crate) mod ai;
pub(crate) mod db;
pub(crate) mod git;
pub(crate) mod lsp;

/// The workbench surface a controller may change while it handles work.
pub(crate) struct Ui<'a> {
    pub workspace: &'a Path,
    pub settings: &'a Settings,
    pub workbench: &'a mut Workbench,
    pub documents: &'a mut DocumentRegistry,
    pub overlay: &'a mut Option<Box<dyn Modal>>,
    pub focus: &'a mut Focus,
    pub session_dirty: &'a mut bool,
    pub effects: Vec<Effect>,
}

/// Work that belongs to the app rather than the controller asking for it.
pub(crate) enum Effect {
    Status(String),
    /// Opens a file, optionally at a 1-based line.
    OpenFile {
        path: PathBuf,
        line: Option<usize>,
    },
    /// Puts text on the clipboard, then reports `done`.
    Copy {
        text: String,
        done: String,
    },
    FileDecorations(Vec<(PathBuf, String)>),
    /// Runs a workbench command.
    Command(Command),
    HunkSaved {
        document: DocumentHandle,
        version: u64,
        success: bool,
    },
}

impl Ui<'_> {
    /// Records that the session snapshot is out of date.
    pub fn mark_session_dirty(&mut self) {
        *self.session_dirty = true;
    }

    pub fn status(&mut self, message: impl Into<String>) {
        self.effects.push(Effect::Status(message.into()));
    }

    pub fn setting_bool(&self, key: &str, default: bool) -> bool {
        self.settings
            .get(key)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(default)
    }

    pub fn setting_u64(&self, key: &str, default: u64) -> u64 {
        self.settings
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(default)
    }

    /// Opens `tab` in the focused pane and focuses the main region.
    pub fn open_tab(&mut self, tab: Tab) {
        self.workbench.insert_tab_focused(tab);
        *self.focus = Focus::Main;
        self.mark_session_dirty();
    }
}

/// The `editor.tabSize` setting, clamped to a usable width.
pub(crate) fn tab_size(settings: &Settings) -> usize {
    settings
        .get("editor.tabSize")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(2)
        .clamp(1, 32) as usize
}
