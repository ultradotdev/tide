# Editor and Code Intelligence

The Tide editor combines a terminal-cell-aware text model with syntax highlighting and optional language-server features.

## Opening and saving

| To | Press |
| --- | --- |
| Open a file by name | {{key:workbench.quickOpen}} |
| New file | {{key:file.new}} |
| Save | {{key:file.save}} |
| Save As | {{key:file.saveAs}} |
| Close the tab | {{key:file.close}} |
| Go to a line | {{key:editor.gotoLine}} |

**{{label:file.saveAll}}**, **{{label:file.reload}}** and **{{label:file.overwrite}}** are in the palette. When a file changes on disk while it is open, Tide blocks saving over it until you reload or explicitly overwrite. Files that are not valid UTF-8 open read-only so a save cannot corrupt them; Save As writes a copy.

## Editing

Undo, redo, selection, comments, folding, automatic indentation, auto-closing pairs (`()`, `[]`, `{}`, quotes and backticks), bracket matching and word wrap all work as you would expect.

| To | Press |
| --- | --- |
| Undo / redo | {{key:edit.undo}} / {{key:edit.redo}} |
| Toggle a line comment | {{key:editor.toggleComment}} |
| Select the line | {{key:editor.selectLine}} |
| Duplicate the selection or line | {{key:editor.duplicateSelection}} |
| Add the next match to the selection | {{key:edit.selectNextMatch}} |
| Select every match | {{key:edit.selectAllOccurrences}} |
| Add a cursor above / below | {{key:editor.addCursorAbove}} / {{key:editor.addCursorBelow}} |
| Back to one cursor | {{key:editor.clearCursors}} |
| Fold / unfold the region | {{key:editor.fold}} / {{key:editor.unfold}} |

**{{label:editor.foldAll}}** and **{{label:editor.unfoldAll}}** are in the palette, and the fold arrows in the gutter respond to clicks. Folding follows indentation and multi-line brackets. Moving the caret highlights the innermost matching brackets and the lines that open and close them; brackets inside strings and comments are ignored.

Word wrap follows `editor.wordWrap` ({{setting:editor.wordWrap}}): `off`, `on`, or `bounded` and `wordWrapColumn` at `editor.wordWrapColumn` ({{setting:editor.wordWrapColumn}}). **{{label:workbench.toggleWordWrap}}** switches it on or off until Tide restarts. The minimap beside the scrollbar follows `editor.minimap.enabled` ({{setting:editor.minimap.enabled}}) and needs an editor at least 30 columns wide.

## Find and replace

{{key:search.find}} opens Find and {{key:search.replace}} opens Find and Replace. Matching is case-insensitive until you change it, and the options toggle while you type:

| In the Find bar | Press |
| --- | --- |
| Next / previous match | `Enter` / `Shift+Enter` |
| Match case | `Alt+C` |
| Whole word | `Alt+W` |
| Regular expression | `Alt+R` |
| Only in the selection | `Alt+S` |
| Switch replace on or off | {{key:search.replace}} |
| Close | `Esc` |

Tab moves to the buttons, including Replace and Replace All. Regular-expression replacements expand `$1` and `${name}`. An invalid pattern shows an error and changes nothing, and replacements keep undo history. After closing the bar, {{key:search.findNext}} and {{key:search.findPrevious}} repeat the search.

{{key:search.findInFiles}} searches the whole workspace in the background and lists the matches; the search is bounded and stops when you close it.

## Syntax highlighting

Tide highlights TypeScript, JavaScript, JSON, Python, Rust, Go, C and C++, HTML, CSS, Markdown, YAML, TOML, shell, Ruby, Java, Kotlin, Swift, PHP, SQL, GraphQL, Lua and other common formats. Each built-in theme brings its own syntax colors, independent of the terminal's ANSI palette.

## Language servers

Servers start on demand when their executable is on the `PATH`:

{{languageservers}}

With a server running you get completion ({{key:lsp.triggerCompletion}}), hover ({{key:lsp.showHover}}), signature help ({{key:lsp.triggerSignatureHelp}}), go to definition ({{key:lsp.goToDefinition}}), code actions ({{key:lsp.codeActions}}), and from the palette **{{label:lsp.findReferences}}**, **{{label:lsp.rename}}**, **{{label:editor.formatDocument}}** and diagnostics navigation. Without a server, editing and highlighting work as usual. Run `tide --check-lsp` to see which servers are installed.

To use another server or turn one off, add `lsp.servers` to {{path:settings}}. Each key is a language ID from the table's languages (`typescript`, `rust`, `python`, `go`, `ruby`, `c`, `cpp`, `json`, `html`, `css`, `sql` and so on); the value is a command with optional arguments, or `null` to disable that language:

```jsonc
"lsp.servers": {
  "python": { "command": "pyright-langserver", "args": ["--stdio"] },
  "sql": null
}
```

The `lsp.*` settings in {{help:settings}} control hover, signature help, diagnostics and completion timing.

## Navigation

| To | Press |
| --- | --- |
| Go to a symbol in this file | {{key:editor.gotoSymbol}} |
| Go to a symbol in the workspace | {{key:editor.gotoWorkspaceSymbol}} |
| Next / previous diagnostic | {{key:lsp.nextDiagnostic}} / {{key:lsp.previousDiagnostic}} |

The Outline section follows the active document, and Timeline shows the file's or the repository's history.

## Explorer and scrolling

Explorer folders expand and collapse with their arrow or with Left and Right. With the Explorer focused, {{key:fileTree.newFile}} creates a file, {{key:fileTree.newFolder}} a folder, {{key:fileTree.rename}} renames and {{key:fileTree.delete}} deletes after asking. File icons use Nerd Font glyphs; set `tui.iconStyle` to `ascii` if your font lacks them. Scrollbars are proportional to the document.
