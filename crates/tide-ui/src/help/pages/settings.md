# Settings and Storage

Tide starts from its built-in defaults and applies your settings file, {{path:settings}}, on top. Open the Settings list with {{key:workbench.openSettings}} to change a value; the selected setting's description shows under the list. Enter flips an on/off setting, opens a list of the allowed values for a setting that has them (the current one is marked, and Esc goes back), opens the theme list for `workbench.colorTheme`, and otherwise opens a prompt that shows the allowed range for numbers. Tide checks the value, saves the file and applies it immediately. You can also edit the file by hand: Tide notices within a couple of seconds of saving and applies the changes, and the status bar says which ones wait for new terminals or a restart. If the file no longer parses, Tide lists the problem in **Configuration Problems** and keeps the settings it had until you fix it. Invalid values fall back to the default, and keys Tide does not recognize are kept in the file but ignored.

Most settings apply at once. `terminal.shell` and `tui.terminal.scrollback` apply to terminals opened afterwards. `session.restoreOnStartup`, `workbench.startupEditor`, `update.checkAutomatically`, `tui.outline.collapsedOnStartup` and `tui.timeline.collapsedOnStartup` are read when Tide starts, so they take effect after a restart.

## Icons and colors

`tui.iconStyle` ({{setting:tui.iconStyle}}) decides whether the Explorer and the edge buttons use Nerd Font glyphs. `auto` uses them in Ghostty, kitty and WezTerm, which bundle the glyphs and show them with any font, and plain text in other terminals. Choose `nerd` if your terminal font is a Nerd Font, or `ascii` to never use them. Explorer icons are drawn in the theme's terminal colours by file type, for example orange for Rust, blue for TypeScript and yellow for folders, so they follow the theme you choose.

`tui.colorMode` ({{setting:tui.colorMode}}) decides how colors are sent to the terminal. `auto` sends 24-bit color when the terminal reports it, through `COLORTERM=truecolor` or `24bit`, a `TERM` ending in `-direct`, or a terminal known to support it such as iTerm2, Ghostty, kitty, WezTerm, Alacritty or the VS Code terminal. Otherwise each color becomes the nearest of the 256-color palette, which every supported terminal shows, including Terminal.app before macOS 26. Choose `truecolor` or `256` if `auto` guesses wrong, for example inside tmux or over SSH.

## All settings

{{live}}Values you have changed are shown in bold.

{{/live}}{{settings}}

## Language server overrides

`lsp.servers` is not in the Settings list; add it to {{path:settings}} by hand. Each key is a language ID (`typescript`, `javascript`, `typescriptreact`, `javascriptreact`, `rust`, `python`, `go`, `ruby`, `c`, `cpp`, `json`, `html`, `css` or `sql`). The value is the server to start, `{ "command": "...", "args": [...] }`, or `null` to turn off language-server features for that language:

```jsonc
{
  "lsp.servers": {
    "python": { "command": "pyright-langserver", "args": ["--stdio"] },
    "sql": null
  }
}
```

Languages you leave out use the defaults listed in {{help:editor}}.

## Where Tide keeps your files

| What | Default location |
| --- | --- |
| Settings | {{path:settings}} |
| Keybindings | {{path:keybindings}} |
| User themes | {{path:themes}} |
| Global database connections | {{path:connections}} |
| Encrypted secrets | {{path:secrets}} |
| Query history | {{path:history}} |
| Sessions | {{path:sessions}} |
| Debug log (with `--debug`) | {{path:log}} |
| Cache | {{path:cache}} |
| Project database connections | `<workspace>/.tide/connections.json` |

The configuration, data, state and cache roots follow `$XDG_CONFIG_HOME`, `$XDG_DATA_HOME`, `$XDG_STATE_HOME` and `$XDG_CACHE_HOME` when they are set to absolute paths, and otherwise `~/.config`, `~/.local/share`, `~/.local/state` and `~/.cache`. Tide creates them readable only by you.

## Permissions and removal

Installing, updating and uninstalling Tide leave all of these files in place. `tide --uninstall --purge` removes Tide's own roots after asking; project `.tide` directories and the agents' own session stores are never removed by Tide. See {{help:getting-started}}.
