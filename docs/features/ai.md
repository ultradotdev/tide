# External AI CLI Integration

Tide hosts supported coding-agent CLIs in the same pane and terminal system used by files, diffs, SQL, and shells. The integration is provider-neutral at the terminal layer and capability-aware at the session layer.

## Supported providers

| Provider | Executable | Session discovery |
| --- | --- | --- |
| Claude Code | `claude` | Read-only native catalog adapter |
| Codex | `codex` | Local app-server thread API |
| Gemini CLI | `gemini` | Native session-list commands |
| Antigravity CLI | `agy` | Read-only local summary index |
| OpenCode | `opencode` | Native CLI and local session API |

Tide checks exact executables on `PATH`. Available providers appear in Add Tab, provider-specific command-palette entries, the Agents sidebar, and the session picker.

## Start a session

1. Install and authenticate the provider CLI outside Tide.
2. Restart Tide after changing `PATH`.
3. Open Add Tab with `Ctrl+Shift+A` or `Cmd+Shift+A`.
4. Choose an available provider.

`AI CLI: New...` opens a chooser when multiple providers are available and launches directly when only one is available. Provider-specific commands are added dynamically while that executable is available.

## Find and resume sessions

The Agents sidebar groups sessions by provider. Search matches session title, provider, and model. A single click selects a session; double-click or Enter opens it. Provider headers still expand/collapse on one click. If the session is already open anywhere in the workbench, Tide focuses the existing tab instead of creating a duplicate.

Press `Ctrl+Shift+J` or `Cmd+Shift+J` to open the all-provider session picker. Press `Ctrl+Shift+I` or `Cmd+Shift+I` to focus the most recently used open agent tab, or choose a provider when no agent tab is open.

Provider catalogs refresh in the background while visible. A failure in one provider is shown without preventing the other providers from loading.

## Session ownership

- Provider-native IDs remain the source of session identity.
- Closing a tab stops only the hosted PTY. It does not delete or archive native history.
- Rename, delete, and archive actions appear only when the provider exposes a supported operation.
- Tide does not rewrite private provider files to manufacture unsupported capabilities.
- Restored tabs remain identifiable when a provider executable is temporarily unavailable.
- Workspace filtering includes sessions rooted at the current workspace or a descendant directory.

## Terminal behavior

All provider tabs use the same native terminal emulator, viewport renderer, input controller, paste path, mouse reporting, and resize logic as ordinary terminals. Provider adapters can choose executable arguments and discover session identity; they cannot alter parsing, cursor painting, or terminal modes.

Authentication remains owned by each CLI. Tide runs executable and argument arrays without a shell.

## Troubleshooting

- If a provider is missing, confirm its executable name with `command -v <name>` and restart Tide.
- If session listing fails, open a fresh provider tab to confirm the CLI itself starts normally.
- If a restored tab is unavailable, reinstall the same provider. Tide does not silently substitute another CLI.
- Provider credentials and native stores are not copied into Tide fixtures or public diagnostics.
