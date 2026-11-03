use super::*;

#[test]
fn csv_export_confirms_overwrites_and_reports_directory_errors() {
    let (_home, mut app) = test_app();
    app.with_db(|db, ui| db.open_tab(Some("select 1"), ui));
    let result: QueryResult = serde_json::from_value(serde_json::json!({"queryId":"q","connectionId":"local","sql":"select 1","rows":[{"n":"1"}],"fields":[{"name":"n","dataType":"int4","dataTypeId":23}],"rowCount":1,"startedAt":"","completedAt":"","durationMs":1})).unwrap();
    if let Some(Tab::Sql(tab)) = app.workbench.active_tab_mut() {
        tab.set_result(result);
    }
    let existing = app.workspace.join("results.csv");
    std::fs::write(&existing, "keep").unwrap();
    app.with_db(|db, ui| db.export_results("results.csv", ui));
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "keep");
    assert_eq!(
        app.overlay.as_ref().map(|overlay| overlay.kind().clone()),
        Some(ModalKind::Confirm(ConfirmAction::ExportResults(
            existing.clone()
        )))
    );
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "n\n1\n");
    assert!(app.status.contains("exported 1 rows"), "{}", app.status);

    std::fs::write(app.workspace.join("blocker"), "").unwrap();
    app.with_db(|db, ui| db.export_results("blocker/out.csv", ui));
    assert!(app.status.contains("export failed"), "{}", app.status);
    assert!(app.overlay.is_none());
}

#[test]
fn editing_a_connection_preserves_tls_and_timeouts() {
    let (_home, mut app) = test_app();
    let mut connection = ConnectionConfig {
        id: "secure".into(),
        name: "Secure".into(),
        host: "db.example".into(),
        database: "app".into(),
        username: "alice".into(),
        ssl: Some(serde_json::json!({"ca":"/certificate.pem","rejectUnauthorized":true})),
        connection_timeout: Some(5000),
        query_timeout: Some(9000),
        ..ConnectionConfig::default()
    };
    app.db.store.add(connection.clone()).unwrap();
    let dsn = DbController::connection_dsn(&connection);
    app.with_db(|db, ui| db.edit_connection("secure", &dsn, ui));
    assert_eq!(app.db.store.get("secure"), Some(&connection));
    connection.host = "next.example".into();
    app.with_db(|db, ui| {
        db.edit_connection("secure", &DbController::connection_dsn(&connection), ui)
    });
    assert_eq!(app.db.store.get("secure"), Some(&connection));
}

#[test]
fn f5_in_the_sql_editor_executes_instead_of_refreshing_results() {
    let (_home, mut app) = test_app();
    app.with_db(|db, ui| db.open_tab(Some("SELECT 1;"), ui));
    app.handle_key(KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE));
    assert!(app.status.contains("connection"));
}

#[test]
fn sql_tabs_open_edit_and_classify_risky_statements() {
    let (_home, mut app) = test_app();
    app.execute_command(Command::DatabaseNewQuery);
    assert!(matches!(app.workbench.active_tab(), Some(Tab::Sql(_))));
    if let Some(Tab::Sql(sql)) = app.workbench.active_tab_mut() {
        sql.set_sql("DELETE FROM users");
    }
    // No connection yet: the query is not sent anywhere.
    app.with_db(|db, ui| db.execute(false, ui));
    assert!(app.status.contains("connection"));
    assert!(!matches!(
        app.overlay.as_ref().map(|overlay| overlay.kind().clone()),
        Some(ModalKind::Confirm(ConfirmAction::RunSql(_)))
    ));
    app.overlay = None;

    let mut config = tide_db::ConnectionConfig {
        id: "test-db".to_owned(),
        name: "Test".to_owned(),
        host: "127.0.0.1".to_owned(),
        database: "postgres".to_owned(),
        username: "postgres".to_owned(),
        read_only: false,
        ..tide_db::ConnectionConfig::default()
    };
    config.kind = "postgres".to_owned();
    app.db.store.add(config).unwrap();
    app.with_db(|db, ui| db.select_connection("test-db", ui));
    app.with_db(|db, ui| db.execute(false, ui));
    assert!(matches!(
        app.overlay.as_ref().map(|overlay| overlay.kind().clone()),
        Some(ModalKind::Confirm(ConfirmAction::RunSql(_)))
    ));
}

#[test]
fn read_only_connections_refuse_mutations_before_connecting() {
    let (_home, mut app) = test_app();
    app.execute_command(Command::DatabaseNewQuery);
    let mut config = tide_db::ConnectionConfig {
        id: "readonly".to_owned(),
        name: "Read-only".to_owned(),
        host: "127.0.0.1".to_owned(),
        database: "postgres".to_owned(),
        username: "postgres".to_owned(),
        read_only: true,
        ..tide_db::ConnectionConfig::default()
    };
    config.kind = "postgres".to_owned();
    app.db.store.add(config).unwrap();
    app.with_db(|db, ui| db.select_connection("readonly", ui));
    if let Some(Tab::Sql(sql)) = app.workbench.active_tab_mut() {
        sql.set_sql("DELETE FROM users");
    }
    app.with_db(|db, ui| db.execute(false, ui));
    assert!(app.status.contains("read-only"));
    assert!(app.overlay.is_none());
}

#[test]
fn sql_tabs_persist_in_the_session() {
    let (_home, mut app) = test_app();
    app.with_db(|db, ui| db.open_tab(Some("SELECT 42;"), ui));
    let session = app.build_session();
    assert_eq!(session.sql_editors.len(), 1);
    assert_eq!(session.sql_editors[0].content, "SELECT 42;");
    app.close_all();
    app.restore_session(session).unwrap();
    let restored = app.workbench.pane_ids().into_iter().find_map(|id| {
        app.workbench.pane(id).and_then(|pane| {
            pane.tabs.iter().find_map(|tab| match tab {
                Tab::Sql(sql) => Some(sql.sql()),
                _ => None,
            })
        })
    });
    assert_eq!(restored.as_deref(), Some("SELECT 42;"));
}

#[test]
fn database_tls_fallback_warnings_reach_the_status_line_and_results() {
    let (_home, mut app) = test_app();
    let warning = "sslmode=prefer fell back to an unencrypted connection";
    app.handle_background(AppEvent::Db(DbEvent::Tested {
        name: "local".to_owned(),
        success: true,
        error: None,
        latency_ms: 3,
        server_version: Some("16".to_owned()),
        warning: Some(warning.to_owned()),
    }));
    assert!(app.status.contains(warning), "{}", app.status);

    app.with_db(|db, ui| db.open_tab(Some("select 1"), ui));
    let generation = match app.workbench.active_tab() {
        Some(Tab::Sql(tab)) => tab.generation,
        _ => unreachable!(),
    };
    let result: QueryResult = serde_json::from_value(serde_json::json!({"queryId":"q","connectionId":"local","sql":"select 1","rows":[{"n":"1"}],"fields":[{"name":"n","dataType":"int4","dataTypeId":23}],"rowCount":1,"startedAt":"","completedAt":"","durationMs":1,"notices":[warning]})).unwrap();
    app.handle_background(AppEvent::Db(DbEvent::Completed {
        generation,
        connection_id: "local".to_owned(),
        result: Box::new(result),
    }));
    assert!(app.status.contains(warning), "{}", app.status);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(200, 40)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(crate::test_support::screen_text(&terminal).contains(warning));
}

#[test]
fn structured_connections_keep_credentials_only_at_the_approved_destination() {
    let (_home, mut app) = test_app();
    let config = ConnectionConfig {
        id: "owned".into(),
        name: "Original".into(),
        host: "db.example.test".into(),
        database: "app".into(),
        username: "alice".into(),
        password_secret: Some("approved-secret".into()),
        ssl: Some(true.into()),
        connection_timeout: Some(12345),
        query_timeout: Some(6789),
        ..Default::default()
    };
    app.db.store.add(config.clone()).unwrap();
    let mut renamed = config.clone();
    renamed.name = "Renamed".into();
    app.with_db(|db, ui| {
        db.submit_connection_form(
            crate::connection_form::Submission {
                config: renamed.clone(),
                password: None,
                action: ConnectionAction::Save,
            },
            ui,
        )
    });
    assert_eq!(
        app.db.store.get("owned").unwrap().password_secret,
        config.password_secret
    );
    renamed.host = "different.example.test".into();
    app.with_db(|db, ui| {
        db.submit_connection_form(
            crate::connection_form::Submission {
                config: renamed,
                password: None,
                action: ConnectionAction::Save,
            },
            ui,
        )
    });
    let saved = app.db.store.get("owned").unwrap();
    assert!(saved.password_secret.is_none());
    assert_eq!(saved.connection_timeout, Some(12345));
    assert_eq!(saved.query_timeout, Some(6789));
    assert_eq!(saved.ssl, Some(true.into()));
    app.execute_command(Command::DatabaseNewConnection);
    assert_eq!(
        *app.overlay.as_ref().unwrap().kind(),
        ModalKind::ConnectionForm
    );
}

#[test]
fn history_opens_independent_connected_copies_and_clear_is_scoped_and_safe() {
    let (_home, mut app) = test_app();
    app.with_db(|db, ui| db.open_tab(Some("original buffer"), ui));
    let first = app
        .db
        .history
        .add_entry(
            "one",
            "First",
            "SELECT 1;\nSELECT 2;",
            1,
            2,
            QueryStatus::Success,
            None,
        )
        .unwrap()
        .unwrap();
    app.db
        .history
        .add_entry(
            "two",
            "Second",
            "SELECT 3;",
            1,
            1,
            QueryStatus::Success,
            None,
        )
        .unwrap();
    app.db.history.set_favorite(&first.id, true).unwrap();
    app.with_db(|db, ui| db.load_history_entry(&first.id, ui));
    app.with_db(|db, ui| db.load_history_entry(&first.id, ui));
    let sqls = app
        .workbench
        .pane_ids()
        .iter()
        .flat_map(|id| app.workbench.pane(*id).unwrap().tabs.iter())
        .filter_map(|tab| {
            if let Tab::Sql(sql) = tab {
                Some(sql)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(sqls.len(), 3);
    assert_eq!(sqls[0].sql(), "original buffer");
    assert_eq!(sqls[1].sql(), first.sql);
    assert_eq!(sqls[2].connection_id.as_deref(), Some("one"));
    assert!(sqls.iter().all(|sql| !sql.running));
    app.with_db(|db, ui| db.open_history(ui));
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::ALT));
    assert_eq!(app.overlay.as_ref().unwrap().base().items.len(), 1);
    app.db.history_connection = Some("one".into());
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT));
    app.handle_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.db.history.entries().unwrap().len(), 2);
    app.with_db(|db, ui| db.open_history(ui));
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT));
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    assert_eq!(app.db.history.entries().unwrap().len(), 1);
    assert_eq!(app.db.history.entries().unwrap()[0].connection_id, "two");
}

#[test]
fn project_connections_keep_their_password_in_the_user_store_not_the_project() {
    let (home, mut app) = test_app();
    let secrets = Arc::new(SecretService::encrypted_only(
        home.path().join("fixture-secrets.json"),
    ));
    secrets.unlock("master-fixture-password").unwrap();
    app.db.secrets = secrets;
    app.db.pgpass = None;
    let config = ConnectionConfig {
        name: "Team database".into(),
        host: "db.example.test".into(),
        database: "app".into(),
        username: "alice".into(),
        scope: tide_db::ConnectionScope::Project,
        ..Default::default()
    };
    let submit = |app: &mut App, config: ConnectionConfig, password: &str| {
        app.with_db(|db, ui| {
            db.submit_connection_form(
                crate::connection_form::Submission {
                    config,
                    password: Some(password.into()),
                    action: ConnectionAction::Save,
                },
                ui,
            )
        });
    };
    submit(&mut app, config, "first-fixture-password");
    assert!(app.status.contains("saved"), "{}", app.status);
    let project_file = app.paths.workspace_connections_file(&app.workspace);
    let shared = std::fs::read_to_string(&project_file).unwrap();
    assert!(shared.contains("Team database"));
    assert!(!shared.contains("fixture-password") && !shared.contains("passwordSecret"));
    let saved = app.db.store.connections()[0].clone();
    assert_eq!(
        app.db.password(&saved).as_deref(),
        Some("first-fixture-password")
    );

    // A new password replaces the old one, whose secret is removed.
    let old = app.db.store.project_credential(&saved).unwrap().to_owned();
    submit(&mut app, saved.clone(), "second-fixture-password");
    assert_eq!(
        app.db.password(&saved).as_deref(),
        Some("second-fixture-password")
    );
    assert_eq!(app.db.secrets.get(&old), None);
    assert!(
        !std::fs::read_to_string(&app.paths.connections_file)
            .unwrap()
            .contains("fixture-password")
    );
}

#[cfg(unix)]
#[test]
fn a_libpq_password_file_supplies_passwords_tide_has_not_stored() {
    use std::os::unix::fs::PermissionsExt;
    let (home, mut app) = test_app();
    let pgpass = home.path().join("pgpass");
    std::fs::write(&pgpass, "db.example.test:5432:app:alice:from-pgpass\n").unwrap();
    std::fs::set_permissions(&pgpass, std::fs::Permissions::from_mode(0o600)).unwrap();
    app.db.pgpass = Some(pgpass);
    let config = ConnectionConfig {
        name: "Team database".into(),
        host: "db.example.test".into(),
        database: "app".into(),
        username: "alice".into(),
        scope: tide_db::ConnectionScope::Project,
        ..Default::default()
    };
    assert_eq!(app.db.password(&config).as_deref(), Some("from-pgpass"));
}

#[test]
fn database_password_unlock_preserves_draft_and_never_renders_or_persists_plaintext() {
    let (home, mut app) = test_app();
    let secrets = Arc::new(SecretService::encrypted_only(
        home.path().join("fixture-secrets.json"),
    ));
    app.db.secrets = secrets;
    let config = ConnectionConfig {
        name: "Private fixture".into(),
        database: "app".into(),
        username: "fixture".into(),
        ..Default::default()
    };
    app.with_db(|db, ui| {
        db.submit_connection_form(
            crate::connection_form::Submission {
                config,
                password: Some("db-fixture-password".into()),
                action: ConnectionAction::Save,
            },
            ui,
        )
    });
    assert!(app.db.store.connections().is_empty());
    assert_eq!(
        *app.overlay.as_ref().unwrap().kind(),
        ModalKind::Prompt(PromptAction::UnlockConnectionSecrets)
    );
    app.handle_modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(
        *app.overlay.as_ref().unwrap().kind(),
        ModalKind::ConnectionForm
    );
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(82, 26)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(!text.contains("db-fixture-password"));
    app.handle_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    assert_eq!(
        *app.overlay.as_ref().unwrap().kind(),
        ModalKind::Prompt(PromptAction::UnlockConnectionSecrets)
    );
    app.handle_paste("master-fixture-password");
    terminal.draw(|f| app.draw(f)).unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(!text.contains("master-fixture-password"));
    app.handle_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.db.store.connections().len(), 1);
    let config = &app.db.store.connections()[0];
    assert_eq!(
        app.db
            .secrets
            .get(config.password_secret.as_ref().unwrap())
            .as_deref(),
        Some("db-fixture-password")
    );
    let records = std::fs::read_to_string(&app.paths.connections_file).unwrap();
    assert!(!records.contains("db-fixture-password"));
    assert!(
        !std::fs::read_to_string(home.path().join("fixture-secrets.json"))
            .unwrap()
            .contains("db-fixture-password")
    );
}

#[test]
fn sql_uses_shared_editor_commands_and_disk_recovery_guards() {
    let (_home, mut app) = test_app();
    app.with_db(|db, ui| db.open_tab(Some("SELECT 1;"), ui));
    app.execute_command(Command::EditSelectAll);
    assert_eq!(
        app.workbench
            .active_editor()
            .unwrap()
            .selected_text()
            .as_deref(),
        Some("SELECT 1;")
    );
    app.handle_paste("SELECT 2;");
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "SELECT 2;"
    );
    app.execute_command(Command::EditUndo);
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "SELECT 1;"
    );
    let path = app.workspace.join("query.sql");
    app.save_as_to(path.clone(), false);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "SELECT 1;");
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("-- draft\n");
    let saved = app.build_session();
    assert_eq!(
        saved.sql_editors[0].file_path.as_deref(),
        Some(path.to_str().unwrap())
    );
    std::fs::write(&path, "external writer\n").unwrap();
    app.restore_session(saved).unwrap();
    app.save_active();
    assert!(app.status.contains("changed on disk"));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "external writer\n");
}

#[tokio::test]
async fn database_requests_are_bounded_and_disconnect_stops_owned_jobs() {
    let (_home, mut app) = test_app_async();
    for _ in 0..64 {
        assert!(app.with_db(|db, ui| db.spawn("fixture".into(), std::future::pending(), ui)));
    }
    assert!(!app.with_db(|db, ui| db.spawn("fixture".into(), std::future::pending(), ui)));
    assert_eq!(app.db.tasks.len(), 64);
    app.with_db(|db, ui| db.cancel_requests("fixture", ui));
    assert!(app.db.tasks.is_empty());
    app.with_db(|db, ui| db.open_tab(Some("SELECT 1"), ui));
    if let Some(Tab::Sql(sql)) = app.workbench.active_tab_mut() {
        sql.running = true;
    }
    app.with_db(|db, ui| db.execute_text("SELECT 2".into(), false, ui));
    assert!(app.status.contains("already running"));
    app.shutdown().await;
}
