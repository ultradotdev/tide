# Tide by Ultra.dev

_v0.10.0_

## Terminal-Native Agentic IDE

**Code, Git, databases, terminal, editing, agent CLIs and more, all in one terminal-native workspace.**

Tide combines a code editor, Git tools, PostgreSQL, shells and external coding agents in a persistent terminal workspace. The application is a native Rust executable, and its maintainer tooling is a Rust `cargo xtask` crate.

This repository is preparing its first public release. The Rust 0.10.0 artifacts and installer are still being validated. Public downloads are unavailable. The commands below work once the release is published on GitHub.

## Install

Prebuilt releases support macOS 14 or later (Apple silicon and Intel) and Linux with glibc 2.35 or later (Ubuntu 22.04 and newer, ARM64 and x64). macOS executables are Developer ID signed and notarized.

### Homebrew (macOS and Linux)

```sh
brew install ultradotdev/tap/tide
brew upgrade tide      # Update
brew uninstall tide    # Remove; settings, sessions and history are kept
```

Homebrew owns that copy: run from it, `tide --uninstall` refuses and names `brew uninstall tide` instead. Use one installation method at a time, so a single `tide` is on your PATH.

### Install script

```sh
curl -fsSL https://github.com/ultradotdev/tide/releases/latest/download/install.sh | sh
```

The script installs the latest GitHub release into `~/.local/lib/tide/releases/` with the launcher `~/.local/bin/tide`. It checks the archive against the release's `SHA256SUMS`, on macOS the Developer ID signature and Apple notarization, and, when an authenticated GitHub CLI (`gh`) is available, the GitHub build attestation. To install a specific version, run that release's script with its version:

```sh
curl -fsSL https://github.com/ultradotdev/tide/releases/download/v0.10.0/install.sh | sh -s -- --version 0.10.0
```

Re-run the script to update; the previous release is retained for `install.sh --rollback`. Remove a script installation with `tide --uninstall` (add `--purge` to also remove Tide user data after confirmation), or `curl -fsSL https://github.com/ultradotdev/tide/releases/latest/download/uninstall.sh | sh`.

### Build from source

Requirements: Git, the pinned Rust toolchain in `rust-toolchain.toml`, a C compiler/linker, and macOS or Linux. On macOS install Xcode Command Line Tools. Linux source builds need the usual C development tools and headers.

From this checkout:

```sh
cargo build --locked --release -p tide
./target/release/tide .
```

Use the optimized release build for everyday use. `cargo run --locked -p tide -- .` runs a debug build, which has substantially more runtime overhead. The executable embeds its configuration catalogs, syntax grammars and themes; no PTY sidecar or Bun runtime is required.

To install a source build through the same managed installer:

```sh
./install-from-source.sh
# Or install an already built, trusted local executable:
./install.sh --binary "$PWD/target/release/tide"
~/.local/bin/tide .
```

Managed installs use `~/.local/lib/tide/releases/` and `~/.local/bin/tide`. Each update retains the previous active build. The installer refuses foreign launchers, changed payloads and concurrent activation. Local installs leave shell startup files unchanged. Add `~/.local/bin` to your PATH if needed.

```sh
./install.sh --rollback
tide --uninstall             # Keeps settings, sessions, history and credentials
tide --uninstall --purge     # Requires confirmation before removing Tide user data
```

See [getting started](docs/getting-started.md) for prefixes, rollback and where Tide keeps your data.

## In the workspace

- **Edit:** syntax highlighting, undo/redo, multicursor editing, folding, find/replace, word wrap, minimap, outline and installed language servers.
- **Arrange:** split main panes, move tabs, resize/collapse sidebars and the terminal dock, scroll overflowing tabs or choose from the tab menu.
- **Navigate:** Explorer file-type icons, folder expansion, proportional scrollbars, Source Control, Timeline and provider accordions.
- **Use Git:** review diffs, stage, unstage, commit, inspect history and run repository actions from the palette. Discard and other destructive operations request confirmation.
- **Query PostgreSQL:** saved connections, schema browsing, SQL completion, history/favorites, sortable results, export and eligible row changes. Results currently occupy the lower part of their SQL tab.
- **Run real tools:** shells and external coding agents share the native PTY and terminal emulator. Their authentication and native history stay provider-owned.
- **Choose a theme:** 13 built-in themes, Ultra Dusk by default, plus user theme files. See [theme attribution](docs/THEME_ATTRIBUTION.md).

| External CLI | Executable |
| --- | --- |
| Claude Code | `claude` |
| Codex | `codex` |
| Gemini CLI | `gemini` |
| Antigravity CLI | `agy` |
| OpenCode | `opencode` |

## Start here

1. Open a repository with `tide .`.
2. Use `Ctrl+P` to find a file and `Ctrl+S` to save it.
3. Open the command palette with `Ctrl+K`. `Ctrl+Shift+P` and `F1` also work, and `Ctrl+T` reaches it while a terminal pane has focus (where `Ctrl+K` belongs to the shell).
4. Use `F6` to cycle regions and `F7` / `F8` to cycle main panes. Split panes and move tabs through their menus or palette commands.
5. Open a shell, a query, or an available agent from Add Tab. The Agents sidebar groups workspace sessions by provider and focuses sessions already open.

The [generated keyboard reference](docs/reference/keyboard-shortcuts.md) lists effective Rust defaults, including context-specific controls and terminal-safe alternatives. The [command reference](docs/reference/commands.md) lists routed commands and compatibility aliases.

## Candidate measurements

The unpublished 0.10.0 candidate was measured with Rust 1.97.1 release builds, a 4,377,826-byte Rust fixture and 100 cursor-movement samples:

| CI runner | First frame | Cursor latency, p95 | Palette opening |
| --- | --- | --- | --- |
| macOS ARM64 | 91.94 ms | 1.20 ms | 1.77 ms |
| Linux x64 | 80.32 ms | 1.39 ms | 1.77 ms |

These are synthetic fixture measurements on shared CI runners. They are not a controlled TypeScript/Rust benchmark or a promise about every machine. Reproduce the workload after building with `cargo xtask measure-rust-performance --binary target/release/tide`.

| Candidate target | Executable | Download archive |
| --- | --- | --- |
| macOS ARM64, signed | 25.92 MiB | 10.02 MiB |
| macOS x64, signed | 27.09 MiB | 10.80 MiB |
| Linux ARM64 | 29.52 MiB | 11.29 MiB |
| Linux x64 | 30.80 MiB | 11.19 MiB |

## Documentation

- [Getting started and installation](docs/getting-started.md)
- [Workbench](docs/features/workbench.md), [editor](docs/features/editor.md), [Git](docs/features/git.md), [terminal](docs/features/terminal.md)
- [PostgreSQL](docs/features/postgresql.md), [external agents](docs/features/ai.md), [themes](docs/features/themes.md)
- [Configuration and storage](docs/reference/configuration.md), [troubleshooting](docs/troubleshooting.md)
- [Architecture](docs/architecture/overview.md), [contributing](CONTRIBUTING.md), [security policy](SECURITY.md)
- [0.10.0 release notes](docs/v0.10.0.md)

## Development

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

Maintainer documentation, release and verification tooling runs through `cargo xtask` (`cargo xtask --help` lists every task). See [CONTRIBUTING.md](CONTRIBUTING.md) for generation and validation.

Tide is licensed under the [Apache License, Version 2.0](LICENSE). See [NOTICE](NOTICE).
