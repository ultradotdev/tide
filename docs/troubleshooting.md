# Troubleshooting

## Slow startup or input

Use `cargo build --locked --release -p tide` and run `target/release/tide`. A debug build can be considerably slower, especially during syntax parsing and hashing. First compilation is separate from application startup. Compare release builds with release builds.

If input stalls in a release build, record the terminal emulator, dimensions, file size/type, enabled language servers and a synthetic reproduction. Check whether a provider discovery, search, or language-server operation is active. Do not include provider histories or private files in reports.

## Shortcuts do not arrive

Many terminals cannot distinguish Ctrl+Shift combinations. Use `Ctrl+K` for the palette and consult the [generated shortcuts](reference/keyboard-shortcuts.md) for alternatives. A terminal's own shortcut may intercept a key before Tide receives it.

## Icons or layout look wrong

Use a Nerd Font for file-type glyphs. Set `tui.iconStyle` to `ascii` for a plain-text fallback. Terminal font metrics and Unicode width handling affect alignment. Increase terminal dimensions if a sidebar or dock has collapsed responsively; saved visibility and size are retained.

## Agents are missing

Run `tide --check-ai-cli` and confirm the provider's executable is available in the environment used to launch Tide. Install/authenticate the CLI separately. The Agents sidebar shows sessions belonging to the current workspace or its descendants; use the workspace association action after moving a repository. Expanding a provider or refreshing cannot create sessions that its native catalog does not expose.

## Missing language-server features

Run `tide --check-lsp`. Install the corresponding server executable and put it on PATH. Basic editing and syntax highlighting do not require a language server.

## Installation refuses a path or launcher

The installer refuses symlink directory components, unrelated launchers, changed release contents and a busy lock. Use an absolute canonical prefix. On macOS `/tmp` and `/var` are system symlinks; use `/private/tmp` or the resolved real directory for a temporary prefix. Do not delete a lock until you have confirmed there is no installer running.

For rollback use the same prefix as installation. See [installation](getting-started.md).

## PostgreSQL or credentials

Check the active connection, TLS mode and server permissions. Read-only database roles provide the authorization boundary. Tide's read-only mode adds client checks and a read-only session setting. Keychain availability and encrypted-store unlock state can affect saved credentials. Reports should redact passwords, connection strings and query contents that contain sensitive data.

## Session restoration

Session snapshots live under Tide's state root. To isolate a restoration issue, start with `--no-session`; do not erase the original snapshot to obtain a clean test.

## Diagnostic commands

```sh
tide --version
tide --check-catalogs
tide --check-terminal
tide --check-lsp
tide --check-ai-cli
```

Debug logging is available with `--debug`. Inspect and redact logs before sharing them.
