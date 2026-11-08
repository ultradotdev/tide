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

**{{label:file.saveAll}}**, **{{label:file.reload}}** and **{{label:file.overwrite}}** are in the palette. When an open file changes on disk, after a `git checkout`, a formatter or an agent's edit, a tab with no unsaved edits reloads by itself, keeping the cursor near its place; **{{label:edit.undo}}** brings back the previous text. If the tab has unsaved edits, Tide says so once and blocks saving over the newer file until you run **{{label:file.reload}}** or **{{label:file.overwrite}}**. A file deleted on disk stays open with its text. Files that are not valid UTF-8 open read-only so a save cannot corrupt them; Save As writes a copy.

A file you do not have permission to write shows **Read-only** in the status bar and `[RO]` on its tab. You can still edit it, but saving asks first: confirming replaces the file's contents and keeps its read-only permissions. If the folder itself is read-only, the save fails and says so. **{{label:file.saveAll}}** and saving before closing a tab skip read-only files and list them; save each one on its own.

Go to Line opens with the current line number selected, so typing a number replaces it. A line already on screen stays where it is; any other line is scrolled to the middle of the editor.

If the terminal window closes, an SSH connection drops or Tide receives `SIGTERM`, Tide keeps your unsaved edits in the session recovery snapshot straight away, the same as after a crash, and quits. Your files are not written. Start Tide in the same folder again and the edits are back, still unsaved.

## Editing

Undo, redo, selection, comments, folding, automatic indentation, auto-closing pairs (`()`, `[]`, `{}`, quotes and backticks), bracket matching and word wrap all work as you would expect.

To move the cursor, `Ctrl+Left` and `Ctrl+Right` (`Option+Left` and `Option+Right` on macOS, which terminals send as Alt or as `Alt+B` and `Alt+F`) move by word, and `Ctrl+Home` and `Ctrl+End` go to the start and end of the file. `Home` goes to the first non-blank character, then to column 0 on a second press. Hold Shift to select as you move. `Ctrl+Backspace` or `Alt+Backspace` deletes the word before the cursor, and `Ctrl+Delete` or `Alt+D` deletes the word after it. Some terminals send `Ctrl+Backspace` as `Ctrl+H` (Replace); use `Alt+Backspace` in those. Where the terminal supports the kitty keyboard protocol, `Cmd+Left` and `Cmd+Right` go to the line's start and end, and `Cmd+Up` and `Cmd+Down` to the file's.

| To | Press |
| --- | --- |
| Undo / redo | {{key:edit.undo}} / {{key:edit.redo}} |
| Toggle a line comment | {{key:editor.toggleComment}} |
| Toggle a block comment | {{key:editor.toggleBlockComment}} |
| Move the line up / down | {{key:edit.moveLinesUp}} / {{key:edit.moveLinesDown}} |
| Indent / outdent the selected lines | `Tab` / `Shift+Tab` |
| Select the line | {{key:editor.selectLine}} |
| Duplicate the selection or line | {{key:editor.duplicateSelection}} |
| Add the next match to the selection | {{key:edit.selectNextMatch}} |
| Select every match | {{key:edit.selectAllOccurrences}} |
| Add a cursor above / below | {{key:editor.addCursorAbove}} / {{key:editor.addCursorBelow}} |
| Back to one cursor | {{key:editor.clearCursors}} |
| Fold / unfold the region | {{key:editor.fold}} / {{key:editor.unfold}} |

Line commands act on every cursor and count as one undo step. More are in the palette: **{{label:edit.deleteLine}}**, **{{label:edit.joinLines}}**, **{{label:edit.transformToUppercase}}**, **{{label:edit.transformToLowercase}}** and **{{label:edit.transformToTitleCase}}** (the selection, or the word at the cursor), **{{label:editor.sortLinesAscending}}** and **{{label:editor.sortLinesDescending}}** (the selected lines, or the whole file), and **{{label:editor.trimTrailingWhitespace}}**. Help pages are read-only, so these leave them unchanged.

**{{label:editor.foldAll}}**, **{{label:editor.unfoldAll}}** and **{{label:editor.foldLevel1}}** to level 3 are in the palette, and the fold arrows in the gutter respond to clicks. Folding follows indentation and multi-line brackets. Moving the caret highlights the innermost matching brackets and the lines that open and close them; brackets inside strings and comments are ignored.

## Indentation and .editorconfig

Tab and automatic indentation use the indentation the file already has. When a file opens, Tide looks at its first 1,000 lines: it uses tabs or spaces, whichever more lines start with, and takes the width from the most common step in indentation between neighbouring lines. A file with too little indentation to tell uses `editor.tabSize` ({{setting:editor.tabSize}}) and `editor.insertSpaces` ({{setting:editor.insertSpaces}}). Set `editor.detectIndentation` ({{setting:editor.detectIndentation}}) to `false` to use those two settings for every file. The status bar shows what the active file uses, such as `Spaces: 4` or `Tab Size: 8`.

An [EditorConfig](https://editorconfig.org) file overrides both. Tide reads the `.editorconfig` files from the file's folder upward, stopping at one that says `root = true`; nearer files and later sections win. It applies these properties:

| Property | Effect in Tide |
| --- | --- |
| `indent_style` | `tab` or `space` for Tab and automatic indentation |
| `indent_size`, `tab_width` | The indent width, and how wide a tab character is drawn |
| `end_of_line` | The line break for a new file or one with no line breaks yet; Tide does not convert an existing file's line breaks |
| `insert_final_newline = true` | Adds a line break at the end of the file when you save |
| `trim_trailing_whitespace = true` | Removes spaces and tabs at the ends of lines when you save |
| `charset` | Not applied: Tide reads and writes UTF-8 only |

Changes made on save are one undo step, so the buffer matches the file. Tide reads `.editorconfig` when it opens a file; saving an `.editorconfig` in Tide applies it to the open files.

Word wrap follows `editor.wordWrap` ({{setting:editor.wordWrap}}): `off`, `on`, or `bounded` and `wordWrapColumn` at `editor.wordWrapColumn` ({{setting:editor.wordWrapColumn}}). **{{label:workbench.toggleWordWrap}}** switches it on or off until Tide restarts. The minimap beside the scrollbar follows `editor.minimap.enabled` ({{setting:editor.minimap.enabled}}) and needs an editor at least 30 columns wide; **{{label:workbench.toggleMinimap}}** shows or hides it, and the session remembers the choice.

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

## Markdown preview

Markdown files (`.md` and `.markdown`) open formatted: headings, bold and italic, code, links, lists, quotes and tables, laid out for the pane's width. The status bar shows **Preview**; click it, or run **{{label:markdown.togglePreview}}**, to see the source in the same tab and edit it, and the status bar shows **Edit**. In the preview the arrow keys, Page Up, Page Down, Space, Home, End and the mouse wheel scroll, and typing does nothing. Commands that work on the text, such as find, undo or paste, switch to the source first. Saving from the preview saves the source. `editor.markdown.openMode` ({{setting:editor.markdown.openMode}}) chooses how Markdown files open: `preview` or `edit`. The session remembers which view each tab had.

## Language servers

Servers start on demand when their executable is on the `PATH`:

{{languageservers}}

For TypeScript and JavaScript, Tide uses the TypeScript nearest the workspace: the closest `node_modules/typescript` at or above it, then the global one (`npm root -g`). TypeScript 7 is its own language server, so Tide runs its `tsc --lsp --stdio` and typescript-language-server isn't needed. TypeScript 5 runs through typescript-language-server, given that TypeScript's `tsserver.js`. With no TypeScript found, the status bar says so; install one (`npm install -g typescript`) and run **{{label:lsp.restartServer}}**.

With a server running you get completion ({{key:lsp.triggerCompletion}}), hover ({{key:lsp.showHover}}), signature help ({{key:lsp.triggerSignatureHelp}}), go to definition ({{key:lsp.goToDefinition}}), go to implementation ({{key:lsp.goToImplementation}}), code actions ({{key:lsp.codeActions}}), and from the palette **{{label:lsp.goToDeclaration}}**, **{{label:lsp.goToTypeDefinition}}**, **{{label:lsp.findReferences}}**, **{{label:lsp.rename}}**, **{{label:editor.formatDocument}}**, **{{label:editor.formatSelection}}** and diagnostics navigation. When a lookup finds several places, a picker lists them; when the server does not offer a feature, the status bar says so. **{{label:lsp.restartServer}}** restarts the active file's server and reopens its documents. Without a server, editing and highlighting work as usual. Run `tide --check-lsp` to see which servers are installed.

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

Explorer folders expand and collapse with their arrow or with Left and Right. With the Explorer focused, {{key:fileTree.newFile}} creates a file, {{key:fileTree.newFolder}} a folder, {{key:fileTree.rename}} renames and {{key:fileTree.delete}} deletes after asking.

From the palette, **{{label:file.rename}}**, **{{label:file.delete}}**, **{{label:file.newFolder}}**, **{{label:file.copyPath}}** ({{key:file.copyPath}}), **{{label:file.copyRelativePath}}** and **{{label:file.revealInFinder}}** ({{key:file.revealInFinder}}) act on the active file, or on the Explorer selection while the Explorer has focus. Open tabs follow a rename. Deleting is permanent, since Tide does not use the trash, and closes the file's tabs; the confirmation says when unsaved changes would be lost. Rename and delete only work inside the workspace. File icons use Nerd Font glyphs where the terminal can show them; `tui.iconStyle` chooses (see {{help:settings}}). Scrollbars are proportional to the document.
