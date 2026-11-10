# Themes and Customization

One semantic theme colors the workbench, the syntax highlighting and the terminal palette. Fresh installs use Ultra Vivid. Tide also ships the other Ultra themes, Ultra Broadcast and Ultra Dusk. In a terminal without 24-bit color, Tide shows each theme color as the nearest of the 256-color palette; see `tui.colorMode` in {{help:settings}}.

## Choose a theme

| To | Press |
| --- | --- |
| Pick a theme from a list | {{key:workbench.selectTheme}} |
| Next / previous theme | {{key:theme.next}} / {{key:theme.previous}} |
| Switch between light and dark | {{key:theme.toggleDarkMode}} |

The change applies immediately and is saved as `workbench.colorTheme` in {{path:settings}}. Next and previous go through the themes in ID order; switching between light and dark picks the first theme of the other type.

{{themes}}

The bundled palettes from other projects keep their attribution and pinned provenance; see {{doc:THEME_ATTRIBUTION.md|theme attribution}}.

## Your own themes

Put `.json` or `.jsonc` theme files in {{path:themes}}. Tide notices within a couple of seconds of saving and reloads them, so edits to the theme you're using show at once. The file name without its extension is the theme's ID (letters, digits, `-` and `_`). A file with a built-in theme's ID replaces that theme. A file that cannot be loaded is skipped, listed in **Configuration Problems**, and the built-in themes stay as they are. The {{doc:THEME_AUTHORING.md|theme authoring guide}} describes the format and every color role.

## Settings

Run {{key:workbench.openSettings}} to browse and change settings, or edit {{path:settings}} directly; Tide applies your edits when you save the file and rewrites it when you change a setting in the app. Values that fail validation fall back to their defaults, and keys Tide does not know are kept but ignored. {{help:settings}} lists every setting with its default and current value.

## Keybindings

{{key:workbench.openKeybindings}} lists the active keys and changes the one you pick. Your own bindings are kept in {{path:keybindings}} as a JSON list:

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

An entry with the same key and `when` context as a default replaces it; other entries are added. A command of `""` or `-<command>` removes the key. Context bindings win over global ones for the same key. Edits you make to the file by hand apply when you save it. {{help:keyboard-shortcuts}} lists every key and command ID, and {{help:commands}} lists the commands.

## Startup

`workbench.startupEditor` ({{setting:workbench.startupEditor}}) decides when the Welcome guide opens on its own: `welcomeOnFirstRun` the first time Tide starts without a file or restored session, `welcome` every time it does, and `none` never. `session.restoreOnStartup` ({{setting:session.restoreOnStartup}}) reopens the last session for the folder.
