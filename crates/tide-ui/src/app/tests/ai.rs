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
        app.session_baseline = Some(session.clone());
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
fn excluded_chat_is_disabled_and_hidden_from_the_palette() {
    let (_home, mut app) = test_app();
    app.execute_command(Command::AiChatNew);
    assert!(!app.workbench.has_ai_chat());
    assert!(app.status.contains("not included"));
    app.open_command_palette();
    assert!(
        app.overlay
            .as_ref()
            .unwrap()
            .base()
            .items
            .iter()
            .all(|item| !item.value.starts_with("aiChat.") && !item.value.starts_with("ensemble."))
    );
}

#[test]
fn ctrl_alt_i_opens_ai_chat_through_the_keymap() {
    let (_home, mut app) = test_app();
    app.ai.chat_enabled = true;
    app.handle_key(KeyEvent::new(
        KeyCode::Char('i'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert!(app.workbench.has_ai_chat());
    assert_eq!(app.focus, Focus::Main);
}

#[test]
fn ai_chat_tab_opens_and_streams_a_transcript() {
    let (_home, mut app) = test_app();
    app.ai.chat_enabled = true;
    app.with_ai(|ai, ui| ai.open_chat_tab(true, ui));
    assert_eq!(app.focus, Focus::Main);
    assert!(app.workbench.has_ai_chat());
    let backend = ratatui::backend::TestBackend::new(100, 40);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();

    app.ai.chat = ChatSession::new("openai", "gpt-4o");
    app.ai.chat.push_user_text("hello");
    app.ai.chat_turn = Some(TurnAccumulator::new());
    app.ai.chat_streaming = true;
    let generation = app.ai.chat_generation;
    app.handle_background(AppEvent::Ai(AiEvent::Stream {
        generation,
        event: StreamEvent::TextDelta {
            text: "hi there".to_owned(),
        },
    }));
    app.handle_background(AppEvent::Ai(AiEvent::Finished { generation }));
    assert!(!app.ai.chat_streaming);
    assert_eq!(app.ai.chat.messages.len(), 2);
    assert_eq!(app.ai.chat.messages[1].text(), "hi there");
    assert!(app.ai.chat.interrupted.is_none());
    assert!(app.ai.chat_store.load(&app.ai.chat.id).is_ok());
    terminal.draw(|frame| app.draw(frame)).unwrap();
}

#[test]
fn ai_chat_cancel_records_cancellation() {
    let (_home, mut app) = test_app();
    app.ai.chat_enabled = true;
    app.with_ai(|ai, ui| ai.open_chat_tab(true, ui));
    app.ai.chat.push_user_text("hello");
    app.ai.chat_turn = Some(TurnAccumulator::new());
    app.ai.chat_streaming = true;
    app.with_ai(|ai, ui| ai.cancel_chat(ui));
    assert!(!app.ai.chat_streaming);
    let last = app.ai.chat.messages.last().unwrap();
    assert!(last.blocks.iter().any(|block| matches!(
        block,
        tide_ai::chat::ContentBlock::Error { message } if message == "cancelled"
    )));
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_chat_kills_a_running_validation_command() {
    let (_home, mut app) = test_app_async();
    app.settings
        .set("ai.allowValidationCommands", serde_json::json!(true));
    let pid_file = app.workspace.join("validation.pid");
    let input = serde_json::json!({
        "program": "sh",
        "args": ["-c", "echo $$ > validation.pid; exec sleep 30"],
    });
    app.ai.chat.messages.push(ChatMessage::new(
        Role::Assistant,
        vec![ContentBlock::ToolUse {
            id: "call-1".to_owned(),
            name: "run_validation".to_owned(),
            input_json: input.to_string(),
        }],
    ));
    app.with_ai(|ai, ui| ai.handle_tool_calls(ui));
    let started = Instant::now();
    let pid = loop {
        if let Some(pid) = std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|text| text.trim().parse::<i32>().ok())
        {
            break nix::unistd::Pid::from_raw(pid);
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "validation never started"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    app.with_ai(|ai, ui| ai.cancel_chat(ui));
    let cancelled = Instant::now();
    let mut alive = true;
    while alive && cancelled.elapsed() < Duration::from_secs(5) {
        tokio::time::sleep(Duration::from_millis(20)).await;
        alive = nix::sys::signal::kill(pid, None).is_ok();
    }
    if alive {
        let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGKILL);
    }
    assert!(!alive, "the validation command survived cancellation");
    assert_eq!(app.status, "AI: turn cancelled");
}

#[test]
fn selecting_a_model_updates_the_chat_and_preferences() {
    let (_home, mut app) = test_app();
    app.ai.chat_enabled = true;
    app.with_ai(|ai, ui| ai.select_model("gpt-4o", ui));
    assert_eq!(app.ai.chat.provider, "openai");
    assert_eq!(app.ai.chat.model, "gpt-4o");
    let preferences = ModelPreferences::load(&app.paths.model_preferences_file);
    assert_eq!(preferences.chat.as_deref(), Some("gpt-4o"));
}

#[test]
fn sending_without_an_api_key_keeps_the_draft() {
    temp_env::with_vars(
        [
            ("OPENAI_API_KEY", None::<&str>),
            ("TIDE_SECRET_AI_OPENAI_API_KEY", None::<&str>),
        ],
        || {
            let (_home, mut app) = test_app();
            app.ai.chat_enabled = true;
            app.with_ai(|ai, ui| ai.open_chat_tab(true, ui));
            app.ai.chat.provider = "openai".to_owned();
            app.ai.chat.model = "gpt-4o".to_owned();
            app.ai.chat_draft = "hello".to_owned();
            app.with_ai(|ai, ui| ai.send_chat_message(ui));
            assert_eq!(app.ai.chat_draft, "hello");
            assert!(app.status.contains("no API key"));
            assert!(!app.ai.chat_streaming);
        },
    );
}

#[test]
fn an_ollama_chat_starts_without_an_api_key() {
    // A current-thread runtime that is never driven: the turn is spawned
    // but no request reaches a local Ollama server.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _context = runtime.enter();
    let (_home, mut app) = test_app();
    app.ai.chat_enabled = true;
    app.with_ai(|ai, ui| ai.open_chat_tab(true, ui));
    app.ai.chat.provider = "ollama".to_owned();
    app.ai.chat.model = "llama3.2".to_owned();
    app.ai.chat_draft = "hello".to_owned();
    app.with_ai(|ai, ui| ai.send_chat_message(ui));
    assert!(!app.status.contains("no API key"), "{}", app.status);
    assert!(app.ai.chat_streaming);
    assert!(app.ai.chat_draft.is_empty());
    assert_eq!(app.ai.chat.messages.len(), 1);
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
fn disabled_ai_chat_restore_preserves_data_without_opening_a_tab() {
    let (_home, mut app) = test_app();
    app.ai.chat_enabled = true;
    app.with_ai(|ai, ui| ai.open_chat_tab(true, ui));
    let session = app.build_session();
    assert_eq!(session.ai_chat_panes.len(), 1);

    let (_home, mut disabled) = test_app();
    disabled.session_baseline = Some(session.clone());
    disabled.restore_session(session).unwrap();
    assert!(!disabled.workbench.has_ai_chat());
    let rebuilt = disabled.build_session();
    assert_eq!(rebuilt.ai_chat_panes.len(), 1);
}

#[test]
fn session_state_records_ai_chat_and_agents_regions() {
    let (_home, mut app) = test_app();
    app.ai.chat_enabled = true;
    app.with_ai(|ai, ui| ai.open_chat_tab(true, ui));
    app.ai.agents_visible = true;
    let session = app.build_session();
    assert_eq!(session.ai_chat_panes.len(), 1);
    assert_eq!(
        session.ai_chat_panes[0].conversation_id.as_deref(),
        Some(app.ai.chat.id.as_str())
    );
    assert!(session.ui.agents_sidebar_visible);
    assert!(session.ui.ai_panel_visible);
}

#[test]
fn approving_an_ai_change_set_applies_it_and_records_tool_results() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("ai-change.txt");
    std::fs::write(&path, "old\n").unwrap();
    let workspace = ToolWorkspace::new(&app.workspace).unwrap();
    let change =
        tide_tools::FileChange::write(&workspace, "ai-change.txt", "new\n", &MapBuffers::new())
            .unwrap();
    app.ai.pending_change_set = Some(ChangeSet::new(vec![change]));
    app.ai.pending_tool_results = vec![ChatToolResult {
        tool_use_id: "call-1".to_owned(),
        content: "Pending approval for write_file ai-change.txt".to_owned(),
        is_error: false,
    }];
    app.with_ai(|ai, ui| ai.resolve_pending_changes(true, ui));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
    let last = app.ai.chat.messages.last().unwrap();
    assert!(matches!(
        &last.blocks[0],
        ContentBlock::ToolResult { content, is_error: false, .. } if content.contains("Applied changes")
    ));
}

#[test]
fn approving_a_change_set_for_a_vanished_workspace_never_writes_elsewhere() {
    let (_home, mut app) = test_app();
    let name = "tide-vanished-workspace-change.txt";
    let workspace = ToolWorkspace::new(&app.workspace).unwrap();
    let change =
        tide_tools::FileChange::write(&workspace, name, "new\n", &MapBuffers::new()).unwrap();
    app.ai.pending_change_set = Some(ChangeSet::new(vec![change]));
    app.ai.pending_tool_results = vec![ChatToolResult {
        tool_use_id: "call-1".to_owned(),
        content: format!("Pending approval for write_file {name}"),
        is_error: false,
    }];
    std::fs::remove_dir_all(&app.workspace).unwrap();
    app.with_ai(|ai, ui| ai.resolve_pending_changes(true, ui));
    let stray = std::env::current_dir().unwrap().join(name);
    let written = stray.exists();
    let _ = std::fs::remove_file(&stray);
    assert!(
        !written,
        "the change set was applied to the current directory"
    );
    assert!(
        app.status_history
            .iter()
            .any(|status| status.contains("change set failed")),
        "{:?}",
        app.status_history
    );
    let last = app.ai.chat.messages.last().unwrap();
    assert!(matches!(
        &last.blocks[0],
        ContentBlock::ToolResult { is_error: true, .. }
    ));
}

#[test]
fn approving_a_reads_only_change_set_reports_the_reads() {
    let (_home, mut app) = test_app();
    std::fs::write(app.workspace.join(".env"), "KEY=1\n").unwrap();
    app.ai.pending_change_set = Some(ChangeSet::sensitive_read(".env".to_owned()));
    app.ai.pending_tool_results = vec![ChatToolResult {
        tool_use_id: "call-1".to_owned(),
        content: format!("{}.env", tide_tools::changeset::PENDING_READ_PREFIX),
        is_error: false,
    }];
    app.with_ai(|ai, ui| ai.resolve_pending_changes(true, ui));
    assert!(
        app.status_history
            .contains(&"AI: shared 1 approved read".to_owned()),
        "{:?}",
        app.status_history
    );
}

#[test]
fn rejecting_an_ai_change_set_leaves_files_untouched() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("ai-change.txt");
    std::fs::write(&path, "old\n").unwrap();
    let workspace = ToolWorkspace::new(&app.workspace).unwrap();
    let change =
        tide_tools::FileChange::write(&workspace, "ai-change.txt", "new\n", &MapBuffers::new())
            .unwrap();
    app.ai.pending_change_set = Some(ChangeSet::new(vec![change]));
    app.ai.pending_tool_results = vec![ChatToolResult {
        tool_use_id: "call-1".to_owned(),
        content: "Pending approval for write_file ai-change.txt".to_owned(),
        is_error: false,
    }];
    app.with_ai(|ai, ui| ai.resolve_pending_changes(false, ui));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "old\n");
    let last = app.ai.chat.messages.last().unwrap();
    assert!(matches!(
        &last.blocks[0],
        ContentBlock::ToolResult { content, is_error: true, .. } if content.contains("rejected")
    ));
}
