<!-- Generated from crates/tide-ui/src/help/pages/getting-started.md by `cargo xtask generate-rust-reference --write`. Edit the template, not this file. -->
# Getting Started

Tide 0.13.0 runs as a native Rust executable on macOS and Linux. Releases are published on GitHub and install with Homebrew or the install script; you can also build from source.

Prebuilt releases support macOS 14 or later (Apple silicon and Intel) and Linux with glibc 2.35 or later (Ubuntu 22.04 and newer, ARM64 and x64). On older or musl-based Linux distributions such as Alpine, build from source. Windows isn't supported yet; run the Linux build inside WSL.

## Your installation

- **Homebrew:** Update with `brew update && brew upgrade tide` and uninstall with `brew uninstall tide`. Homebrew owns this copy, so `tide --uninstall` refuses and names the `brew` command to use instead.
- **Install script:** Update by running the install command again: `curl -fsSL https://github.com/ultradotdev/tide/releases/latest/download/install.sh | sh`. Uninstall with `tide --uninstall`; add `--purge` to remove your settings and data as well.
- **Managed install from source** (`./install-from-source.sh`): Update by pulling the latest source and running `./install-from-source.sh` again from the checkout; `./install.sh --rollback` returns to the previous build. Uninstall with `tide --uninstall`.
- **Source build** (`./target/release/tide`): Update with `git pull` and then `cargo build --locked --release -p tide` in the checkout. To remove it, delete the build; nothing else was installed.

Updates and uninstalls keep your settings, keybindings, sessions and connections; only `tide --uninstall --purge` removes them.

**Help: Check for Updates** asks GitHub for the latest release and, when it is newer, shows the update step for this installation. Tide also makes the same request at most once a day when it starts, and says in the status bar when a newer release is out. Set `update.checkAutomatically` to `false` to turn that off, or set `TIDE_NO_UPDATE_CHECK=1` in the environment.

## Install with Homebrew

```sh
brew install ultradotdev/tap/tide
```

The formula in the `ultradotdev/homebrew-tap` tap installs the prebuilt release archive for your platform; on macOS that executable is Developer ID signed and notarized. Update with `brew update && brew upgrade tide` and remove with `brew uninstall tide`. Homebrew owns that copy: run from it, `tide --uninstall` and the managed-install commands refuse and name the `brew` command to use instead.

## Install with the install script

```sh
curl -fsSL https://github.com/ultradotdev/tide/releases/latest/download/install.sh | sh
```

`install.sh` is published with every release and covered by its `SHA256SUMS` and GitHub build attestation. Without `--version` it installs GitHub's latest release, found through the `https://github.com/ultradotdev/tide/releases/latest` redirect rather than the rate-limited API. To install a specific version, name it explicitly:

```sh
curl -fsSL https://github.com/ultradotdev/tide/releases/download/v0.13.0/install.sh | sh -s -- --version 0.13.0
```

The script downloads the archive and `SHA256SUMS`, verifies the checksum, and on macOS also verifies the notarization receipt and the executable's Developer ID signature, team and notarization ticket with `codesign`. When an authenticated GitHub CLI is present it verifies the archive's GitHub build attestation; `TIDE_REQUIRE_ATTESTATION=1` makes that check mandatory. It then smoke-tests the executable and activates it in the managed store described below. Add `--add-to-path` to append `~/.local/bin` to your zsh or bash startup file.

Re-run the script to update. Remove the installation with `tide --uninstall`, or without a working launcher:

```sh
curl -fsSL https://github.com/ultradotdev/tide/releases/latest/download/uninstall.sh | sh
```

A piped `uninstall.sh` hands over to the uninstaller bundled with the active release, which verifies ownership before it removes anything. Pass options after `sh -s --`, for example `| sh -s -- --purge --yes`.

## Build and run from source

Install Rust with the toolchain selected by `rust-toolchain.toml`, Git, and a C compiler/linker. macOS needs Xcode Command Line Tools. On Debian/Ubuntu, install the normal build-essential development tools. The locked dependencies compile with Rust 1.97.1.

```sh
cargo build --locked --release -p tide
./target/release/tide .
```

The release profile is the recommended interactive build. Debug builds are useful during development and can be much slower.

To build and install into `~/.cargo/bin` without a checkout, install from the repository. The `tide` crate on crates.io is an unrelated project, and Tide is not published there.

```sh
cargo install --locked --git https://github.com/ultradotdev/tide tide
```

## Managed source or local install

```sh
./install-from-source.sh
# Equivalent after building:
./install.sh --binary "$PWD/target/release/tide"
```

The managed launcher is `~/.local/bin/tide`. Add `~/.local/bin` to PATH yourself if it is absent. Local installs do not edit shell profiles.

A custom absolute prefix puts releases under `<prefix>/lib/tide` and the launcher under `<prefix>/bin`:

```sh
./install.sh --binary "$PWD/target/release/tide" --prefix "$HOME/apps"
```

Install paths must use real directories, without symlink components. On macOS use the canonical `/private/tmp` path rather than `/tmp` for a temporary custom prefix. Build-only source compilation is available with `./install-from-source.sh --build-only`.

## Updates and rollback

Repeat the same local install command with a newly built executable. Local payload identities include the full binary SHA-256. A manifest records the executable, uninstaller and notices; activation verifies their hashes and modes. An interrupted or rejected update leaves the prior active build available.

```sh
./install.sh --rollback
# With the same custom prefix:
./install.sh --prefix "$HOME/apps" --rollback
```

`current` and `previous` point inside the owned release store. Rollback verifies the saved payload before activating it. A foreign launcher or busy installation lock causes a clear refusal.

## Your data

Settings, keybindings, themes, sessions, connections, query history and the encrypted secrets store live in the configuration, data and state directories listed in [Settings Reference](reference/configuration.md): settings in `~/.config/tide/settings.jsonc`, keybindings in `~/.config/tide/keybindings.jsonc` and sessions under `~/.local/state/tide/sessions`. Installation, updates, rollback and uninstall never modify them; only `tide --uninstall --purge` removes them.

## Uninstall

```sh
tide --uninstall
tide --uninstall --purge
# Explicit noninteractive purge:
tide --uninstall --purge --yes
```

Normal removal keeps user data. Purge asks before removal and deletes only Tide's configured user roots. Project `.tide` directories and external providers' native stores remain. Run uninstall through the managed launcher; an arbitrary source executable cannot remove an installation it does not own. For a Homebrew installation use `brew uninstall tide`; Homebrew does not remove Tide's user data roots.

## First steps

1. Start Tide in a project: `tide .` opens the current directory, `tide path/to/file` opens a file in its folder, and `tide` alone opens the directory you are in. The last session for that folder is restored while `session.restoreOnStartup` is on; `--no-session` starts fresh for one run and `--session <name>` opens a named session.
2. Open files with `Ctrl+P` or from the Explorer, and run any command from the palette with `Ctrl+K` or `Ctrl+Shift+P`.
3. Save with `Ctrl+S`. `F6` or `Ctrl+Alt+]` cycles the workbench regions, `Ctrl+\` splits a pane, and `Ctrl+Shift+A` adds a tab.
4. Dialogs wrap their text and their buttons work from the keyboard or the mouse. Clicking outside closes ordinary pickers; confirmations, Save As and forms stay open until you choose.

[Welcome](welcome.md) gives a tour of everything else. See also [Workbench and Panes](features/workbench.md), [Keyboard Shortcuts](reference/keyboard-shortcuts.md) and [Troubleshooting](troubleshooting.md).
