# Tide Documentation

This documentation describes Tide 0.10.0. Public archives are still unpublished.

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
- [0.10.0 release notes](v0.10.0.md)

The guides above, except the release notes, theme authoring and architecture pages, are rendered from the same templates as Tide's in-app Help pages (`crates/tide-ui/src/help/pages`), using the default keybindings and settings. Inside Tide, run any `Help:` command from the command palette to read them with your own keys and settings. Maintainers regenerate them with `cargo xtask generate-rust-reference --write`; CI fails when they are stale.
