//! Canonical AI tool definitions and the workspace-bounded router.

use std::path::Path;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::changeset::{ChangeSet, FileChange, PENDING_READ_PREFIX};
use crate::validation::{ValidationCommand, run_validation};
use crate::workspace::{BufferSource, ToolError, Workspace, is_sensitive};

pub const MAX_READ_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    /// Write-like tools must be approved before their change set is applied.
    pub requires_approval: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub input: Value,
}

#[derive(Debug)]
pub struct ToolOutcome {
    pub content: String,
    pub is_error: bool,
    /// Present for write-like tools; the caller decides whether to apply it.
    pub change_set: Option<ChangeSet>,
}

impl ToolOutcome {
    fn text(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
            change_set: None,
        }
    }

    fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
            change_set: None,
        }
    }
}

pub fn canonical_tools() -> &'static [ToolDefinition] {
    static TOOLS: OnceLock<Vec<ToolDefinition>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        vec![
            ToolDefinition {
                name: "read_file".to_owned(),
                description:
                    "Read a UTF-8 file inside the workspace. Open editor buffers take precedence. \
                     Files in .git, .env*, keys, and credentials need user approval."
                        .to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "required": ["path"]
                }),
                requires_approval: false,
            },
            ToolDefinition {
                name: "list_files".to_owned(),
                description: "List workspace files, honouring .gitignore and skipping .git."
                    .to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 500 }
                    }
                }),
                requires_approval: false,
            },
            ToolDefinition {
                name: "search_files".to_owned(),
                description: "Case-insensitive text search across workspace files.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 500 }
                    },
                    "required": ["query"]
                }),
                requires_approval: false,
            },
            ToolDefinition {
                name: "write_file".to_owned(),
                description:
                    "Stage a file write as a reviewable change set. Does not apply until approved."
                        .to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "content": { "type": "string" }
                    },
                    "required": ["path", "content"]
                }),
                requires_approval: true,
            },
            ToolDefinition {
                name: "delete_file".to_owned(),
                description: "Stage a file deletion as a reviewable change set.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "required": ["path"]
                }),
                requires_approval: true,
            },
            ToolDefinition {
                name: "run_validation".to_owned(),
                description:
                    "Run a validation command inside the workspace. Requires an explicit opt-in."
                        .to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "program": { "type": "string" },
                        "args": { "type": "array", "items": { "type": "string" } },
                        "cwd": { "type": "string" },
                        "timeout_secs": { "type": "integer", "minimum": 1, "maximum": 600 }
                    },
                    "required": ["program"]
                }),
                requires_approval: true,
            },
        ]
    })
}

pub fn tool_definition(name: &str) -> Option<&'static ToolDefinition> {
    canonical_tools().iter().find(|tool| tool.name == name)
}

/// Execute a tool call. `allow_commands` is the user's explicit opt-in for
/// `run_validation`; write tools only stage changes.
pub async fn execute_tool(
    workspace: &Workspace,
    call: &ToolCall,
    buffers: &dyn BufferSource,
    allow_commands: bool,
) -> Result<ToolOutcome, ToolError> {
    match call.name.as_str() {
        "read_file" => {
            let path = string_arg(&call.input, "path")?;
            let resolved = workspace.resolve(&path)?;
            let relative = workspace.relative(&resolved);
            if is_sensitive(Path::new(&path)) || is_sensitive(Path::new(&relative)) {
                let set = ChangeSet::sensitive_read(relative.clone());
                let preview = set.preview();
                return Ok(ToolOutcome {
                    content: format!("{PENDING_READ_PREFIX}{relative}\n{preview}"),
                    is_error: false,
                    change_set: Some(set),
                });
            }
            let content = match buffers.get(&resolved) {
                Some(text) => text,
                None => workspace.read_text(&path)?,
            };
            Ok(ToolOutcome::text(truncate(&content, MAX_READ_BYTES)))
        }
        "list_files" => {
            let path = call.input.get("path").and_then(Value::as_str);
            let limit = call
                .input
                .get("limit")
                .and_then(Value::as_u64)
                .map_or(200, tide_core::num::saturating_usize);
            let files = workspace.list_files(path, limit)?;
            Ok(ToolOutcome::text(files.join("\n")))
        }
        "search_files" => {
            let query = string_arg(&call.input, "query")?;
            let limit = call
                .input
                .get("limit")
                .and_then(Value::as_u64)
                .map_or(100, tide_core::num::saturating_usize);
            let matches = workspace.search(&query, limit)?;
            let text = matches
                .iter()
                .map(|found| format!("{}:{}: {}", found.path, found.line, found.text))
                .collect::<Vec<_>>()
                .join("\n");
            Ok(ToolOutcome::text(text))
        }
        "write_file" => {
            let path = string_arg(&call.input, "path")?;
            let content = string_arg(&call.input, "content")?;
            let change = FileChange::write(workspace, &path, &content, buffers)?;
            if change.before == change.after {
                return Ok(ToolOutcome::text(format!("no changes for {path}")));
            }
            let mut set = ChangeSet::new(vec![change]);
            set.refresh_bases(workspace, buffers)?;
            let preview = set.preview();
            Ok(ToolOutcome {
                content: format!("Pending approval for write_file {path}\n{preview}"),
                is_error: false,
                change_set: Some(set),
            })
        }
        "delete_file" => {
            let path = string_arg(&call.input, "path")?;
            let change = FileChange::delete(workspace, &path)?;
            let mut set = ChangeSet::new(vec![change]);
            set.refresh_bases(workspace, buffers)?;
            let preview = set.preview();
            Ok(ToolOutcome {
                content: format!("Pending approval for delete_file {path}\n{preview}"),
                is_error: false,
                change_set: Some(set),
            })
        }
        "run_validation" => {
            if !allow_commands {
                return Ok(ToolOutcome::error(
                    "run_validation is disabled; the user must opt in to command execution",
                ));
            }
            let command = serde_json::from_value::<ValidationCommand>(call.input.clone())
                .map_err(|error| ToolError::InvalidInput(error.to_string()))?;
            let output = run_validation(workspace, &command).await?;
            let text = serde_json::to_string_pretty(&output)
                .unwrap_or_else(|_| "validation finished".to_owned());
            Ok(ToolOutcome {
                content: text,
                is_error: !output.success,
                change_set: None,
            })
        }
        other => Err(ToolError::UnknownTool(other.to_owned())),
    }
}

fn string_arg(input: &Value, key: &str) -> Result<String, ToolError> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| ToolError::InvalidInput(format!("missing string field `{key}`")))
}

pub(crate) fn truncate(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut boundary = limit;
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}\n… file truncated", &text[..boundary])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::MapBuffers;

    fn buffer_with(path: &str, text: &str) -> (tempfile::TempDir, Workspace, MapBuffers) {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        let mut buffers = MapBuffers::new();
        buffers.insert(workspace.resolve(path).unwrap(), text);
        (dir, workspace, buffers)
    }

    async fn call(
        workspace: &Workspace,
        buffers: &MapBuffers,
        name: &str,
        input: Value,
    ) -> ToolOutcome {
        execute_tool(
            workspace,
            &ToolCall {
                id: "1".to_owned(),
                name: name.to_owned(),
                input,
            },
            buffers,
            false,
        )
        .await
        .unwrap()
    }

    #[test]
    fn canonical_definitions_cover_the_tool_surface() {
        let names = canonical_tools()
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 6);
        assert!(names.contains(&"read_file"));
        assert!(tool_definition("write_file").unwrap().requires_approval);
        assert!(!tool_definition("read_file").unwrap().requires_approval);
    }

    #[tokio::test]
    async fn read_file_prefers_open_buffers() {
        let (_dir, workspace, buffers) = buffer_with("a.txt", "unsaved");
        let outcome = call(
            &workspace,
            &buffers,
            "read_file",
            json!({ "path": "a.txt" }),
        )
        .await;
        assert_eq!(outcome.content, "unsaved");
        assert!(!outcome.is_error);
    }

    #[tokio::test]
    async fn write_file_stages_instead_of_applying() {
        let (_dir, workspace, buffers) = buffer_with("a.txt", "old");
        let outcome = call(
            &workspace,
            &buffers,
            "write_file",
            json!({ "path": "a.txt", "content": "new" }),
        )
        .await;
        assert!(outcome.change_set.is_some());
        assert!(outcome.content.contains("Pending approval"));
        assert_eq!(
            buffers.get(&workspace.resolve("a.txt").unwrap()).unwrap(),
            "old"
        );
    }

    #[tokio::test]
    async fn commands_require_an_explicit_opt_in() {
        let (_dir, workspace, buffers) = buffer_with("a.txt", "x");
        let outcome = call(
            &workspace,
            &buffers,
            "run_validation",
            json!({ "program": "true" }),
        )
        .await;
        assert!(outcome.is_error);
        assert!(outcome.content.contains("opt in"));
    }

    #[tokio::test]
    async fn tool_paths_are_workspace_bounded() {
        let (_dir, workspace, buffers) = buffer_with("a.txt", "x");
        let error = execute_tool(
            &workspace,
            &ToolCall {
                id: "1".to_owned(),
                name: "read_file".to_owned(),
                input: json!({ "path": "../../etc/passwd" }),
            },
            &buffers,
            false,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, ToolError::OutsideWorkspace(_)));
        let error = execute_tool(
            &workspace,
            &ToolCall {
                id: "2".to_owned(),
                name: "nope".to_owned(),
                input: json!({}),
            },
            &buffers,
            false,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, ToolError::UnknownTool(_)));
    }

    #[tokio::test]
    async fn sensitive_reads_wait_for_approval() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".git/config"), "git-secret").unwrap();
        std::fs::write(dir.path().join(".env"), "env-secret").unwrap();
        std::fs::write(dir.path().join("id_ed25519"), "key-secret").unwrap();
        std::fs::write(dir.path().join("credentials.json"), "cred-secret").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path().join(".env"), dir.path().join("notes.txt")).unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        let buffers = MapBuffers::new();
        let mut paths = vec![".git/config", ".env", "id_ed25519", "credentials.json"];
        if cfg!(unix) {
            paths.push("notes.txt");
        }
        for path in paths {
            let outcome = call(&workspace, &buffers, "read_file", json!({ "path": path })).await;
            assert!(!outcome.content.contains("secret"), "{path} leaked");
            assert!(outcome.content.starts_with("Pending approval"), "{path}");
            let mut set = outcome.change_set.expect("a pending read");
            assert!(!set.preview().contains("secret"));
            let report = set.apply(&workspace).unwrap();
            let (content, is_error) = report.resolve_pending(&outcome.content);
            assert!(!is_error, "{path}: {content}");
            assert!(content.ends_with("secret"), "{path}: {content}");
        }
    }

    #[tokio::test]
    async fn merged_change_sets_keep_pending_reads() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "env-secret").unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        let buffers = MapBuffers::new();
        let write = call(
            &workspace,
            &buffers,
            "write_file",
            json!({ "path": "a.txt", "content": "new" }),
        )
        .await;
        let read = call(&workspace, &buffers, "read_file", json!({ "path": ".env" })).await;
        let mut set = write.change_set.unwrap();
        set.merge(read.change_set.unwrap());
        let report = set.apply(&workspace).unwrap();
        assert_eq!(
            report.resolve_pending(&write.content),
            ("Applied changes to a.txt".to_owned(), false)
        );
        assert_eq!(
            report.resolve_pending(&read.content),
            ("env-secret".to_owned(), false)
        );
    }

    #[tokio::test]
    async fn search_and_list_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello\nworld\n").unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        let buffers = MapBuffers::new();
        let outcome = call(
            &workspace,
            &buffers,
            "search_files",
            json!({ "query": "HELLO" }),
        )
        .await;
        assert!(outcome.content.contains("a.txt:1"));
        let outcome = call(&workspace, &buffers, "list_files", json!({})).await;
        assert!(outcome.content.contains("a.txt"));
    }
}
