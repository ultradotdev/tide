# Tide Theme Authoring

Tide accepts JSON or JSONC theme files in the `themes` directory under its configuration root. The filename stem is the theme ID. Tide reloads the folder within a couple of seconds of a file being saved, so there's no need to restart; choose a new theme from Select Color Theme or `workbench.colorTheme`. Edits to the theme you're using show at once.

## Format

```json
{
  "name": "Example Theme",
  "type": "dark",
  "colors": {
    "editor.background": "#101218",
    "editor.foreground": "#e6e9f2",
    "editorCursor.foreground": "#c0abf5",
    "panel.border": "#565666",
    "focusBorder": "#c0abf5"
  },
  "tokenColors": [
    {"scope": ["comment"], "settings": {"foreground": "#89919d", "fontStyle": "italic"}}
  ]
}
```

Use an ID containing ASCII letters, numbers, hyphens or underscores. A user file can override a built-in ID. Invalid files leave the existing built-in theme intact and produce a diagnostic. The loader accepts at most 100 directory entries, 1 MiB per regular file, and 2,000 color or token rules per file; symlink files are ignored with a diagnostic. Names must be nonempty, bounded and free of control characters, and type must be `dark` or `light`.

## Workbench roles

The runtime schema is `ThemeSource` in `crates/tide-config/src/theme.rs`. `crates/tide-ui/src/palette.rs` maps semantic roles into the workbench palette. Start from one of the complete built-ins in `config/themes` to retain the full role inventory.

Examples include `sideBar.background`, `statusBar.background`, `tab.activeBackground`, `input.background`, selection roles, Git decoration colors and diagnostic colors. `successForeground` colors confirmation actions. Destructive actions use `statusBarItem.errorBackground` or `editorError.foreground`. Focus styling must remain visible against structural borders.

Missing or invalid colors receive deterministic runtime fallbacks. This is compatibility behavior; a complete theme should supply intentional values. Workbench colors accept three- or six-digit hexadecimal RGB. Syntax rules additionally support alpha-bearing hexadecimal values.

## Syntax and terminal colors

`tokenColors` contains TextMate scope strings or arrays, foreground/background values and font styles. Syntect maps these into the embedded grammar bundle. Syntax scopes, workbench roles and the sixteen `terminal.ansi*` entries remain distinct. Explicit colors emitted by hosted applications stay child-owned.

Use the same multi-language samples when comparing themes. Check focused and unfocused panes, selected rows, ordinary and destructive dialog actions, diagnostics, Git diffs and terminal output. Keep normal text legible and avoid making focus depend only on a tiny color difference.

## Attribution

Retain the source license and copyright when adapting a theme. Built-in palette provenance is in [theme attribution](THEME_ATTRIBUTION.md); bundled notice text is in [ATTRIBUTIONS.md](ATTRIBUTIONS.md). Ultra Dusk is Tide's original Zorz LLC design.
