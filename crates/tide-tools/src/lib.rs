//! Workspace-bounded AI tools: canonical definitions, read views, staged change
//! sets, atomic apply, guarded rollback, and validation commands.

pub mod changeset;
pub mod router;
pub mod validation;
pub mod workspace;

pub use changeset::{ApplyReport, ChangeSet, FileChange};
pub use router::{
    ToolCall, ToolDefinition, ToolOutcome, canonical_tools, execute_tool, tool_definition,
};
pub use validation::{ValidationCommand, ValidationOutput, run_validation};
pub use workspace::{BufferSource, MapBuffers, ToolError, Workspace, is_sensitive};
