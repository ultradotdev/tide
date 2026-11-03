//! Ratatui workbench, panes, editors and overlays for Tide.

pub mod accordion;
pub mod app;
mod catalog;
mod clipboard;
pub mod command;
mod command_aliases;
mod connection_form;
mod controllers;
mod delimiters;
pub mod diff_browser;
mod display;
pub mod editor;
pub mod editorconfig;
pub mod file_tree;
mod git_jobs;
pub mod git_panel;
pub mod help;
mod jobs;
pub mod modal;
pub mod palette;
mod preferences;
mod reference;
pub mod regions;
mod row_insert_form;
pub mod run;
mod schema_browser;
mod scrollbar;
pub mod search;
pub mod session_store;
pub mod sql_tab;
pub mod tab_strip;
pub mod terminal_tab;
#[cfg(test)]
mod test_support;
mod watch;
pub mod workbench;
pub mod workspace_replace;
pub mod workspace_search;

pub use accordion::{AccordionAction, AccordionState, SectionId};
pub use app::{
    AgentDiscovery, App, AppConfig, Focus, HitTarget as AppHitTarget, command_supported,
    discover_agent_sessions,
};
pub use command::Command;
pub use editor::{DocumentHandle, DocumentId, DocumentRegistry, EditorState};
pub use file_tree::FileTreeState;
pub use jobs::Jobs;
pub use modal::{
    ConfirmAction, Modal, ModalBase, ModalItem, ModalKind, ModalOutcome, PromptAction, Submitted,
};
pub use preferences::supported as setting_supported;
pub use run::run;
pub use workbench::{LayoutNode, Pane, SplitDirection, Tab, Workbench};

pub use reference::reference_catalog;

/// Locks `mutex`, recovering its data when another thread panicked while holding it, so a
/// recovery snapshot can still read open documents while the app unwinds.
pub(crate) fn lock<T: ?Sized>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
