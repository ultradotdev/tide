# Getting Started

Tide 0.10.0 runs as a native Rust executable on macOS and Linux. Public binary downloads are not published yet. Work from your existing source checkout.

## Build and run

Install Rust with the toolchain selected by `rust-toolchain.toml`, Git, and a C compiler/linker. macOS needs Xcode Command Line Tools. On Debian/Ubuntu, install the normal build-essential development tools. The locked dependencies compile with Rust 1.97.1.

```sh
cargo build --locked --release -p tide
./target/release/tide .
```

The release profile is the recommended interactive build. Debug builds are useful during development and can be much slower. There is no Bun requirement or external PTY sidecar for running the Rust application.

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

## Earlier installations and TypeScript data

An earlier Rust installer used `~/.local/lib/tide-rust`. Migration is explicit:

```sh
./install.sh --binary "$PWD/target/release/tide" --migrate-legacy
```

The installer validates the recognized earlier launcher and manifest, imports the old active executable as a rollback candidate, and preserves the old store. It refuses to guess ownership of an unrelated `tide` command.

Before switching implementations, back up the Tide configuration, data and state roots listed in the [configuration reference](reference/configuration.md). Rust can read historical TypeScript sessions, but writes its own session files by default. Shared session writes are disabled. Settings and theme files are reused where supported; preserved unsupported settings do not enable a feature.

The frozen TypeScript archive and the independently extracted comparison baseline remain historical references. The active Rust branch and managed launcher are the implementation to test.

## Uninstall

```sh
tide --uninstall
tide --uninstall --purge
# Explicit noninteractive purge:
tide --uninstall --purge --yes
```

Normal removal keeps user data. Purge asks before removal and deletes only Tide's configured user roots. Project `.tide` directories and external providers' native stores remain. Run uninstall through the managed launcher; an arbitrary source executable cannot remove an installation it does not own.

## First use

Open a folder, use `Ctrl+P` for Quick Open and `Ctrl+K` for the command palette. Save with `Ctrl+S`. `F6` cycles workbench regions. Use pane menus to split, add tabs, or collapse surrounding regions. Dialogs wrap their text; ordinary dialogs dismiss on outside click, while an unresolved save/discard decision remains explicit.

See [workbench controls](features/workbench.md), [keyboard shortcuts](reference/keyboard-shortcuts.md) and [troubleshooting](troubleshooting.md).
