<!-- Generated from crates/tide-ui/src/help/pages/agents.md by `cargo xtask generate-rust-reference --write`. Edit the template, not this file. -->
# External AI CLI Agents

Tide runs the coding-agent CLIs you already use, such as Claude Code and Codex, in its own tabs, and lists their saved sessions for this workspace. Each agent tab is a full terminal, so the CLI looks and behaves exactly as it does on its own.

## Supported agents

| Agent | Executable | Path override |
| --- | --- | --- |
| Claude Code | `claude` | `TIDE_CLAUDE_PATH` |
| Codex | `codex` | `TIDE_CODEX_PATH` |
| Gemini CLI | `gemini` | `TIDE_GEMINI_PATH` |
| Antigravity | `agy` | `TIDE_ANTIGRAVITY_PATH` |
| OpenCode | `opencode` | `TIDE_OPENCODE_PATH` |

Tide looks for each executable on the `PATH` it was started with. Set the path override variable to use a copy elsewhere. Install, update and sign in to each CLI outside Tide; authentication stays with the CLI. Run `tide --check-ai-cli` to see what Tide detects and how many sessions each agent has.

## Start a session

1. Press `Ctrl+Shift+I`, or choose AI CLI from Add Tab (`Ctrl+Shift+A`).
2. Pick an installed agent. `ai.cli.defaultProvider` (`claude-code`) is selected first.

The session opens as a tab in the focused pane. With `ai.autoStartCli` (`true`) on, the CLI starts as soon as its tab has focus. Closing a tab stops only that process; the agent's own history is untouched.

## Find and resume sessions

`Ctrl+Shift+J` or `Ctrl+Alt+A` opens a picker of every agent's sessions for this workspace. The Agents sidebar (“Agents: Toggle Sidebar” from the command palette (`Ctrl+K`)) groups the same sessions by agent and refreshes them in the background while it is visible. If one agent fails to list its sessions, the others still load and the failure shows under that agent.

| In the Agents sidebar | Press |
| --- | --- |
| Move | Up and Down, `j` and `k`, Home, End, PageUp, PageDown |
| Open the session, or expand an agent | Enter, `o` or Space |
| Collapse or expand an agent | Left, Right |
| Search titles, session IDs and models | `/` |
| New session | `n` |
| Refresh | `r` |
| Rename the session | `Shift+R` |
| Delete the session (asks first) | `d` |
| Archive the session | `a` |
| Associate an earlier workspace path | `w` |
| Back to the main region | Esc |

A single click selects and a double-click opens. If the session is already open, Tide focuses its tab instead of starting a second copy. The `+` in the sidebar header starts a new session and `▾` moves or hides the sidebar.

Each agent decides which actions it supports: Codex and OpenCode can rename sessions, Codex, Gemini CLI and OpenCode can delete them, and Codex can archive them. Others report that the action is unavailable. Tide never edits an agent's private files to add an action it does not offer.

## Sessions and workspaces

The sidebar shows sessions started in this workspace folder or any folder inside it. After moving or renaming a project folder, run **Agents: Associate Previous Workspace...** (or press `w`) and pick the old path; Tide records it in `ai.cli.workspaceAliases` so those sessions show here again.

Open agent tabs are restored with the rest of the session when the agent is still installed. A tab whose agent has since been removed is skipped with a message in the status bar.

## Troubleshooting

- If an agent is missing, check `command -v <executable>` in the shell you start Tide from, or set its path override variable.
- If its sessions do not list, open a new session to confirm the CLI itself starts and is signed in, then refresh the sidebar with `r`.
- “Not signed in” under an agent means the CLI is installed but asked to be signed in or configured when Tide listed its sessions. Press Enter on the row to see its message, then start a session to sign in. A project with no sessions yet simply shows “No workspace sessions”.
- Tide never copies agent credentials or native session stores into its own files or diagnostics.

See [Terminals](terminal.md) for how agent tabs handle keys and [Troubleshooting](../troubleshooting.md) for more.
