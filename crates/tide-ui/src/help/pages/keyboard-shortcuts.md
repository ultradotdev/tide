# Keyboard Shortcuts

{{live}}These are the keys that work in Tide right now: the built-in bindings, the terminal-safe alternates Tide adds for keys that some terminals cannot send, and your own bindings from {{path:keybindings}}.{{/live}}{{docs}}These are Tide's default keys: the built-in bindings and the terminal-safe alternates Tide adds for keys that some terminals cannot send. Your own bindings go in {{path:keybindings}}; inside Tide, **{{label:help.keyboardShortcuts}}** lists the keys with yours included.{{/docs}} Change a key with {{key:workbench.openKeybindings}}; {{help:themes}} explains the file format.

How a key is resolved:

- A binding with a `when` context, such as `gitPanelFocus`, wins over a global binding for the same key while that context is active.
- For the same key and context, the last entry wins, so your entries replace the defaults.
- A command of `""` or `-<command>` removes a key.
- While a terminal has focus, the shell keeps {{terminalkeys}}. Other workbench keys still reach Tide.

Many terminals cannot distinguish Ctrl+Shift combinations or send Cmd at all, so Tide adds Ctrl+Alt and function-key alternates for many commands. When a key does not arrive, run the command from the palette with {{key:workbench.commandPalette}}. Lists, dialogs, the find bar, diff tabs and the Agents sidebar also have keys of their own, described in each feature guide.

## All keys

{{keybindings}}
