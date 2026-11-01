//! Service-layer spikes for Tide: syntax highlighting and Git inspection.
//!
//! This crate deliberately starts with narrow, testable surfaces. File, LSP,
//! session, and search services join it as the port progresses.

pub mod git;
pub mod git_process;
pub mod syntax;

pub use git::{GitChange, GitSnapshot, GitStatus, repository_snapshot};
pub use syntax::{HighlightSpan, SyntaxHighlighter, syntect_theme_from_tide};

mod syntax_document;
