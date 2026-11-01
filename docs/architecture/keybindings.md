# Keybinding Resolution

Rust loads embedded JSONC defaults and the Rust overlay, then merges user entries by key and context. Terminal-safe alternate bindings are installed through the same runtime catalog used by generated documentation.

Command aliases canonicalize before dispatch. Supported IDs are checked against routed handlers. Context-specific matches outrank unconditional matches; the last match within a context wins. An empty command or a command beginning with `-` removes that binding. User removals retain ownership of their key when alternate defaults are considered.

Overlays receive input before the workbench. Focused SQL, editor, sidebar and terminal surfaces supply their own contexts. Workbench shortcuts are intercepted before bytes reach a hosted terminal process.

Use `Ctrl+K` for the command palette when a terminal cannot transmit Ctrl+Shift combinations. See the [generated defaults](../reference/keyboard-shortcuts.md) for actual effective bindings. That table comes from compiled Rust metadata rather than a source-only JSONC listing.
