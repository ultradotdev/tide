use super::*;

#[test]
fn workspace_sessions_are_deduplicated_by_provider_and_canonical_id() {
    let (_home, app) = test_app();
    let child = app.workspace.join("child");
    std::fs::create_dir(&child).unwrap();
    let make = |provider: &str, cwd: &Path| CliSession {
        provider_id: provider.into(),
        session_id: "same-id".into(),
        title: "title".into(),
        cwd: Some(cwd.display().to_string()),
        model: None,
        updated_at: None,
    };
    let mut sessions = vec![
        make("claude-code", &child),
        make("claude-code", &child),
        make("codex", &child),
        make("codex", &app.workspace.with_file_name("outside")),
        make("codex", &app.workspace.join("child/../../outside")),
    ];
    normalize_workspace_sessions(&mut sessions, &app.workspace);
    assert_eq!(sessions.len(), 2);
    assert_ne!(sessions[0].provider_id, sessions[1].provider_id);
}

#[cfg(unix)]
#[test]
fn agents_mouse_collapses_providers_opens_sessions_and_preserves_collapse_state() {
    use std::os::unix::fs::PermissionsExt;
    let (_home, mut app) = test_app();
    let executable = app.workspace.join("fake-agent");
    std::fs::write(&executable, "#!/bin/sh\nexec sleep 30\n").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let provider = CliProvider::ClaudeCode;
    app.ai.cli_status = vec![CliStatus {
        provider,
        installed: true,
        executable: Some(executable),
    }];
    let session = CliSession {
        provider_id: provider.id().into(),
        session_id: "one-session".into(),
        title: "One session".into(),
        cwd: Some(app.workspace.display().to_string()),
        model: None,
        updated_at: None,
    };
    app.handle_background(AppEvent::Ai(AiEvent::Sessions(DiscoveryReport {
        sessions: vec![session.clone(), session],
        ..DiscoveryReport::default()
    })));
    assert_eq!(app.ai.agent_sessions.len(), 1);
    app.ai.agents_visible = true;
    app.focus = Focus::Agents;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 40)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    click_hit(&mut app, |target| {
        *target == HitTarget::AgentProvider(provider)
    });
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(
        !app.layout
            .hits
            .iter()
            .any(|(_, target)| matches!(target, HitTarget::AgentSession(_)))
    );
    assert!(app.with_ai(|ai, ui| ai.selected_session(ui)).is_none());
    click_hit(&mut app, |target| {
        *target == HitTarget::AgentProvider(provider)
    });
    terminal.draw(|frame| app.draw(frame)).unwrap();
    click_hit(&mut app, |target| *target == HitTarget::AgentSession(0));
    assert_eq!(app.focus, Focus::Agents);
    assert_eq!(app.ai.agent_row, 1);
    assert!(
        app.build_session().ai_cli_panes.is_empty(),
        "Single click only selects"
    );
    app.handle_input_key(KeyEvent::new(KeyCode::Null, KeyModifiers::NONE));
    click_hit(&mut app, |target| *target == HitTarget::AgentSession(0));
    assert!(
        app.build_session().ai_cli_panes.is_empty(),
        "Intervening keyboard input breaks a double click"
    );
    app.pointer.agent_click.as_mut().unwrap().4 = Instant::now() - Duration::from_millis(500);
    click_hit(&mut app, |target| *target == HitTarget::AgentSession(0));
    assert!(
        app.build_session().ai_cli_panes.is_empty(),
        "Two slow clicks must not open a session"
    );
    click_hit(&mut app, |target| *target == HitTarget::AgentSession(0));
    assert_eq!(app.build_session().ai_cli_panes.len(), 1);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    click_hit(&mut app, |target| *target == HitTarget::AgentSession(0));
    assert_eq!(app.focus, Focus::Agents);
    click_hit(&mut app, |target| *target == HitTarget::AgentSession(0));
    assert_eq!(app.build_session().ai_cli_panes.len(), 1);
    app.force_close_active();
    app.with_ai(|ai, ui| ai.toggle_provider(provider, ui));
    let saved = app.build_session();
    let (_other_home, mut restored) = test_app();
    restored.restore_session(saved).unwrap();
    assert!(restored.ai.agent_collapsed.contains(provider.id()));
}

#[test]
fn agents_keyboard_follows_visible_rows_and_refresh_preserves_session_identity() {
    let (_home, mut app) = test_app();
    let provider = CliProvider::ClaudeCode;
    app.ai.cli_status = vec![CliStatus {
        provider,
        installed: true,
        executable: None,
    }];
    let make = |id: &str, timestamp: &str| CliSession {
        provider_id: provider.id().into(),
        session_id: id.into(),
        title: id.into(),
        cwd: Some(app.workspace.display().to_string()),
        model: None,
        updated_at: Some(timestamp.into()),
    };
    let a = make("a", "2026-02-01");
    let b = make("b", "2026-01-01");
    app.handle_background(AppEvent::Ai(AiEvent::Sessions(DiscoveryReport {
        sessions: vec![a.clone(), b.clone()],
        ..DiscoveryReport::default()
    })));
    app.focus = Focus::Agents;
    app.with_ai(|ai, ui| {
        ai.handle_agents_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), ui)
    });
    app.with_ai(|ai, ui| {
        ai.handle_agents_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), ui)
    });
    assert_eq!(
        app.with_ai(|ai, ui| ai.selected_session(ui)).unwrap().1,
        "b"
    );
    let mut newer_b = b;
    newer_b.updated_at = Some("2026-03-01".into());
    app.handle_background(AppEvent::Ai(AiEvent::Sessions(DiscoveryReport {
        sessions: vec![a, newer_b],
        ..DiscoveryReport::default()
    })));
    assert_eq!(
        app.with_ai(|ai, ui| ai.selected_session(ui)).unwrap().1,
        "b"
    );
    app.with_ai(|ai, ui| {
        ai.handle_agents_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE), ui)
    });
    assert_eq!(app.ai.rows(), vec![AgentRow::Provider(provider)]);
    assert!(app.with_ai(|ai, ui| ai.selected_session(ui)).is_none());
    app.with_ai(|ai, ui| {
        ai.handle_agents_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), ui)
    });
    app.with_ai(|ai, ui| {
        ai.handle_agents_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), ui)
    });
    assert_eq!(
        app.with_ai(|ai, ui| ai.selected_session(ui)).unwrap().1,
        "b"
    );
}

#[test]
fn provider_refresh_keeps_independent_caches_and_explicit_workspace_associations() {
    let (_home, mut app) = test_app();
    let make = |provider: CliProvider, id: &str, cwd: &Path| CliSession {
        provider_id: provider.id().into(),
        session_id: id.into(),
        title: id.into(),
        cwd: Some(cwd.display().to_string()),
        model: Some("fixture-model".into()),
        updated_at: None,
    };
    let claude = make(CliProvider::ClaudeCode, "claude-one", &app.workspace);
    let old = app.workspace.with_file_name("old-project");
    let codex = make(CliProvider::Codex, "codex-one", &old);
    app.with_ai(|ai, ui| {
        ai.merge_provider(
            ProviderDiscovery {
                provider: CliProvider::ClaudeCode,
                state: DiscoveryState::Ok,
                sessions: vec![claude],
                error: None,
                notice: None,
            },
            ui,
        )
    });
    app.with_ai(|ai, ui| {
        ai.merge_provider(
            ProviderDiscovery {
                provider: CliProvider::Codex,
                state: DiscoveryState::Ok,
                sessions: vec![codex],
                error: None,
                notice: None,
            },
            ui,
        )
    });
    assert_eq!(app.ai.agent_sessions.len(), 1);
    app.with_ai(|ai, ui| {
        ai.merge_provider(
            ProviderDiscovery {
                provider: CliProvider::ClaudeCode,
                state: DiscoveryState::Error,
                sessions: Vec::new(),
                error: Some("fixture timeout".into()),
                notice: None,
            },
            ui,
        )
    });
    assert_eq!(app.ai.agent_sessions.len(), 1);
    app.associate_agent_workspace(old.to_str().unwrap());
    assert_eq!(app.ai.agent_sessions.len(), 2);
    assert!(app.paths.settings_file.is_file());
    app.ai.agent_query = "codex".into();
    let matches = app
        .ai
        .rows()
        .into_iter()
        .filter_map(|row| match row {
            AgentRow::Session(index) => Some(&app.ai.agent_sessions[index]),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].session_id, "codex-one");
    app.with_ai(|ai, ui| {
        ai.merge_provider(
            ProviderDiscovery {
                provider: CliProvider::Codex,
                state: DiscoveryState::Ok,
                sessions: Vec::new(),
                error: None,
                notice: None,
            },
            ui,
        )
    });
    assert_eq!(app.ai.agent_sessions.len(), 1);
    assert_eq!(app.ai.agent_sessions[0].session_id, "claude-one");
}

#[test]
fn agents_search_paste_never_edits_the_underlying_buffer_and_picker_refreshes() {
    let (_home, mut app) = test_app();
    app.new_file();
    app.focus = Focus::Agents;
    app.ai.agent_search = true;
    app.handle_paste("search 👩‍💻");
    assert_eq!(app.workbench.active_editor().unwrap().document.text(), "");
    assert_eq!(app.ai.agent_query, "search 👩‍💻");
    app.with_ai(|ai, ui| {
        ai.handle_agents_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE), ui)
    });
    assert_eq!(app.ai.agent_query, "search ");
    app.with_ai(|ai, ui| ai.open_session_picker(ui));
    assert_eq!(app.overlay.as_ref().unwrap().base().items.len(), 0);
    let session = CliSession {
        provider_id: "codex".into(),
        session_id: "fresh-native".into(),
        title: "Fresh native".into(),
        cwd: Some(app.workspace.display().to_string()),
        model: None,
        updated_at: None,
    };
    app.with_ai(|ai, ui| {
        ai.merge_provider(
            ProviderDiscovery {
                provider: CliProvider::Codex,
                state: DiscoveryState::Ok,
                sessions: vec![session],
                error: None,
                notice: None,
            },
            ui,
        )
    });
    assert_eq!(app.overlay.as_ref().unwrap().base().items.len(), 1);
    assert_eq!(
        app.overlay.as_ref().unwrap().base().items[0].value,
        "codex\tfresh-native"
    );
}

#[cfg(unix)]
#[test]
fn fake_cli_providers_launch_dedupe_restore_and_close_without_duplicate_tabs() {
    use std::os::unix::fs::PermissionsExt;
    let (_home, mut app) = test_app();
    let executable = app.workspace.join("fake-agent");
    std::fs::write(
        &executable,
        "#!/bin/sh\nprintf agent-ready\nexec sleep 30\n",
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    for provider in ai_cli::CLI_PROVIDERS {
        app.ai.cli_status = vec![CliStatus {
            provider,
            installed: true,
            executable: Some(executable.clone()),
        }];
        app.with_ai(|ai, ui| ai.launch_cli(provider, Some("review-session"), ui));
        app.with_ai(|ai, ui| ai.launch_cli(provider, Some("review-session"), ui));
        let session = app.build_session();
        assert_eq!(session.ai_cli_panes.len(), 1);
        app.force_close_active();
        temp_env::with_var(
            provider.env_override(),
            Some(executable.as_os_str()),
            || app.restore_session(session).unwrap(),
        );
        assert_eq!(app.build_session().ai_cli_panes.len(), 1);
        app.force_close_active();
        assert!(app.build_session().ai_cli_panes.is_empty());
    }
}

#[test]
fn agents_sidebar_is_a_directional_focus_region() {
    let (_home, mut app) = test_app();
    app.ai.agents_visible = true;
    let backend = ratatui::backend::TestBackend::new(160, 40);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.focus = Focus::Main;
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
    assert_eq!(app.focus, Focus::Agents);
    app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT));
    assert_eq!(app.focus, Focus::Main);
}

#[test]
fn session_state_records_the_agents_region() {
    let (_home, mut app) = test_app();
    app.ai.agents_visible = true;
    let session = app.build_session();
    assert!(session.ui.agents_sidebar_visible);
}

#[test]
fn a_cli_that_needs_signing_in_shows_a_calm_status_instead_of_an_error() {
    let (_home, mut app) = test_app();
    app.with_ai(|ai, ui| {
        ai.cli_status = vec![tide_ai::cli::CliStatus {
            provider: CliProvider::Gemini,
            installed: true,
            executable: Some(PathBuf::from("/usr/local/bin/gemini")),
        }];
        ai.merge_provider(
            ProviderDiscovery {
                provider: CliProvider::Gemini,
                state: DiscoveryState::Ok,
                sessions: Vec::new(),
                error: None,
                notice: Some("Please set an Auth method".into()),
            },
            ui,
        )
    });
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 40)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(screen.contains("Not signed in"), "{screen}");
    assert!(!screen.contains("Gemini CLI (0) !"), "no error marker");
    app.with_ai(|ai, ui| ai.show_status(CliProvider::Gemini, ui));
    let message = app
        .overlay
        .as_ref()
        .unwrap()
        .base()
        .message
        .clone()
        .unwrap();
    assert!(message.contains("needs signing in"), "{message}");
    assert!(message.contains("Please set an Auth method"), "{message}");
}

#[test]
fn an_ineligible_account_is_not_shown_as_signed_out() {
    let (_home, mut app) = test_app();
    let notice = "Error authenticating: IneligibleTierError: this client is not supported";
    app.with_ai(|ai, ui| {
        ai.cli_status = vec![tide_ai::cli::CliStatus {
            provider: CliProvider::Gemini,
            installed: true,
            executable: Some(PathBuf::from("/usr/local/bin/gemini")),
        }];
        ai.merge_provider(
            ProviderDiscovery {
                provider: CliProvider::Gemini,
                state: DiscoveryState::Ok,
                sessions: Vec::new(),
                error: None,
                notice: Some(notice.into()),
            },
            ui,
        )
    });
    app.ai.agents_visible = true;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 40)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(screen.contains("Account not eligible"), "{screen}");
    assert!(!screen.contains("Not signed in"), "{screen}");
    app.with_ai(|ai, ui| ai.show_status(CliProvider::Gemini, ui));
    let message = app
        .overlay
        .as_ref()
        .unwrap()
        .base()
        .message
        .clone()
        .unwrap();
    assert!(message.contains("account can't use it"), "{message}");
    assert!(message.contains("IneligibleTierError"), "{message}");
}
