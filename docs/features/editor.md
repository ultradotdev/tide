<!-- Generated from crates/tide-ui/src/help/pages/editor.md by `cargo xtask generate-rust-reference --write`. Edit the template, not this file. -->
# Editor and Code Intelligence

The Tide editor combines a terminal-cell-aware text model with syntax highlighting and optional language-server features.

## Opening and saving

| To | Press |
| --- | --- |
| Open a file by name | `Ctrl+P` |
| New file | `Ctrl+N` |
| Save | `Ctrl+S` |
| Save As | `Ctrl+Shift+S` or `Ctrl+Alt+S` |
| Close the tab | `Ctrl+W` |
| Go to a line | `Ctrl+G` |

**Save All**, **Revert File** and **Overwrite File on Disk** are in the palette. When a file changes on disk while it is open, Tide blocks saving over it until you reload or explicitly overwrite. Files that are not valid UTF-8 open read-only so a save cannot corrupt them; Save As writes a copy.

## Editing

Undo, redo, selection, comments, folding, automatic indentation, auto-closing pairs (`()`, `[]`, `{}`, quotes and backticks), bracket matching and word wrap all work as you would expect.

| To | Press |
| --- | --- |
| Undo / redo | `Ctrl+Z` / `Ctrl+Shift+Z` or `Ctrl+Y` |
| Toggle a line comment | `Ctrl+/` |
| Toggle a block comment | `Alt+Shift+A` |
| Move the line up / down | `Ctrl+Shift+Up` / `Ctrl+Shift+Down` |
| Indent / outdent the selected lines | `Tab` / `Shift+Tab` |
| Select the line | `Ctrl+L` |
| Duplicate the selection or line | `Ctrl+Shift+D` |
| Add the next match to the selection | `Ctrl+D` |
| Select every match | `Ctrl+Shift+L` |
| Add a cursor above / below | `Ctrl+Alt+Up` / `Ctrl+Alt+Down` |
| Back to one cursor | `Esc` |
| Fold / unfold the region | `Ctrl+Shift+[` / `Ctrl+Shift+]` |

Line commands act on every cursor and count as one undo step. More are in the palette: **Delete Line**, **Join Lines**, **Transform to Uppercase**, **Transform to Lowercase** and **Transform to Title Case** (the selection, or the word at the cursor), **Sort Lines Ascending** and **Sort Lines Descending** (the selected lines, or the whole file), and **Trim Trailing Whitespace**. Help pages are read-only, so these leave them unchanged.

**Fold All**, **Unfold All** and **Fold Level 1** to level 3 are in the palette, and the fold arrows in the gutter respond to clicks. Folding follows indentation and multi-line brackets. Moving the caret highlights the innermost matching brackets and the lines that open and close them; brackets inside strings and comments are ignored.

Word wrap follows `editor.wordWrap` (`on`): `off`, `on`, or `bounded` and `wordWrapColumn` at `editor.wordWrapColumn` (`80`). **Toggle Word Wrap** switches it on or off until Tide restarts. The minimap beside the scrollbar follows `editor.minimap.enabled` (`true`) and needs an editor at least 30 columns wide; **Toggle Minimap** shows or hides it, and the session remembers the choice.

## Find and replace

`Ctrl+F` opens Find and `Ctrl+H` opens Find and Replace. Matching is case-insensitive until you change it, and the options toggle while you type:

| In the Find bar | Press |
| --- | --- |
| Next / previous match | `Enter` / `Shift+Enter` |
| Match case | `Alt+C` |
| Whole word | `Alt+W` |
| Regular expression | `Alt+R` |
| Only in the selection | `Alt+S` |
| Switch replace on or off | `Ctrl+H` |
| Close | `Esc` |

Tab moves to the buttons, including Replace and Replace All. Regular-expression replacements expand `$1` and `${name}`. An invalid pattern shows an error and changes nothing, and replacements keep undo history. After closing the bar, `F3` and `Shift+F3` repeat the search.

`Ctrl+Shift+F` or `Ctrl+Alt+F` searches the whole workspace in the background and lists the matches; the search is bounded and stops when you close it.

## Syntax highlighting

Tide highlights TypeScript, JavaScript, JSON, Python, Rust, Go, C and C++, HTML, CSS, Markdown, YAML, TOML, shell, Ruby, Java, Kotlin, Swift, PHP, SQL, GraphQL, Lua and other common formats. Each built-in theme brings its own syntax colors, independent of the terminal's ANSI palette.

## Language servers

Servers start on demand when their executable is on the `PATH`:

| Languages | Server |
| --- | --- |
| TypeScript, JavaScript, TSX, JSX | `typescript-language-server --stdio` |
| Rust | `rust-analyzer` |
| Python | `pylsp` |
| Go | `gopls` |
| Ruby | `solargraph stdio` |
| C, C++ | `clangd` |
| JSON | `vscode-json-language-server --stdio` |
| HTML | `vscode-html-language-server --stdio` |
| CSS | `vscode-css-language-server --stdio` |
| SQL | `postgres-language-server lsp-proxy` |

With a server running you get completion (`Ctrl+Space`), hover (`Ctrl+I`), signature help (`Ctrl+Shift+Space`), go to definition (`Ctrl+Shift+K`), go to implementation (`Ctrl+F12`), code actions (`Ctrl+.`), and from the palette **Go to Declaration**, **Go to Type Definition**, **Find All References**, **Rename Symbol**, **Format Document**, **Format Selection** and diagnostics navigation. When a lookup finds several places, a picker lists them; when the server does not offer a feature, the status bar says so. **Restart Language Server** restarts the active file's server and reopens its documents. Without a server, editing and highlighting work as usual. Run `tide --check-lsp` to see which servers are installed.

To use another server or turn one off, add `lsp.servers` to `~/.config/tide/settings.jsonc`. Each key is a language ID from the table's languages (`typescript`, `rust`, `python`, `go`, `ruby`, `c`, `cpp`, `json`, `html`, `css`, `sql` and so on); the value is a command with optional arguments, or `null` to disable that language:

```jsonc
"lsp.servers": {
  "python": { "command": "pyright-langserver", "args": ["--stdio"] },
  "sql": null
}
```

The `lsp.*` settings in [Settings Reference](../reference/configuration.md) control hover, signature help, diagnostics and completion timing.

## Navigation

| To | Press |
| --- | --- |
| Go to a symbol in this file | `Ctrl+R` |
| Go to a symbol in the workspace | `Ctrl+Shift+R` |
| Next / previous diagnostic | “Go to Next Problem” from the command palette (`Ctrl+K`) / “Go to Previous Problem” from the command palette (`Ctrl+K`) |

The Outline section follows the active document, and Timeline shows the file's or the repository's history.

## Explorer and scrolling

Explorer folders expand and collapse with their arrow or with Left and Right. With the Explorer focused, `n` creates a file, `Shift+N` a folder, `r` or `F2` renames and `d` or `Delete` deletes after asking.

From the palette, **Rename File...**, **Delete File...**, **New Folder...**, **Copy Path** (`Ctrl+Alt+C`), **Copy Relative Path** and **Reveal in File Manager** (`Alt+Shift+R`) act on the active file, or on the Explorer selection while the Explorer has focus. Open tabs follow a rename. Deleting is permanent, since Tide does not use the trash, and closes the file's tabs; the confirmation says when unsaved changes would be lost. Rename and delete only work inside the workspace. File icons use Nerd Font glyphs; set `tui.iconStyle` to `ascii` if your font lacks them. Scrollbars are proportional to the document.
