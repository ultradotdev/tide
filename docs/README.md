# Tide Documentation

This documentation describes Tide 0.13.2. Releases are published on [GitHub Releases](https://github.com/ultradotdev/tide/releases).

- [Welcome tour](welcome.md)
- [Getting started and installation](getting-started.md)
- [Workbench and panes](features/workbench.md)
- [Editor and language servers](features/editor.md)
- [Git](features/git.md)
- [Terminals](features/terminal.md)
- [PostgreSQL](features/postgresql.md)
- [External coding agents](features/ai.md)
- [Themes](features/themes.md) and [theme authoring](THEME_AUTHORING.md)
- [Generated commands](reference/commands.md), [keyboard shortcuts](reference/keyboard-shortcuts.md), [configuration](reference/configuration.md)
- [Troubleshooting](troubleshooting.md)
- [Architecture](architecture/overview.md), [keybindings](architecture/keybindings.md), [rendering](architecture/rendering.md)
- Release notes: [0.13.2](v0.13.2.md), [0.13.1](v0.13.1.md), [0.13.0](v0.13.0.md), [0.12.2](v0.12.2.md), [0.12.1](v0.12.1.md), [0.12.0](v0.12.0.md), [0.11.1](v0.11.1.md), [0.11.0](v0.11.0.md), [0.10.0](v0.10.0.md)

The guides above, except the release notes, theme authoring and architecture pages, are rendered from the same templates as Tide's in-app Help pages (`crates/tide-ui/src/help/pages`), using the default keybindings and settings. Inside Tide, run any `Help:` command from the command palette to read them with your own keys and settings. Maintainers regenerate them with `cargo xtask generate-rust-reference --write`; CI fails when they are stale.
