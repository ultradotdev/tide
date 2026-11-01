# Editor and Code Intelligence

The Tide editor combines a terminal-cell-aware text model with syntax highlighting and optional language-server features.

## Editing

The editor supports undo and redo, selection, multiline operations, comments, folding, search and replace, word wrap, bracket matching, automatic indentation, auto-pairs, multiple cursors, occurrence selection, and configurable minimap behavior.

Cursor and selection calculations use grapheme and display-cell boundaries so rendering and hit testing share terminal geometry.

Moving the caret highlights the innermost matching delimiters and their opening/closing statement lines. Strings and comments do not create matching pairs. Indentation-based blocks also mark their header and final body line. Selection and search highlighting take priority over the nesting decoration.

Clicking a fold arrow preserves a manually scrolled viewport. An unrelated caret outside the viewport stays offscreen until you navigate or click back into the text.

## Syntax highlighting

Tide maps common file extensions to language-aware syntax grammars. Supported formats include TypeScript, JavaScript, JSON, Python, Rust, Go, C/C++, HTML, CSS, Markdown, YAML, TOML, shell, Ruby, Java, Kotlin, Swift, PHP, SQL, GraphQL, Lua, and additional common languages.

Every built-in theme has its own source-backed syntax mapping. Syntax colors are independent from terminal ANSI colors and workbench semantic roles.

## Language servers

Tide has default executable configurations for:

- TypeScript and JavaScript via `typescript-language-server`
- Rust via `rust-analyzer`
- Python via `pylsp`
- Go via `gopls`
- Ruby via `solargraph`
- C and C++ via `clangd`
- JSON, HTML, and CSS via the corresponding VS Code language servers
- SQL via `postgres-language-server`

Servers start on demand when their executable is available. Completion, hover, signature help, definitions, diagnostics, document symbols, workspace symbols, references, and rename use the shared LSP service. Missing servers do not prevent basic editing.

## Navigation

Quick Open finds files by name. Document and workspace symbol pickers provide fuzzy navigation. Outline follows the active document, while Timeline exposes file or repository history.

See [Keyboard shortcuts](../reference/keyboard-shortcuts.md) for editing and LSP bindings.

## Find and replace

The find/replace surface supports case-sensitive, whole-word and regular-expression matching, plus a selection scope. Toggle the visible options with their controls before searching. Invalid patterns show an error without changing the document. Regex replacement supports capture expansion. Replace-one and replace-all keep undo history and respect the chosen scope.

Workspace search runs separately in the background. Large searches are bounded and cancellable; throughput optimization remains separate from correctness and input responsiveness.

## Explorer and scroll controls

Folder rows expand and collapse with their disclosure control or Left/Right keys. File-type icons use Nerd Font glyphs, with `tui.iconStyle: "ascii"` as a fallback. Proportional scrollbars show the viewport relative to the document, and the minimap renders compact content density rather than full-size source text.
