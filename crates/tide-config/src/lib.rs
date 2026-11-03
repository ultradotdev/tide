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

/// Why a JSONC document could not be edited in place.
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("{0}")]
    Syntax(#[from] jsonc_parser::errors::ParseError),
    #[error("the file must contain a JSON {0}")]
    Shape(&'static str),
}

/// Sets the top-level `key` of a JSONC object to `value`, keeping the author's comments,
/// ordering and formatting. An empty document becomes a new object.
pub fn set_jsonc_member(
    text: &str,
    key: &str,
    value: &serde_json::Value,
) -> Result<String, EditError> {
    let root = parse_cst(text, "{}")?;
    let object = root.object_value().ok_or(EditError::Shape("object"))?;
    match object.get(key) {
        Some(property) => property.set_value(cst_value(value)),
        None => {
            object.append(key, cst_value(value));
        }
    }
    Ok(root.to_string())
}

/// Appends `values` to a JSONC array, keeping the author's comments and formatting. An
/// empty document becomes a new array.
pub fn append_jsonc_elements(
    text: &str,
    values: &[serde_json::Value],
) -> Result<String, EditError> {
    let root = parse_cst(text, "[]")?;
    let array = root.array_value().ok_or(EditError::Shape("array"))?;
    for value in values {
        array.append(cst_value(value));
    }
    Ok(root.to_string())
}

fn parse_cst(text: &str, empty: &str) -> Result<jsonc_parser::cst::CstRootNode, EditError> {
    let text = if text.trim().is_empty() {
        format!("{empty}\n")
    } else {
        text.to_owned()
    };
    Ok(jsonc_parser::cst::CstRootNode::parse(
        &text,
        &Default::default(),
    )?)
}

fn cst_value(value: &serde_json::Value) -> jsonc_parser::cst::CstInputValue {
    use jsonc_parser::cst::CstInputValue;
    match value {
        serde_json::Value::Null => CstInputValue::Null,
        serde_json::Value::Bool(value) => CstInputValue::Bool(*value),
        serde_json::Value::Number(number) => CstInputValue::Number(number.to_string()),
        serde_json::Value::String(text) => CstInputValue::String(text.clone()),
        serde_json::Value::Array(items) => {
            CstInputValue::Array(items.iter().map(cst_value).collect())
        }
        serde_json::Value::Object(map) => CstInputValue::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), cst_value(value)))
                .collect(),
        ),
    }
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

    #[test]
    fn editing_a_jsonc_member_keeps_comments_and_other_values() {
        let text = "{\n  // my font size\n  \"editor.tabSize\": 2, // two\n  \"editor.wordWrap\": \"on\"\n}\n";
        let edited = set_jsonc_member(text, "editor.tabSize", &serde_json::json!(4)).unwrap();
        assert_eq!(
            edited,
            "{\n  // my font size\n  \"editor.tabSize\": 4, // two\n  \"editor.wordWrap\": \"on\"\n}\n"
        );
        let added =
            set_jsonc_member(&edited, "workbench.colorTheme", &serde_json::json!("x")).unwrap();
        assert!(added.contains("// my font size"), "{added}");
        let value: serde_json::Value = parse_jsonc(&added).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"editor.tabSize": 4, "editor.wordWrap": "on", "workbench.colorTheme": "x"})
        );
        let fresh = set_jsonc_member("", "a", &serde_json::json!({"b": [1, null]})).unwrap();
        assert_eq!(
            parse_jsonc::<serde_json::Value>(&fresh).unwrap(),
            serde_json::json!({"a": {"b": [1, null]}})
        );
    }

    #[test]
    fn a_broken_or_wrongly_shaped_document_is_not_rewritten() {
        assert!(matches!(
            set_jsonc_member("{ \"a\": ", "a", &serde_json::json!(1)),
            Err(EditError::Syntax(_))
        ));
        assert!(matches!(
            set_jsonc_member("[]", "a", &serde_json::json!(1)),
            Err(EditError::Shape("object"))
        ));
        let bindings = append_jsonc_elements(
            "[\n  // mine\n  { \"key\": \"f5\", \"command\": \"a\" }\n]\n",
            &[serde_json::json!({"key": "f6", "command": "b"})],
        )
        .unwrap();
        assert!(bindings.contains("// mine"), "{bindings}");
        assert_eq!(
            parse_jsonc::<serde_json::Value>(&bindings).unwrap()[1]["key"],
            "f6"
        );
    }
}
