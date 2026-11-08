//! Runs alone in its own binary because it replaces `PATH` for the process;
//! `temp_env` keeps its tests from overlapping.

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tide_lsp::{LspClient, ServerConfig};

#[test]
fn typescript_tsserver_lookup_does_not_block_the_runtime() {
    let bin = tempfile::tempdir().unwrap();
    let npm = bin.path().join("npm");
    std::fs::write(&npm, "#!/bin/sh\nsleep 2\necho /nonexistent\n").unwrap();
    std::fs::set_permissions(&npm, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    temp_env::with_vars(
        [("PATH", Some(path.as_str())), ("TIDE_TSSERVER_PATH", None)],
        || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let worst_gap = Arc::new(AtomicU64::new(0));
                let ticker = {
                    let worst_gap = worst_gap.clone();
                    tokio::spawn(async move {
                        let mut last = Instant::now();
                        loop {
                            tokio::time::sleep(Duration::from_millis(10)).await;
                            let gap = u64::try_from(last.elapsed().as_millis()).unwrap_or(u64::MAX);
                            worst_gap.fetch_max(gap, Ordering::Relaxed);
                            last = Instant::now();
                        }
                    })
                };
                tokio::time::sleep(Duration::from_millis(50)).await;
                let workspace = tempfile::tempdir().unwrap();
                let config = ServerConfig::new(
                    "typescript",
                    "python3",
                    [concat!(env!("CARGO_MANIFEST_DIR"), "/tests/mock_server.py")],
                );
                let (tx, _rx) = tokio::sync::mpsc::channel(32);
                let started = Instant::now();
                let client = LspClient::start(config, workspace.path(), tx)
                    .await
                    .unwrap();
                assert!(
                    started.elapsed() >= Duration::from_secs(2),
                    "fake npm did not run"
                );
                client.shutdown().await;
                ticker.abort();
                // A blocked runtime stalls for the whole 2 s the fake npm sleeps. Busy CI
                // runners stall a 10 ms tick for over half a second on their own, so the
                // limit sits well between the two.
                let worst_gap = worst_gap.load(Ordering::Relaxed);
                assert!(
                    worst_gap < 1500,
                    "the runtime stalled for {worst_gap}ms during initialize"
                );
            });
        },
    );
}

#[test]
fn a_workspace_without_typescript_is_told_so_plainly() {
    let bin = tempfile::tempdir().unwrap();
    let npm = bin.path().join("npm");
    std::fs::write(&npm, "#!/bin/sh\necho /nonexistent\n").unwrap();
    std::fs::set_permissions(&npm, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    temp_env::with_vars(
        [("PATH", Some(path.as_str())), ("TIDE_TSSERVER_PATH", None)],
        || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let workspace = tempfile::tempdir().unwrap();
                let config = tide_lsp::server_for_language("typescript").unwrap();
                let registry = tide_lsp::LspRegistry::new();
                let error = registry
                    .get_or_start(&config, workspace.path(), || {
                        tokio::sync::mpsc::channel(1).0
                    })
                    .await
                    .err()
                    .unwrap();
                assert!(matches!(error, tide_lsp::LspError::NoTypeScript));
                assert!(error.to_string().contains("npm install -g typescript"));
            });
        },
    );
}
