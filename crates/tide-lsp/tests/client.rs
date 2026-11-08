use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tide_lsp::{
    DocumentLease, LspClient, LspError, LspEvent, LspRegistry, Position, ServerConfig, SyncKind,
    TextDocumentSync, command_available,
};

#[tokio::test]
async fn initializes_requests_and_routes_diagnostics() {
    if !command_available("python3") {
        eprintln!("python3 not available; skipping LSP mock test");
        return;
    }
    const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/mock_server.py");
    const ARGS: &[&str] = &[SCRIPT];
    let workspace = tempfile::tempdir().unwrap();
    let config = ServerConfig::new("rust", "python3", ARGS.iter().copied());
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);
    let client = LspClient::start(config, workspace.path(), tx)
        .await
        .unwrap();
    client
        .did_open("file:///tmp/mock.rs", "rust", 1, "fn main() {}")
        .await
        .unwrap();

    let completion = client
        .completion(
            "file:///tmp/mock.rs",
            Position {
                line: 0,
                character: 3,
            },
        )
        .await
        .unwrap();
    assert_eq!(completion[0].label, "mock_fn");
    assert_eq!(completion[0].insert_text, "mock_fn()");

    let formatted = client
        .formatting_with_options("file:///tmp/mock.rs", 4, true)
        .await
        .unwrap();
    assert_eq!(formatted[0]["newText"], "    formatted");
    let hover = client
        .hover(
            "file:///tmp/mock.rs",
            Position {
                line: 0,
                character: 3,
            },
        )
        .await
        .unwrap();
    assert_eq!(hover, "mock hover");

    let definitions = client
        .definition(
            "file:///tmp/mock.rs",
            Position {
                line: 0,
                character: 3,
            },
        )
        .await
        .unwrap();
    assert_eq!(definitions[0].range.start.line, 2);

    let actions = client
        .code_actions(
            "file:///tmp/mock.rs",
            tide_lsp::Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            serde_json::json!([]),
        )
        .await
        .unwrap();
    assert_eq!(actions[0].title, "Mock quick fix");
    assert!(actions[0].is_preferred);
    assert!(actions[0].edit.is_some());

    let diagnostics = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match rx.recv().await {
                Some(LspEvent::Diagnostics { diagnostics, .. }) => return diagnostics,
                Some(_) => continue,
                None => panic!("event channel closed"),
            }
        }
    })
    .await
    .expect("diagnostics timeout");
    assert_eq!(diagnostics[0].message, "mock diagnostic");

    client.shutdown().await;
}

#[tokio::test]
#[ignore = "needs TypeScript 7 installed globally or in a parent node_modules"]
async fn starts_real_typescript_seven() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(
        workspace.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"target":"ES2022","module":"ESNext"},"include":["*.ts"]}"#,
    )
    .unwrap();
    let text = "const answer = 42;\nanswer.\n";
    std::fs::write(workspace.path().join("index.ts"), text).unwrap();
    assert!(
        matches!(
            tide_lsp::typescript_install(workspace.path()),
            Some(tide_lsp::TypeScriptInstall::NativeLsp(_))
        ),
        "no TypeScript 7 found"
    );
    let registry = LspRegistry::new();
    let config = tide_lsp::server_for_language("typescript").unwrap();
    let client = tokio::time::timeout(
        Duration::from_secs(20),
        registry.get_or_start(&config, workspace.path(), quiet_events),
    )
    .await
    .expect("TypeScript 7 startup timeout")
    .expect("TypeScript 7 start");
    let uri = tide_lsp::path_to_uri(&workspace.path().join("index.ts")).unwrap();
    client.did_open(&uri, "typescript", 1, text).await.unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let items = client
            .completion(
                &uri,
                Position {
                    line: 1,
                    character: 7,
                },
            )
            .await
            .unwrap();
        if items.iter().any(|item| item.label == "toFixed") {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "TypeScript 7 did not return numeric member completion"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    registry.shutdown_all().await;
}

#[tokio::test]
#[ignore = "needs typescript-language-server on PATH"]
async fn starts_real_typescript_language_server() {
    assert!(
        command_available("typescript-language-server"),
        "typescript-language-server is not on PATH"
    );
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(
        workspace.path().join("package.json"),
        r#"{"name":"lsp-smoke","private":true}"#,
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"target":"ES2022","module":"ESNext"},"include":["*.ts"]}"#,
    )
    .unwrap();
    std::fs::write(
        workspace.path().join("index.ts"),
        "const answer = 42;\nanswer.\n",
    )
    .unwrap();
    let config = ServerConfig::new("typescript", "typescript-language-server", ["--stdio"]);
    let (tx, _rx) = tokio::sync::mpsc::channel(32);
    let client = tokio::time::timeout(
        Duration::from_secs(20),
        LspClient::start(config, workspace.path(), tx),
    )
    .await
    .expect("typescript-language-server startup timeout")
    .expect("typescript-language-server start");
    client
        .did_open(
            &tide_lsp::path_to_uri(&workspace.path().join("index.ts")).unwrap(),
            "typescript",
            1,
            "const answer = 42;\nanswer.\n",
        )
        .await
        .unwrap();
    let uri = tide_lsp::path_to_uri(&workspace.path().join("index.ts")).unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let items = client
            .completion(
                &uri,
                Position {
                    line: 1,
                    character: 7,
                },
            )
            .await
            .unwrap();
        if items.iter().any(|item| item.label == "toFixed") {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Real TypeScript server did not return numeric member completion"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let hover = client
        .hover(
            &uri,
            Position {
                line: 0,
                character: 8,
            },
        )
        .await
        .unwrap();
    assert!(hover.contains("answer"));
    client.shutdown().await;
}

#[tokio::test]
async fn synchronizes_second_documents_and_reopens_after_close() {
    const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/mock_server.py");
    let workspace = tempfile::tempdir().unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(32);
    let client = LspClient::start(
        ServerConfig::new("rust", "python3", [SCRIPT]),
        workspace.path(),
        tx,
    )
    .await
    .unwrap();
    let position = Position {
        line: 0,
        character: 3,
    };
    client
        .did_open("file:///first.rs", "rust", 1, "first")
        .await
        .unwrap();
    client
        .did_change("file:///second.rs", 1, "a😀b")
        .await
        .unwrap();
    assert!(
        !client
            .completion("file:///second.rs", position)
            .await
            .unwrap()
            .is_empty()
    );
    client.did_close("file:///second.rs").await.unwrap();
    assert!(
        client
            .completion("file:///second.rs", position)
            .await
            .is_err()
    );
    client
        .did_change("file:///second.rs", 1, "reopened")
        .await
        .unwrap();
    assert!(
        !client
            .completion("file:///second.rs", position)
            .await
            .unwrap()
            .is_empty()
    );
    client.shutdown().await;
}

#[tokio::test]
async fn server_requests_cannot_consume_responses_and_abandoned_requests_cancel() {
    const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/mock_server.py");
    let workspace = tempfile::tempdir().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);
    let client = LspClient::start(
        ServerConfig::new("rust", "python3", [SCRIPT]),
        workspace.path(),
        tx,
    )
    .await
    .unwrap();
    client
        .did_open("file:///tmp/mock.rs", "rust", 7, "fn main() {}")
        .await
        .unwrap();
    for name in ["fixture.applyNumeric", "fixture.applyString"] {
        let command = client.execute_command(name, serde_json::json!([]));
        tokio::pin!(command);
        loop {
            tokio::select! {
                result=&mut command=>{assert_eq!(result.unwrap()["applied"],true);break;}
                event=rx.recv()=>{if let Some(LspEvent::ApplyEdit{edit,versions,reply})=event {assert_eq!(versions.get("file:///tmp/mock.rs"),Some(&7));assert_eq!(tide_lsp::checked_workspace_edit(&edit).unwrap().len(),1);reply.send(true).unwrap();}}
            }
        }
    }
    let result = client
        .execute_command("fixture.unknown", serde_json::json!([]))
        .await
        .unwrap();
    assert_eq!(result["code"], -32601);
    assert!(
        tokio::time::timeout(
            Duration::from_millis(80),
            client.execute_command("fixture.cancel", serde_json::json!([]))
        )
        .await
        .is_err()
    );
    assert_eq!(
        client
            .execute_command("fixture.cancelSeen", serde_json::json!([]))
            .await
            .unwrap(),
        true
    );
    assert!(matches!(
        client.did_change("file:///tmp/mock.rs", 6, "stale").await,
        Err(LspError::OutOfOrder)
    ));
    client.shutdown().await;
    assert!(!client.is_alive().await);
}

#[tokio::test]
#[ignore = "needs the rust-analyzer component"]
async fn starts_real_rust_analyzer_and_completes_workspace_fields() {
    assert!(
        command_available("rust-analyzer"),
        "rust-analyzer is not on PATH"
    );
    let version = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::process::Command::new("rust-analyzer")
            .arg("--version")
            .kill_on_drop(true)
            .output(),
    )
    .await;
    assert!(
        matches!(version, Ok(Ok(output)) if output.status.success()),
        "rust-analyzer is on PATH but the rustup proxy could not run it"
    );
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir(workspace.path().join("src")).unwrap();
    std::fs::write(
        workspace.path().join("Cargo.toml"),
        "[package]\nname=\"tide_lsp_fixture\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    let text = "struct Thing { value: i32 }\nfn main() { let item = Thing { value: 1 }; item. }\n";
    let path = workspace.path().join("src/main.rs");
    std::fs::write(&path, text).unwrap();
    let uri = tide_lsp::path_to_uri(&path).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(64);
    let client = LspClient::start(
        ServerConfig::new("rust", "rust-analyzer", Vec::<String>::new()),
        workspace.path(),
        tx,
    )
    .await
    .unwrap();
    client.did_open(&uri, "rust", 1, text).await.unwrap();
    let character = text.lines().nth(1).unwrap().find("item. }").unwrap() + 5;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let items = client
            .completion(
                &uri,
                Position {
                    line: 1,
                    character: u32::try_from(character).unwrap(),
                },
            )
            .await
            .unwrap();
        if items.iter().any(|item| item.label.starts_with("value")) {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Real Rust analyzer did not complete the local struct field"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    client.shutdown().await;
}

#[tokio::test]
async fn completed_language_server_cannot_leave_its_helper_running() {
    const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/mock_server.py");
    let workspace = tempfile::tempdir().unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(32);
    let client = LspClient::start(
        ServerConfig::new("rust", "python3", [SCRIPT]),
        workspace.path(),
        tx,
    )
    .await
    .unwrap();
    let pid = client
        .execute_command("fixture.exitParent", serde_json::json!([]))
        .await
        .unwrap()
        .as_i64()
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    while client.is_alive().await {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let start = std::time::Instant::now();
    client.shutdown().await;
    assert!(start.elapsed() < Duration::from_secs(1));
    #[cfg(target_os = "linux")]
    {
        let deadline = std::time::Instant::now() + Duration::from_millis(500);
        loop {
            match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                Err(_) => break,
                Ok(stat) if stat.split_once(") ").unwrap().1.starts_with('Z') => break,
                Ok(stat) => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "Helper still running: {stat}"
                    );
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let output = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        let state = String::from_utf8_lossy(&output.stdout);
        assert!(state.trim().is_empty() || state.trim().starts_with('Z'));
    }
}

#[tokio::test]
async fn reopened_document_replaces_equal_revision_and_ignores_late_old_work() {
    let workspace = tempfile::tempdir().unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(32);
    let client = LspClient::start(
        ServerConfig::new(
            "rust",
            "python3",
            [concat!(env!("CARGO_MANIFEST_DIR"), "/tests/mock_server.py")],
        ),
        workspace.path(),
        tx,
    )
    .await
    .unwrap();
    let uri = "file:///tmp/reopened.rs";
    let old = tide_lsp::DocumentLease::default();
    client
        .synchronize_document(&old, uri, "rust", 1, "old text")
        .await
        .unwrap();
    old.invalidate();
    let current = tide_lsp::DocumentLease::default();
    client
        .synchronize_document(&current, uri, "rust", 1, "fixed new text")
        .await
        .unwrap();
    client.close_document(&old, uri).await.unwrap();
    assert!(matches!(
        client
            .synchronize_document(&old, uri, "rust", 2, "stale")
            .await,
        Err(LspError::DocumentClosed)
    ));
    assert_eq!(
        client
            .execute_command("fixture.order", serde_json::json!([uri]))
            .await
            .unwrap(),
        serde_json::json!(true)
    );
    client
        .synchronize_document(&current, uri, "rust", 2, "fixed edited")
        .await
        .unwrap();
    assert!(matches!(
        client
            .synchronize_document(&current, uri, "rust", 1, "stale")
            .await,
        Err(LspError::OutOfOrder)
    ));
    current.invalidate();
    client.close_document(&current, uri).await.unwrap();
    assert!(
        client
            .completion(
                uri,
                Position {
                    line: 0,
                    character: 0
                }
            )
            .await
            .is_err()
    );
    client.shutdown().await;
}

const MOCK: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/mock_server.py");

fn mock_config(extra: &[&str]) -> ServerConfig {
    ServerConfig::new(
        "rust",
        "python3",
        std::iter::once(MOCK).chain(extra.iter().copied()),
    )
}

#[tokio::test]
async fn flooded_server_requests_do_not_kill_the_reader() {
    let workspace = tempfile::tempdir().unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(32);
    let client = LspClient::start(mock_config(&[]), workspace.path(), tx)
        .await
        .unwrap();
    let flooded = tokio::time::timeout(
        Duration::from_secs(10),
        client.execute_command("fixture.floodRequests", serde_json::json!([5000])),
    )
    .await
    .expect("flood response timeout")
    .unwrap();
    assert_eq!(flooded, "flooded");
    let hover = tokio::time::timeout(
        Duration::from_secs(5),
        client.hover(
            "file:///tmp/mock.rs",
            Position {
                line: 0,
                character: 0,
            },
        ),
    )
    .await
    .expect("request after flood timed out")
    .unwrap();
    assert_eq!(hover, "mock hover");
    assert!(client.is_alive().await);
    client.shutdown().await;
}

#[tokio::test]
async fn dead_reader_reports_the_client_dead_and_fails_requests_fast() {
    let workspace = tempfile::tempdir().unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(32);
    let client = LspClient::start(mock_config(&[]), workspace.path(), tx)
        .await
        .unwrap();
    let corrupt = client.execute_command("fixture.corrupt", serde_json::json!([]));
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(5), corrupt)
            .await
            .expect("pending request was not failed when the reader stopped"),
        Err(LspError::Exited { method }) if method == "workspace/executeCommand"
    ));
    assert!(!client.is_alive().await);
    let hover = tokio::time::timeout(
        Duration::from_secs(2),
        client.hover(
            "file:///tmp/mock.rs",
            Position {
                line: 0,
                character: 0,
            },
        ),
    )
    .await
    .expect("request to a dead reader waited for its timeout");
    assert!(matches!(hover, Err(LspError::Exited { .. })));
    client.shutdown().await;
}

#[tokio::test]
async fn undrained_diagnostics_do_not_stall_responses() {
    let workspace = tempfile::tempdir().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let client = LspClient::start(mock_config(&[]), workspace.path(), tx)
        .await
        .unwrap();
    let flooded = tokio::time::timeout(
        Duration::from_secs(5),
        client.execute_command("fixture.floodDiagnostics", serde_json::json!([500])),
    )
    .await
    .expect("diagnostics backpressure stalled the reader")
    .unwrap();
    assert_eq!(flooded, "flooded");
    let mut latest = std::collections::HashMap::new();
    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(300), rx.recv()).await {
        if let LspEvent::Diagnostics {
            uri, diagnostics, ..
        } = event
        {
            latest.insert(uri, diagnostics[0].message.clone());
        }
    }
    assert_eq!(latest["file:///tmp/flood0.rs"], "flood 498");
    assert_eq!(latest["file:///tmp/flood1.rs"], "flood 499");
    assert_eq!(latest["file:///tmp/flood2.rs"], "flood 497");
    client.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stalled_server_with_concurrent_edits_and_apply_edit_does_not_deadlock() {
    let workspace = tempfile::tempdir().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let client = std::sync::Arc::new(
        LspClient::start(mock_config(&[]), workspace.path(), tx)
            .await
            .unwrap(),
    );
    let events = tokio::spawn(async move {
        let mut edits = 0;
        while let Some(event) = rx.recv().await {
            if let LspEvent::ApplyEdit { reply, .. } = event {
                edits += 1;
                let _ = reply.send(true);
            }
        }
        edits
    });
    let uri = "file:///tmp/stress.rs";
    let lease = tide_lsp::DocumentLease::default();
    let editor = {
        let client = client.clone();
        let lease = lease.clone();
        tokio::spawn(async move {
            for version in 1..=200_i64 {
                let text = format!("{version}{}", "x".repeat(64 * 1024));
                client
                    .synchronize_document(&lease, uri, "rust", version, &text)
                    .await
                    .unwrap();
            }
        })
    };
    let outcome = tokio::time::timeout(Duration::from_secs(20), async {
        let stalled = client
            .execute_command("fixture.stalledEdit", serde_json::json!([]))
            .await
            .unwrap();
        editor.await.unwrap();
        stalled
    })
    .await
    .expect("document synchronization and applyEdit deadlocked");
    assert_eq!(outcome, "stalled");
    client.shutdown().await;
    drop(client);
    assert!(events.await.unwrap() >= 1);
}

async fn start_mock(extra: &[&str]) -> (tempfile::TempDir, LspClient) {
    let workspace = tempfile::tempdir().unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(32);
    let client = LspClient::start(mock_config(extra), workspace.path(), tx)
        .await
        .unwrap();
    (workspace, client)
}

async fn sync_state(client: &LspClient, uri: &str) -> serde_json::Value {
    client
        .execute_command("fixture.syncState", json!([uri]))
        .await
        .unwrap()
}

#[tokio::test]
async fn incremental_sync_sends_ranges_that_reproduce_the_document() {
    let (_workspace, client) =
        start_mock(&["--sync", "incremental", "--save", "include-text"]).await;
    assert_eq!(
        client.text_document_sync(),
        TextDocumentSync {
            change: SyncKind::Incremental,
            save: true,
            save_include_text: true,
        }
    );
    let uri = "file:///tmp/incremental.rs";
    let lease = DocumentLease::default();
    let revisions = [
        "fn main() {\r\n    let wörld = \"😀\";\r\n}\r\n",
        "fn main() {\r\n    let wörld = \"😀😀\";\r\n}\r\n",
        "fn main() {\r\n    let world = \"😀😀\";\r\n}\r\n",
        "fn main() {\n    let world = \"😀😀\";\r\n}\r\n",
        "fn main() {\n    let world = \"😀😀\";\r\n}\r\n// tail\r",
        "fn main() {\n    let world = \"😀😀\";\r\n}\r\n// tail\r\n",
        "fn main() {\n    let world = \"😀😀\";\r\n}\r\n// tail\r\n",
        "// head\nfn main() {}\n",
        "",
        "x",
    ];
    for (index, text) in revisions.iter().enumerate() {
        client
            .synchronize_document(&lease, uri, "rust", i64::try_from(index).unwrap() + 1, text)
            .await
            .unwrap();
        assert_eq!(
            sync_state(&client, uri).await["text"],
            *text,
            "revision {index}"
        );
    }
    let state = sync_state(&client, uri).await;
    let changes = state["changes"].as_array().unwrap();
    assert_eq!(changes.len(), revisions.len() - 1);
    assert!(changes.iter().all(|change| change.get("range").is_some()));
    assert_eq!(
        changes[0],
        json!({"range":{"start":{"line":1,"character":19},"end":{"line":1,"character":19}},"text":"😀"})
    );
    assert_eq!(
        changes[2],
        json!({"range":{"start":{"line":0,"character":11},"end":{"line":1,"character":0}},"text":"\n"})
    );

    client.did_save(uri, "x").await.unwrap();
    assert_eq!(
        sync_state(&client, uri).await["saves"],
        json!([{"textDocument":{"uri":uri},"text":"x"}])
    );
    client.shutdown().await;
}

#[tokio::test]
async fn full_sync_servers_get_whole_documents_and_no_unrequested_saves() {
    let (_workspace, client) = start_mock(&["--sync", "full"]).await;
    assert_eq!(client.text_document_sync().change, SyncKind::Full);
    let uri = "file:///tmp/full.rs";
    let lease = DocumentLease::default();
    for (version, text) in [(1, "one"), (2, "one two")] {
        client
            .synchronize_document(&lease, uri, "rust", version, text)
            .await
            .unwrap();
    }
    client.did_save(uri, "one two").await.unwrap();
    let state = sync_state(&client, uri).await;
    assert_eq!(state["text"], "one two");
    assert_eq!(state["changes"], json!([{"text":"one two"}]));
    assert_eq!(state["saves"], json!([]));
    client.shutdown().await;
}

#[tokio::test]
async fn numeric_sync_capability_is_incremental_and_saves_without_text() {
    let (_workspace, client) = start_mock(&["--sync", "2"]).await;
    assert_eq!(
        client.text_document_sync(),
        TextDocumentSync {
            change: SyncKind::Incremental,
            save: true,
            save_include_text: false,
        }
    );
    let uri = "file:///tmp/numeric.rs";
    client.did_change(uri, 1, "let a = 1;").await.unwrap();
    client.did_change(uri, 2, "let ab = 1;").await.unwrap();
    client.did_save(uri, "let ab = 1;").await.unwrap();
    let state = sync_state(&client, uri).await;
    assert_eq!(state["text"], "let ab = 1;");
    assert_eq!(
        state["changes"],
        json!([{"range":{"start":{"line":0,"character":5},"end":{"line":0,"character":5}},"text":"b"}])
    );
    assert_eq!(state["saves"], json!([{"textDocument":{"uri":uri}}]));
    client.shutdown().await;
}

fn quiet_events() -> tokio::sync::mpsc::Sender<LspEvent> {
    tokio::sync::mpsc::channel(32).0
}

fn start_count(log: &std::path::Path) -> usize {
    std::fs::read_to_string(log).map_or(0, |text| text.lines().count())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_server_process_serves_every_language_with_the_same_command() {
    let workspace = tempfile::tempdir().unwrap();
    let log = workspace.path().join("starts.log");
    let typescript = ServerConfig {
        language_id: "typescript".to_owned(),
        ..mock_config(&["--start-log", log.to_str().unwrap()])
    };
    let tsx = ServerConfig {
        language_id: "typescriptreact".to_owned(),
        ..typescript.clone()
    };
    let registry = Arc::new(LspRegistry::new());
    let starts: Vec<_> = (0..6)
        .map(|index| {
            let registry = registry.clone();
            let config = if index % 2 == 0 {
                typescript.clone()
            } else {
                tsx.clone()
            };
            let root = workspace.path().to_path_buf();
            tokio::spawn(async move {
                registry
                    .get_or_start(&config, &root, quiet_events)
                    .await
                    .unwrap()
            })
        })
        .collect();
    let mut clients = Vec::new();
    for start in starts {
        clients.push(start.await.unwrap());
    }
    assert!(
        clients
            .iter()
            .all(|client| Arc::ptr_eq(client, &clients[0]))
    );
    assert_eq!(start_count(&log), 1);
    assert!(
        registry
            .running(&tsx, workspace.path())
            .await
            .is_some_and(|client| Arc::ptr_eq(&client, &clients[0]))
    );
    registry.shutdown_all().await;
    assert!(!clients[0].is_alive().await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_slow_server_start_does_not_block_other_servers() {
    let workspace = tempfile::tempdir().unwrap();
    let registry = Arc::new(LspRegistry::new());
    let slow = {
        let registry = registry.clone();
        let root = workspace.path().to_path_buf();
        tokio::spawn(async move {
            registry
                .get_or_start(&mock_config(&["--init-delay", "3"]), &root, quiet_events)
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    tokio::time::timeout(
        Duration::from_secs(2),
        registry.get_or_start(&mock_config(&[]), workspace.path(), quiet_events),
    )
    .await
    .expect("a slow server start blocked an unrelated server")
    .unwrap();
    assert!(!slow.is_finished());
    slow.await.unwrap().unwrap();
    registry.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancelled_caller_does_not_abandon_a_server_start() {
    let workspace = tempfile::tempdir().unwrap();
    let log = workspace.path().join("starts.log");
    let config = mock_config(&["--init-delay", "0.5", "--start-log", log.to_str().unwrap()]);
    let registry = Arc::new(LspRegistry::new());
    let caller = {
        let (registry, config) = (registry.clone(), config.clone());
        let root = workspace.path().to_path_buf();
        tokio::spawn(async move { registry.get_or_start(&config, &root, quiet_events).await })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    caller.abort();
    let client = registry
        .get_or_start(&config, workspace.path(), quiet_events)
        .await
        .unwrap();
    assert!(client.is_alive().await);
    assert_eq!(start_count(&log), 1);
    registry.shutdown_all().await;
}

#[tokio::test]
async fn a_crashing_server_restarts_with_backoff() {
    let workspace = tempfile::tempdir().unwrap();
    let log = workspace.path().join("starts.log");
    let config = mock_config(&["--crash-after-init", "--start-log", log.to_str().unwrap()]);
    let registry = LspRegistry::new();
    let mut backoff_errors = 0;
    let mut poll = async || {
        if let Err(error) = registry
            .get_or_start(&config, workspace.path(), quiet_events)
            .await
            && matches!(error, LspError::Backoff { .. })
            && error.to_string().contains("retrying in")
        {
            backoff_errors += 1;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    // Slow runners take a while to start the mock, so wait for the restart generously.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while start_count(&log) < 2 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the crashed server was never restarted"
        );
        poll().await;
    }
    // The second crash backs off for 500ms, so no third start can come this soon.
    let quiet = tokio::time::Instant::now() + Duration::from_millis(300);
    while tokio::time::Instant::now() < quiet {
        poll().await;
    }
    let starts = start_count(&log);
    assert_eq!(
        starts, 2,
        "the crashed server restarted without backing off"
    );
    assert!(backoff_errors > 0);
    registry.shutdown_all().await;
}

#[tokio::test]
async fn a_server_that_cannot_start_says_so_while_it_waits() {
    let workspace = tempfile::tempdir().unwrap();
    let config = ServerConfig::new("rust", "tide-no-such-language-server", Vec::<String>::new());
    let registry = LspRegistry::new();
    assert!(matches!(
        registry
            .get_or_start(&config, workspace.path(), quiet_events)
            .await,
        Err(LspError::Spawn { .. })
    ));
    let error = registry
        .get_or_start(&config, workspace.path(), quiet_events)
        .await
        .err()
        .unwrap();
    assert!(
        error
            .to_string()
            .starts_with("tide-no-such-language-server could not start; retrying in"),
        "{error}"
    );
}

#[tokio::test]
async fn typescript_seven_in_the_project_serves_itself() {
    use std::os::unix::fs::PermissionsExt;
    let workspace = tempfile::tempdir().unwrap();
    let package = workspace.path().join("node_modules/typescript");
    std::fs::create_dir_all(package.join("bin")).unwrap();
    std::fs::write(
        package.join("package.json"),
        r#"{"name":"typescript","version":"7.0.2"}"#,
    )
    .unwrap();
    let args = workspace.path().join("args.txt");
    let log = workspace.path().join("starts.log");
    let tsc = package.join("bin/tsc");
    std::fs::write(
        &tsc,
        format!(
            "#!/bin/sh\necho \"$@\" > '{}'\nexec python3 '{MOCK}' --start-log '{}'\n",
            args.display(),
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&tsc, std::fs::Permissions::from_mode(0o755)).unwrap();
    let typescript = tide_lsp::server_for_language("typescript").unwrap();
    let tsx = tide_lsp::server_for_language("typescriptreact").unwrap();
    let registry = LspRegistry::new();
    let client = registry
        .get_or_start(&typescript, workspace.path(), quiet_events)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&args).unwrap().trim(),
        "--lsp --stdio"
    );
    let shared = registry
        .get_or_start(&tsx, workspace.path(), quiet_events)
        .await
        .unwrap();
    assert!(Arc::ptr_eq(&client, &shared));
    assert_eq!(start_count(&log), 1);
    assert!(registry.running(&tsx, workspace.path()).await.is_some());
    assert!(registry.stop(&typescript, workspace.path()).await);
    assert!(
        registry
            .running(&typescript, workspace.path())
            .await
            .is_none()
    );
    assert!(!client.is_alive().await);
}

#[tokio::test]
async fn server_stderr_is_kept_in_a_bounded_tail() {
    let (_workspace, client) = start_mock(&["--stderr-lines", "1000"]).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while client.stderr_tail().last().map(String::as_str) != Some("mock stderr 999") {
        assert!(
            tokio::time::Instant::now() < deadline,
            "server stderr was not captured"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let tail = client.stderr_tail();
    assert!(tail.len() <= 200);
    assert_eq!(tail[0], format!("mock stderr {}", 1000 - tail.len()));
    client.shutdown().await;
}
