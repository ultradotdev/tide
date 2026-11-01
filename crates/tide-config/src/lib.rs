//! Configuration, catalogs, themes, paths, and session structures for Tide.

pub mod builtin;
mod catalog;
mod paths;
mod session;
mod settings;
mod theme;

pub use catalog::{CommandInfo, Keybinding, ThemeCatalog, load_commands, load_keybindings};
pub use paths::Paths;
pub use session::{
    SessionAICLIPaneState, SessionBottomTerminalState, SessionDocumentState, SessionLayoutNode,
    SessionSQLEditorState, SessionState, SessionTerminalState, SessionUIState,
};
pub use settings::Settings;
pub use theme::{Scope, Theme, ThemeSource, TokenColor, TokenSettings};

use std::path::{Path, PathBuf};

/// Why configuration could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not parse {}: {source}", path.display())]
    Parse {
        path: PathBuf,
        source: jsonc_parser::errors::ParseError,
    },
    #[error("invalid JSONC: {0}")]
    Syntax(#[from] jsonc_parser::errors::ParseError),
    #[error("unsupported Tide session version {0} (expected 7); the session was not modified")]
    UnsupportedSessionVersion(u32),
}

/// Load and version-check a Tide session without modifying it.
pub fn load_session(path: impl AsRef<Path>) -> Result<SessionState, ConfigError> {
    let session: SessionState = load_jsonc(path)?;
    session.validate_version()?;
    Ok(session)
}

/// Parse a JSONC or JSON document into a typed value.
pub fn load_jsonc<T: serde::de::DeserializeOwned>(
    path: impl AsRef<Path>,
) -> Result<T, ConfigError> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    jsonc_parser::parse_to_serde_value(&text, &Default::default()).map_err(|source| {
        ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        }
    })
}

/// Parse a JSONC or JSON string into a typed value.
pub fn parse_jsonc<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, ConfigError> {
    Ok(jsonc_parser::parse_to_serde_value(
        text,
        &Default::default(),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_fixture() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/session-v7.json")
    }

    #[test]
    fn parses_v7_session_fixture() {
        let session = load_session(session_fixture()).unwrap();
        assert_eq!(session.version, 7);
        assert_eq!(session.documents.len(), 2);
        assert_eq!(session.layout.node_type, "horizontal");
        assert_eq!(session.layout.children.as_ref().unwrap().len(), 2);
        assert_eq!(session.ui.sidebar_width, 36);
    }

    #[test]
    fn rejects_unknown_session_versions() {
        let mut session = load_session(session_fixture()).unwrap();
        session.version = 99;
        let error = session.validate_version().unwrap_err().to_string();
        assert!(error.contains("unsupported Tide session version 99"));
    }

    #[test]
    fn arbitrary_session_and_catalog_text_never_panics() {
        let alphabet = b"{}[]\"':,\n\t 0123456789abcdefghijklmnopqrstuvwxyz-_.!@#$%^&*()";
        let mut state = 0x1234_5678_u64;
        for _ in 0..300 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            // The low byte is `state % 256`.
            let length = usize::from(state.to_le_bytes()[0]) + 1;
            let mut text = String::new();
            let alphabet_len = u64::try_from(alphabet.len()).expect("alphabet length fits u64");
            for _ in 0..length {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                let index = usize::try_from(state % alphabet_len)
                    .expect("index is below the alphabet length");
                text.push(alphabet[index] as char);
            }
            let _ = parse_jsonc::<SessionState>(&text);
            let _ = parse_jsonc::<Settings>(&text);
            let _ = parse_jsonc::<ThemeSource>(&text);
        }
    }

    #[test]
    fn load_errors_name_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let broken = directory.path().join("settings.json");
        std::fs::write(&broken, "{\"editor.tabSize\": }").unwrap();
        let error = load_jsonc::<Settings>(&broken).unwrap_err();
        assert!(matches!(error, ConfigError::Parse { .. }));
        let message = error.to_string();
        assert!(message.contains(&broken.display().to_string()), "{message}");
        assert!(message.contains("line 1"), "{message}");

        let missing = directory.path().join("keybindings.json");
        let error = load_jsonc::<Vec<Keybinding>>(&missing).unwrap_err();
        assert!(matches!(error, ConfigError::Read { .. }));
        assert!(error.to_string().contains(&missing.display().to_string()));
    }
}
