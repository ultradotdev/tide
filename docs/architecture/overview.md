# Architecture Overview

Tide is a native Rust terminal application in a nine-crate Cargo workspace.

| Crate | Responsibility |
| --- | --- |
| `tide` | CLI entry point, terminal lifecycle, managed installation |
| `tide-ui` | App state, pane tree, dialogs, rendering, mouse routing, session restoration |
| `tide-core` | Text buffer, edits, selections, undo, search, folds and the workspace path boundary |
| `tide-config` | Embedded defaults, JSONC loading, themes, XDG paths and session structures |
| `tide-services` | Files, Git, background syntax highlighting and service operations |
| `tide-terminal` | `portable-pty` processes and `alacritty_terminal` emulation |
| `tide-lsp` | Language-server discovery, JSON-RPC transport and typed requests |
| `tide-db` | PostgreSQL, schema metadata, query safety, result and mutation operations |
| `tide-ai` | External CLI adapters, native session discovery and secret providers |

`App` coordinates input and asynchronous events. Pane and overlay state belongs to the UI; service crates remain independent of it. The main region is a recursive split tree. A shared document can have multiple views while retaining one text revision and save state.

Syntax grammars are shared immutable data. Bounded workers parse document snapshots; completion requests and provider discovery also return background events. The render path consumes ready state. Terminals and external agent tabs use the same native emulator and PTY implementation.

## Runtime authorities

`crates/tide-ui/src/catalog.rs` joins embedded metadata with native entries and alternate bindings. Routed command IDs live beside dispatch, with compatibility aliases in `command_aliases.rs`. `preferences.rs` owns supported setting constraints. The pure reference exporter uses these authorities to generate documentation without launching `App`.

## Storage and safety

XDG paths come from `tide-config`. Session snapshots are versioned, and a save refuses to overwrite a snapshot another instance changed. Secrets use the shared provider service; database connection records refer to them. Native provider session stores remain provider-owned.

PostgreSQL client read-only checks and mutation confirmations complement server-side roles. Managed installation validates ownership, payload hashes and modes before activation. macOS distribution additionally verifies Developer ID and notarization provenance.

The executable embeds configuration, grammars and themes. There is no JavaScript runtime or PTY sidecar in a Rust release payload.
