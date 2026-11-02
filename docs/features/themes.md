<!-- Generated from crates/tide-ui/src/help/pages/themes.md by `cargo xtask generate-rust-reference --write`. Edit the template, not this file. -->
# Themes and Customization

One semantic theme colors the workbench, the syntax highlighting and the terminal palette. Fresh installs use Ultra Dusk.

## Choose a theme

| To | Press |
| --- | --- |
| Pick a theme from a list | `Ctrl+Alt+R` |
| Next / previous theme | “Next Theme” from the command palette (`Ctrl+K`) / “Previous Theme” from the command palette (`Ctrl+K`) |
| Switch between light and dark | “Toggle Dark Mode” from the command palette (`Ctrl+K`) |

The change applies immediately and is saved as `workbench.colorTheme` in `~/.config/tide/settings.jsonc`. Next and previous go through the themes in ID order; switching between light and dark picks the first theme of the other type.

| Theme | ID | Type |
| --- | --- | --- |
| Catppuccin Frappé | `catppuccin-frappe` | dark |
| Catppuccin Latte | `catppuccin-latte` | light |
| Catppuccin Macchiato | `catppuccin-macchiato` | dark |
| Catppuccin Mocha | `catppuccin-mocha` | dark |
| Dracula | `dracula` | dark |
| Gruvbox Dark | `gruvbox-dark` | dark |
| Nord | `nord` | dark |
| One Dark | `one-dark` | dark |
| Rosé Pine | `rose-pine` | dark |
| Solarized Dark | `solarized-dark` | dark |
| Solarized Light | `solarized-light` | light |
| Tokyo Night | `tokyo-night` | dark |
| Ultra Dusk | `ultra-dusk` | dark |

The bundled palettes from other projects keep their attribution and pinned provenance; see [theme attribution](../THEME_ATTRIBUTION.md).

## Your own themes

Put `.json` or `.jsonc` theme files in `~/.config/tide/themes` and restart Tide; the file name without its extension is the theme's ID (letters, digits, `-` and `_`). A file with a built-in theme's ID replaces that theme. A file that cannot be loaded is skipped and the built-in themes stay as they are; run Tide with `--debug` to see why in `~/.local/state/tide/logs/debug.log`. The [theme authoring guide](../THEME_AUTHORING.md) describes the format and every color role.

## Settings

Run `Ctrl+,` or `Ctrl+Alt+O` to browse and change settings, or edit `~/.config/tide/settings.jsonc` directly; Tide reads it at startup and rewrites it when you change a setting in the app. Values that fail validation fall back to their defaults, and keys Tide does not know are kept but ignored. [Settings Reference](../reference/configuration.md) lists every setting with its default and current value.

## Keybindings

`Ctrl+Shift+,` or `Ctrl+Alt+K` lists the active keys and changes the one you pick. Your own bindings are kept in `~/.config/tide/keybindings.jsonc` as a JSON list:

```jsonc
[
  // Add a key for a command.
  { "key": "ctrl+alt+p", "command": "workbench.commandPalette" },
  // Bind a key only while a panel has focus.
  { "key": "x", "command": "gitPanel.discard", "when": "gitPanelFocus" },
  // Remove a default binding.
  { "key": "ctrl+e", "command": "-workbench.commandPalette" }
]
```

An entry with the same key and `when` context as a default replaces it; other entries are added. A command of `""` or `-<command>` removes the key. Context bindings win over global ones for the same key. Restart Tide after editing the file by hand. [Keyboard Shortcuts](../reference/keyboard-shortcuts.md) lists every key and command ID, and [All Commands](../reference/commands.md) lists the commands.

## Startup

`workbench.startupEditor` (`welcomeOnFirstRun`) decides when the Welcome guide opens on its own: `welcomeOnFirstRun` the first time Tide starts without a file or restored session, `welcome` every time it does, and `none` never. `session.restoreOnStartup` (`true`) reopens the last session for the folder.
