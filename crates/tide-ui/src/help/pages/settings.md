# Settings and Storage

Tide starts from its built-in defaults and applies your settings file, {{path:settings}}, on top. Open the Settings list with {{key:workbench.openSettings}} to change a value; Tide checks it, saves the file and applies most changes immediately. You can also edit the file by hand and restart Tide. Invalid values fall back to the default, and keys Tide does not recognize are kept in the file but ignored.

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
