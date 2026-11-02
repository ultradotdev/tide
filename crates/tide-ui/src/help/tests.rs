use super::*;
use tide_config::{Keybinding, Settings};

fn docs_context<'a>(
    keybindings: &'a [Keybinding],
    commands: &'a std::collections::BTreeMap<String, tide_config::CommandInfo>,
    settings: &'a Settings,
    themes: &'a tide_config::ThemeCatalog,
) -> Context<'a> {
    Context {
        keybindings,
        user_keybindings: &[],
        commands,
        settings,
        themes,
        host: Host::Docs,
    }
}

/// Renders `page` live against `home` with the defaults plus `user` bindings.
fn render_live(page: &str, user: &[Keybinding], install: &Install) -> Rendered {
    let home = std::path::Path::new("/home/someone");
    let paths = tide_config::Paths::from_env(home, |_| None);
    let mut keybindings = default_keybindings();
    keybindings.extend(user.iter().cloned());
    let commands = crate::catalog::commands();
    let settings = crate::catalog::settings();
    let themes = tide_config::builtin::themes();
    let agents = tide_ai::cli::CLI_PROVIDERS
        .into_iter()
        .map(|provider| tide_ai::cli::CliStatus {
            provider,
            installed: provider == tide_ai::cli::CliProvider::Codex,
            executable: (provider == tide_ai::cli::CliProvider::Codex)
                .then(|| home.join(".local/bin/codex")),
        })
        .collect::<Vec<_>>();
    let context = Context {
        keybindings: &keybindings,
        user_keybindings: user,
        commands: &commands,
        settings: &settings,
        themes: &themes,
        host: Host::Live(Live {
            paths: &paths,
            home: Some(home),
            install,
            agents: &agents,
        }),
    };
    render(super::page(page).unwrap(), &context)
}

fn binding(key: &str, command: &str) -> Keybinding {
    Keybinding {
        key: key.into(),
        command: command.into(),
        when: None,
        args: None,
    }
}

#[test]
fn every_page_resolves_every_placeholder_live_and_for_the_docs() {
    let keybindings = default_keybindings();
    let commands = crate::catalog::commands();
    let settings = crate::catalog::settings();
    let themes = tide_config::builtin::themes();
    let docs = docs_context(&keybindings, &commands, &settings, &themes);
    for page in PAGES {
        let installs = [
            Install::Homebrew {
                formula: "tide".into(),
            },
            Install::Managed { local: false },
            Install::Managed { local: true },
            Install::Source {
                executable: "/home/someone/tide/target/release/tide".into(),
            },
            Install::Unknown { executable: None },
        ];
        for install in &installs {
            let live = render_live(page.id, &[], install);
            assert!(live.errors.is_empty(), "{}: {:?}", page.id, live.errors);
            assert!(!live.text.contains("{{"), "{}", page.id);
        }
        if page.doc.is_some() {
            let rendered = render(page, &docs);
            assert!(
                rendered.errors.is_empty(),
                "{}: {:?}",
                page.id,
                rendered.errors
            );
        }
    }
}

#[test]
fn unknown_placeholders_commands_and_settings_are_reported() {
    let page = Page {
        id: "probe",
        command: Command::HelpAbout,
        title: "Probe",
        doc: None,
        template: "{{key:no.such}} {{label:fileTree.moveUp}} {{setting:editor.fontSize}} {{nope}} {{version:1}} {{path:home}} {{help:missing}} {{key:file.save",
    };
    let keybindings = default_keybindings();
    let commands = crate::catalog::commands();
    let settings = crate::catalog::settings();
    let themes = tide_config::builtin::themes();
    let rendered = render(
        &page,
        &docs_context(&keybindings, &commands, &settings, &themes),
    );
    assert_eq!(rendered.errors.len(), 8, "{:?}", rendered.errors);
    assert!(rendered.text.starts_with("{{key:no.such}}"));
}

#[test]
fn every_help_command_is_cataloged_and_opens_a_page_or_a_link() {
    let commands = crate::catalog::commands();
    for command in Command::ALL {
        let id = command.as_str();
        if !id.starts_with("help.") {
            continue;
        }
        let info = commands
            .get(id)
            .unwrap_or_else(|| panic!("{id} is not cataloged"));
        assert_eq!(info.category, "Help");
        assert!(info.label.starts_with("Help: "), "{id}");
        assert!(
            for_command(*command).is_some()
                || matches!(
                    command,
                    Command::HelpDocumentation | Command::HelpReportIssue
                ),
            "{id} opens nothing"
        );
    }
    for page in PAGES {
        assert!(page.command.as_str().starts_with("help."));
        assert_eq!(super::page(page.id).map(|found| found.id), Some(page.id));
    }
}

#[test]
fn keys_render_from_the_live_keymap_and_unbound_commands_point_to_the_palette() {
    let welcome = render_live("welcome", &[], &Install::Managed { local: false }).text;
    assert!(welcome.contains("| Open the command palette | `Ctrl+K` or `Ctrl+Shift+P` |"));
    assert!(
        welcome.contains(
            "Reopen it any time with “Help: Welcome” from the command palette (`Ctrl+K`)."
        )
    );
    assert!(welcome.contains("Open the palette from a terminal with `Ctrl+Shift+P` or `F1`"));
    assert!(welcome.contains("| Show or hide the bottom terminal | `` Ctrl+` `` |"));
    let terminal = render_live("terminal", &[], &Install::Managed { local: false }).text;
    assert!(
        terminal.contains("“Next Bottom Terminal Tab” from the command palette"),
        "a command known by an alternative ID uses that entry's label"
    );
    assert!(terminal.contains("`terminal.shell` (`\"\"`)"));
}

#[test]
fn user_rebindings_show_first_and_removed_keys_disappear() {
    let user = [
        binding("f2", "workbench.commandPalette"),
        binding("ctrl+k", "-workbench.commandPalette"),
        binding("ctrl+alt+w", "help.welcome"),
    ];
    let welcome = render_live("welcome", &user, &Install::Managed { local: false }).text;
    assert!(
        welcome.contains("| Open the command palette | `F2` or `Ctrl+Shift+P` |"),
        "{welcome}"
    );
    assert!(welcome.contains("Reopen it any time with `Ctrl+Alt+W`."));
    let shortcuts = render_live(
        "keyboard-shortcuts",
        &user,
        &Install::Managed { local: false },
    );
    assert!(!shortcuts.text.contains("| `Ctrl+K`"));
}

#[test]
fn live_pages_show_this_machine() {
    let install = Install::Homebrew {
        formula: "tide-nightly".into(),
    };
    let welcome = render_live("welcome", &[], &install).text;
    assert!(welcome.contains("`~/.config/tide/settings.jsonc`"));
    assert!(
        welcome
            .contains("| Codex | `codex` | Installed: `~/.local/bin/codex` | `TIDE_CODEX_PATH` |")
    );
    assert!(welcome.contains("| Claude Code | `claude` | Not found | `TIDE_CLAUDE_PATH` |"));
    assert!(welcome.contains("installed with **Homebrew**"));
    assert!(welcome.contains("`brew upgrade tide-nightly`"));
    assert!(welcome.contains("**Help: Git**"));
    let source = Install::Source {
        executable: "/home/someone/src/tide/target/release/tide".into(),
    };
    let about = render_live("about", &[], &source).text;
    assert!(about.contains("**source build** at `~/src/tide/target/release/tide`"));
    assert!(about.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn static_docs_link_relatively_and_hold_nothing_machine_specific() {
    let docs = static_docs();
    assert_eq!(
        docs.len(),
        PAGES.iter().filter(|page| page.doc.is_some()).count()
    );
    let home = std::env::var("HOME").unwrap_or_default();
    for (path, text) in &docs {
        assert!(!text.contains("{{"), "{path}");
        assert!(home.is_empty() || !text.contains(&home), "{path}");
        assert!(!text.contains("Installed: "), "{path}");
    }
    let welcome = &docs
        .iter()
        .find(|(path, _)| *path == "docs/welcome.md")
        .unwrap()
        .1;
    assert!(welcome.contains("[Git](features/git.md)"));
    assert!(welcome.contains("`~/.config/tide/settings.jsonc`"));
    assert!(welcome.contains("- **Homebrew:**"));
    let shortcuts = &docs
        .iter()
        .find(|(path, _)| path.ends_with("keyboard-shortcuts.md"))
        .unwrap()
        .1;
    assert!(shortcuts.contains("| `Ctrl+K`, `Ctrl+Shift+P`, `Shift+Cmd+P`, `F1`, `Ctrl+T`, `Ctrl+E`, `Cmd+E` | Command Palette | `workbench.commandPalette` |"));
}

#[test]
fn issue_links_carry_the_version_and_platform_but_no_paths() {
    let url = new_issue_url(&Install::Source {
        executable: "/home/someone/private/tide".into(),
    });
    assert!(url.starts_with("https://github.com/ultradotdev/tide/issues/new?body="));
    let decoded = url::Url::parse(&url).unwrap();
    let body = decoded
        .query_pairs()
        .find(|(name, _)| name == "body")
        .unwrap()
        .1
        .into_owned();
    assert!(body.contains(env!("CARGO_PKG_VERSION")));
    assert!(body.contains("a source build"));
    assert!(!body.contains("private"));
}
