//! Disposable installation, upgrade, rollback and removal checks.
use crate::util::{TimedOutput, chmod, read_text, repo_root, run_with_timeout, sha256_file};
use anyhow::{Context, Result, ensure};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// `install-rust-local`: compatibility wrapper for the common managed installer.
pub enum LocalAction {
    Binary(PathBuf),
    Rollback,
    Uninstall,
}

pub fn install_local_command(
    root: &Path,
    prefix: &Path,
    action: &LocalAction,
    migrate_legacy: bool,
) -> Result<Command> {
    let prefix = std::path::absolute(prefix)?;
    let mut command = match action {
        LocalAction::Binary(binary) => {
            let mut command = Command::new("/bin/sh");
            command
                .arg(root.join("install.sh"))
                .arg("--binary")
                .arg(std::path::absolute(binary)?);
            if migrate_legacy {
                command.arg("--migrate-legacy");
            }
            command
        }
        LocalAction::Rollback => {
            let mut command = Command::new("/bin/sh");
            command.arg(root.join("install.sh")).arg("--rollback");
            command
        }
        LocalAction::Uninstall => {
            let mut command = Command::new("/bin/sh");
            command.arg(prefix.join("lib/tide/current/uninstall.sh"));
            command
        }
    };
    command
        .env("TIDE_INSTALL_ROOT", prefix.join("lib/tide"))
        .env("TIDE_BIN_DIR", prefix.join("bin"));
    Ok(command)
}

pub fn install_local(prefix: &Path, action: &LocalAction, migrate_legacy: bool) -> Result<i32> {
    let status = install_local_command(&repo_root(), prefix, action, migrate_legacy)?.status()?;
    Ok(status.code().unwrap_or(1))
}

fn read_link(path: &Path) -> Result<String> {
    Ok(std::fs::read_link(path)
        .with_context(|| format!("readlink {}", path.display()))?
        .display()
        .to_string())
}

/// `verify-rust-local-install`: install, upgrade, rollback and safe removal in a
/// disposable prefix, with independently built debug and release binaries.
pub fn verify_local_install(debug: &Path, release: &Path) -> Result<()> {
    let xtask = std::env::current_exe()?;
    let debug = std::path::absolute(debug)?;
    let release = std::path::absolute(release)?;
    let scratch = tempfile::Builder::new().prefix("tide-install-").tempdir()?;
    let scratch_path = scratch.path().canonicalize()?;
    let prefix = scratch_path.join("prefix");
    let home = scratch_path.join("home");
    std::fs::create_dir(&home)?;
    let mut env: Vec<(String, PathBuf)> = vec![("HOME".into(), home.clone())];
    for kind in ["CONFIG", "DATA", "STATE", "CACHE"] {
        env.push((format!("XDG_{kind}_HOME"), scratch_path.join(kind)));
    }
    let config = scratch_path.join("CONFIG/tide/settings.jsonc");
    std::fs::create_dir_all(config.parent().context("config parent")?)?;
    std::fs::write(&config, "{\"editor.tabSize\":7}")?;
    let timeout = Duration::from_secs(10);
    let run = |program: &Path, args: &[&str], stdin_null: bool| -> Result<TimedOutput> {
        let mut command = Command::new(program);
        command
            .args(args)
            .envs(env.iter().map(|(key, value)| (key, value)));
        if stdin_null {
            command.stdin(Stdio::null());
        }
        run_with_timeout(&mut command, timeout)
    };
    let manage = |args: &[&str], ok: bool| -> Result<()> {
        let mut full = vec!["install-rust-local", "--prefix"];
        let prefix_text = prefix.display().to_string();
        full.push(&prefix_text);
        full.extend_from_slice(args);
        let result = run(&xtask, &full, false)?;
        ensure!(result.success == ok, "{}", result.stderr);
        Ok(())
    };
    let launcher = prefix.join("bin/tide");
    let check = || -> Result<()> {
        for flag in ["--version", "--check-catalogs", "--check-terminal"] {
            let result = run(&launcher, &[flag], false)?;
            ensure!(
                result.success,
                "{flag}: {} {}",
                result.stdout.chars().take(2000).collect::<String>(),
                result.stderr.chars().take(2000).collect::<String>()
            );
        }
        Ok(())
    };
    let debug_text = debug.display().to_string();
    let release_text = release.display().to_string();
    let current = prefix.join("lib/tide/current");
    // `curl … | sh` feeds a copy of uninstall.sh to sh, which hands over to the
    // active release's own uninstaller.
    let piped_uninstall = || -> Result<TimedOutput> {
        let mut command = Command::new("/bin/sh");
        command
            .envs(env.iter().map(|(key, value)| (key, value)))
            .env("TIDE_INSTALL_ROOT", prefix.join("lib/tide"))
            .stdin(std::fs::File::open(repo_root().join("uninstall.sh"))?);
        run_with_timeout(&mut command, timeout)
    };
    let nothing = piped_uninstall()?;
    ensure!(
        !nothing.success && nothing.stderr.contains("brew uninstall tide"),
        "A piped uninstall without an installation did not explain itself: {}",
        nothing.stderr
    );

    manage(&["--binary", &debug_text], true)?;
    check()?;
    let first = read_link(&current)?;
    manage(&["--binary", &release_text], true)?;
    check()?;
    let second = read_link(&current)?;
    ensure!(
        first != second,
        "Upgrade requires independently built debug/release binaries"
    );
    manage(&["--rollback"], true)?;
    check()?;
    ensure!(
        read_link(&current)? == first,
        "Rollback did not restore the first build"
    );
    let removal = run(&launcher, &["--uninstall"], false)?;
    ensure!(removal.success, "{}", removal.stderr);
    ensure!(
        std::fs::symlink_metadata(&launcher).is_err(),
        "Uninstall left the launcher behind"
    );
    ensure!(
        read_text(&config)? == "{\"editor.tabSize\":7}",
        "Uninstall changed settings"
    );
    // A source binary must not remove a managed installation or its data.
    manage(&["--binary", &release_text], true)?;
    let unmanaged = run(&release, &["--uninstall", "--purge", "--yes"], false)?;
    ensure!(
        !unmanaged.success && launcher.exists() && config.exists(),
        "An unmanaged binary removed the managed installation"
    );
    let cancelled = run(&launcher, &["--uninstall", "--purge"], true)?;
    ensure!(
        !cancelled.success && launcher.exists() && config.exists(),
        "Purge without confirmation removed data"
    );
    let purged = run(&launcher, &["--uninstall", "--purge", "--yes"], false)?;
    ensure!(purged.success, "{}", purged.stderr);
    ensure!(
        !config.exists()
            && std::fs::symlink_metadata(&launcher).is_err()
            && !prefix.join("lib/tide").exists(),
        "Purge left data behind"
    );
    std::fs::write(&launcher, "foreign executable")?;
    chmod(&launcher, 0o755)?;
    manage(&["--binary", &release_text], false)?;
    ensure!(
        read_text(&launcher)? == "foreign executable",
        "A foreign launcher was replaced"
    );
    std::fs::remove_file(&launcher)?;

    let legacy = prefix.join("lib/tide-rust");
    let old_hash = sha256_file(&debug)?;
    let old_release = legacy.join("releases").join(&old_hash);
    std::fs::create_dir_all(&old_release)?;
    std::fs::copy(&debug, old_release.join("tide"))?;
    std::fs::write(
        old_release.join("manifest.json"),
        serde_json::json!({ "format": 1, "sha256": old_hash }).to_string(),
    )?;
    std::os::unix::fs::symlink(format!("releases/{old_hash}"), legacy.join("current"))?;
    std::os::unix::fs::symlink(legacy.join("current/tide"), &launcher)?;
    manage(&["--binary", &release_text], false)?;
    ensure!(
        read_link(&launcher)? == legacy.join("current/tide").display().to_string(),
        "Legacy launcher changed without --migrate-legacy"
    );
    manage(&["--binary", &release_text, "--migrate-legacy"], true)?;
    check()?;
    ensure!(
        read_link(&prefix.join("lib/tide/previous"))? == format!("releases/local-{old_hash}"),
        "Legacy build is not the rollback target"
    );
    ensure!(
        old_release.join("tide").exists(),
        "Legacy rollback build must remain untouched"
    );
    manage(&["--rollback"], true)?;
    check()?;
    let piped = piped_uninstall()?;
    ensure!(piped.success, "{}", piped.stderr);
    ensure!(
        std::fs::symlink_metadata(&launcher).is_err() && !prefix.join("lib/tide").exists(),
        "A piped uninstall left the managed installation behind"
    );
    ensure!(
        ![".zshrc", ".bashrc", ".profile"]
            .iter()
            .any(|name| home.join(name).exists()),
        "The installer edited a shell profile"
    );
    println!(
        "Rust install, binary checks, upgrade/rollback, safe CLI and piped uninstall/purge, legacy migration and foreign-executable refusal passed."
    );
    Ok(())
}

/// Serves files below a directory over loopback HTTP until dropped. Redirect
/// routes stand in for GitHub's `/releases/latest` to `/releases/tag/v…` hop.
struct StaticServer {
    port: u16,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<String>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl StaticServer {
    fn start(directory: PathBuf, redirects: Vec<(String, String)>) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&requests);
        let thread = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = serve(&directory, &redirects, port, &log, stream);
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(10)),
                }
            }
        });
        Ok(Self {
            port,
            stop,
            requests,
            thread: Some(thread),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// Request paths received so far, in order.
    fn requests(&self) -> Vec<String> {
        self.requests
            .lock()
            .map(|log| log.clone())
            .unwrap_or_default()
    }
}

impl Drop for StaticServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(
    directory: &Path,
    redirects: &[(String, String)],
    port: u16,
    log: &Mutex<Vec<String>>,
    stream: TcpStream,
) -> Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
    }
    let path = request.split_whitespace().nth(1).unwrap_or("/");
    if let Ok(mut log) = log.lock() {
        log.push(path.to_owned());
    }
    let safe = !path.split('/').any(|part| part == "..");
    let file = directory.join(path.trim_start_matches('/'));
    let mut stream = stream;
    if let Some((_, target)) = redirects.iter().find(|(from, _)| from == path) {
        write!(
            stream,
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{port}{target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )?;
        stream.flush()?;
        return Ok(());
    }
    match std::fs::read(&file) {
        Ok(body) if safe && file.is_file() => {
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                body.len()
            )?;
            stream.write_all(&body)?;
        }
        _ => {
            stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )?;
        }
    }
    stream.flush()?;
    Ok(())
}

/// Writes `payload` as `<stem>/...` into a gzip tarball, optionally with a
/// traversal entry the installer must reject, and records `SHA256SUMS`.
fn archive_payload(archive: &Path, payload: &Path, stem: &str, traversal: bool) -> Result<()> {
    let file = std::fs::File::create(archive)?;
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut bundle = tar::Builder::new(encoder);
    bundle.append_dir_all(stem, payload)?;
    if traversal {
        let mut header = tar::Header::new_gnu();
        let name = b"../../../escape";
        header.as_old_mut().name[..name.len()].copy_from_slice(name);
        header.set_size(6);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        bundle.append(&header, &b"escape"[..])?;
    }
    bundle.into_inner()?.finish()?;
    let digest = sha256_file(archive)?;
    let name = archive
        .file_name()
        .context("archive name")?
        .to_string_lossy()
        .into_owned();
    std::fs::write(
        archive.with_file_name("SHA256SUMS"),
        format!("{digest}  {name}\n"),
    )?;
    Ok(())
}

/// `verify-rust-release-install --binary <path>`: activates a real Rust payload
/// and, on Linux, the HTTP download bootstrap in a scratch home.
pub fn verify_release_install(binary: &Path) -> Result<()> {
    let binary = std::path::absolute(binary)?.canonicalize()?;
    let repo = repo_root();
    let scratch = tempfile::Builder::new()
        .prefix("tide-release-install-")
        .tempdir()?;
    let root = scratch.path().canonicalize()?;
    let home = root.join("home");
    let temporary = root.join("tmp");
    std::fs::create_dir(&home)?;
    std::fs::create_dir(&temporary)?;
    let prefix = home.join(".local");
    let store = prefix.join("lib/tide");
    let mut env: Vec<(String, String)> = vec![
        ("HOME".into(), home.display().to_string()),
        ("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into()),
        ("TERM".into(), "xterm-256color".into()),
        ("TMPDIR".into(), temporary.display().to_string()),
        ("TIDE_INSTALL_ROOT".into(), store.display().to_string()),
        (
            "TIDE_BIN_DIR".into(),
            prefix.join("bin").display().to_string(),
        ),
    ];
    for kind in ["CONFIG", "DATA", "STATE", "CACHE"] {
        env.push((
            format!("XDG_{kind}_HOME"),
            root.join(kind).display().to_string(),
        ));
    }
    let run = |env: &[(String, String)],
               program: &Path,
               args: &[&Path],
               ok: bool|
     -> Result<TimedOutput> {
        let mut command = Command::new(program);
        command
            .args(args)
            .env_clear()
            .envs(env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::null());
        let result = run_with_timeout(&mut command, Duration::from_secs(15))?;
        ensure!(
            result.success == ok,
            "{}",
            if result.stderr.is_empty() {
                &result.stdout
            } else {
                &result.stderr
            }
        );
        Ok(result)
    };
    let path = Path::new;
    run(&env, &binary, &[path("--install-local")], true)?;
    let local_id = read_link(&store.join("current"))?;
    let local_payload = store.join(&local_id);
    let mut manifest: serde_json::Value =
        serde_json::from_str(&read_text(&local_payload.join("VERSION.json"))?)?;
    let version = manifest["version"].as_str().unwrap_or_default().to_owned();
    let platform = manifest["platform"].as_str().unwrap_or_default().to_owned();
    let stem = format!(
        "tide-v{version}-{platform}-{}",
        manifest["architecture"].as_str().unwrap_or_default()
    );
    let server_root = root.join("server");
    let published = server_root.join(format!("v{version}"));
    std::fs::create_dir_all(&published)?;
    let payload = root.join(&stem);
    std::fs::create_dir(&payload)?;
    for name in ["tide", "uninstall.sh", "LICENSE", "ATTRIBUTIONS.md"] {
        std::fs::copy(local_payload.join(name), payload.join(name))?;
    }
    manifest["local"] = serde_json::json!(false);
    manifest["sourceCommit"] = serde_json::json!("1".repeat(40));
    std::fs::write(
        payload.join("VERSION.json"),
        format!("{}\n", serde_json::to_string_pretty(&manifest)?),
    )?;
    run(
        &env,
        &binary,
        &[path("--verify-install-manifest"), &payload],
        true,
    )?;
    let launcher = prefix.join("bin/tide");
    let settings = root.join("CONFIG/tide/settings.jsonc");
    std::fs::create_dir_all(settings.parent().context("settings parent")?)?;
    std::fs::write(&settings, "{\"editor.tabSize\":7}")?;
    let installer = repo.join("install.sh");
    let sh = path("/bin/sh");

    // Without a version, install.sh follows GitHub's latest-release redirect.
    let server = StaticServer::start(
        server_root.clone(),
        vec![
            (
                "/releases/latest".into(),
                format!("/releases/tag/v{version}"),
            ),
            ("/unreleased/latest".into(), "/releases".into()),
        ],
    )?;
    let mut download_env = env.clone();
    download_env.push(("TIDE_RELEASE_BASE_URL".into(), server.url("")));
    let mut unresolved_env = download_env.clone();
    unresolved_env.push((
        "TIDE_LATEST_VERSION_URL".into(),
        server.url("/unreleased/latest"),
    ));
    let unresolved = run(&unresolved_env, sh, &[&installer], false)?;
    ensure!(
        unresolved.stderr.contains("no published Tide release")
            && !server
                .requests()
                .iter()
                .any(|request| request.ends_with("/SHA256SUMS")),
        "A redirect without a release tag did not stop the installer: {}",
        unresolved.stderr
    );
    let mut latest_env = download_env.clone();
    latest_env.push((
        "TIDE_LATEST_VERSION_URL".into(),
        server.url("/releases/latest"),
    ));
    let checksums = format!("/v{version}/SHA256SUMS");

    if platform == "linux" {
        let archive = published.join(format!("{stem}.tar.gz"));
        archive_payload(&archive, &payload, &stem, false)?;
        run(&latest_env, sh, &[&installer], true)?;
        ensure!(
            server.requests().contains(&checksums),
            "The latest-release redirect did not select v{version}"
        );
        let mut env = download_env.clone();
        env.push(("TIDE_VERSION".into(), version.clone()));
        ensure!(
            read_link(&store.join("current"))? == format!("releases/{version}"),
            "Download did not activate"
        );
        ensure!(
            read_link(&store.join("previous"))? == local_id,
            "Local build is not the rollback target"
        );
        let before = read_link(&store.join("current"))?;
        // Failed fresh downloads cannot change the active installation.
        std::fs::remove_dir_all(store.join("releases").join(&version))?;
        // Restore local first, so the rejection cases have a working active build.
        run(&env, sh, &[&installer, path("--rollback")], true)?;
        let original = std::fs::read(payload.join("tide"))?;
        std::fs::write(payload.join("tide"), "corrupt executable")?;
        archive_payload(&archive, &payload, &stem, false)?;
        run(&env, sh, &[&installer], false)?;
        ensure!(
            read_link(&store.join("current"))? == local_id,
            "A corrupt payload was activated"
        );
        std::fs::write(payload.join("tide"), original)?;
        chmod(&payload.join("tide"), 0o755)?;
        archive_payload(&archive, &payload, &stem, true)?;
        run(&env, sh, &[&installer], false)?;
        ensure!(
            !root.join("escape").exists() && read_link(&store.join("current"))? == local_id,
            "An archive traversal escaped or activated"
        );
        archive_payload(&archive, &payload, &stem, false)?;
        run(&env, sh, &[&installer], true)?;
        std::fs::create_dir(store.join("install.lock"))?;
        run(&env, sh, &[&installer], false)?;
        std::fs::remove_dir(store.join("install.lock"))?;
        ensure!(
            read_link(&store.join("current"))? == before,
            "Concurrent install changed the active build"
        );
    } else {
        // Nothing is published for macOS here: the resolved version must still
        // select that release's checksums before signed downloads are refused.
        run(&latest_env, sh, &[&installer], false)?;
        ensure!(
            server.requests().contains(&checksums),
            "The latest-release redirect did not select v{version}"
        );
        // Unsigned macOS CI artifacts exercise only the explicit trusted-local path.
        // Real codesign/notarization and signed bootstrap acceptance are separate gates.
        run(&env, &binary, &[path("--install-release"), &payload], true)?;
        ensure!(
            read_link(&store.join("current"))? == format!("releases/{version}"),
            "Release payload did not activate"
        );
        ensure!(
            read_link(&store.join("previous"))? == local_id,
            "Local build is not the rollback target"
        );
    }
    drop(server);
    for flag in ["--version", "--check-catalogs", "--check-terminal"] {
        run(&env, &launcher, &[path(flag)], true)?;
    }
    run(&env, &launcher, &[path("--uninstall")], true)?;
    ensure!(
        read_text(&settings)? == "{\"editor.tabSize\":7}",
        "Uninstall changed settings"
    );
    ensure!(
        std::fs::symlink_metadata(&launcher).is_err() && !store.exists(),
        "Uninstall left the installation behind"
    );
    println!(
        "Real Rust payload activation, local-to-release upgrade, rollback preservation and safe uninstall passed."
    );
    println!("GitHub latest-release redirect resolution and untagged-redirect refusal passed.");
    if platform == "linux" {
        println!(
            "Linux HTTP bootstrap, corrupt payload, archive traversal and concurrent-install refusal passed."
        );
    } else {
        println!(
            "macOS unsigned candidate checked through trusted-local payload mode; real signed bootstrap remains a separate gate."
        );
    }
    Ok(())
}

/// Reads exactly what a loopback client receives, for the server test.
#[cfg(test)]
fn http_get(port: u16, path: &str) -> Result<String> {
    use std::io::Read;
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    write!(stream, "GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n")?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_server_serves_files_and_refuses_traversal() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("v1")).unwrap();
        std::fs::write(root.path().join("v1/SHA256SUMS"), "abc  file\n").unwrap();
        let server = StaticServer::start(
            root.path().to_path_buf(),
            vec![("/releases/latest".into(), "/releases/tag/v1".into())],
        )
        .unwrap();
        let found = http_get(server.port, "/v1/SHA256SUMS").unwrap();
        assert!(found.starts_with("HTTP/1.1 200 OK") && found.ends_with("abc  file\n"));
        assert!(
            http_get(server.port, "/v1/missing")
                .unwrap()
                .starts_with("HTTP/1.1 404")
        );
        assert!(
            http_get(server.port, "/../etc/passwd")
                .unwrap()
                .starts_with("HTTP/1.1 404")
        );
        let redirect = http_get(server.port, "/releases/latest").unwrap();
        assert!(
            redirect.starts_with("HTTP/1.1 302")
                && redirect.contains(&format!(
                    "Location: http://127.0.0.1:{}/releases/tag/v1\r\n",
                    server.port
                )),
            "{redirect}"
        );
        assert_eq!(
            server.requests(),
            [
                "/v1/SHA256SUMS",
                "/v1/missing",
                "/../etc/passwd",
                "/releases/latest"
            ]
        );
    }

    #[test]
    fn traversal_archives_carry_the_hostile_entry_and_a_matching_checksum() {
        let root = tempfile::tempdir().unwrap();
        let payload = root.path().join("stem");
        std::fs::create_dir(&payload).unwrap();
        std::fs::write(payload.join("tide"), "binary").unwrap();
        let archive = root.path().join("stem.tar.gz");
        archive_payload(&archive, &payload, "stem", true).unwrap();
        let listing = Command::new("tar")
            .arg("-tzf")
            .arg(&archive)
            .output()
            .unwrap();
        let names = String::from_utf8_lossy(&listing.stdout);
        assert!(
            names.contains("stem/tide") && names.contains("../../../escape"),
            "{names}"
        );
        let sums = std::fs::read_to_string(root.path().join("SHA256SUMS")).unwrap();
        assert_eq!(
            sums,
            format!("{}  stem.tar.gz\n", sha256_file(&archive).unwrap())
        );
    }

    #[test]
    fn local_installer_commands_target_the_managed_prefix() {
        let command = install_local_command(
            Path::new("/repo"),
            Path::new("/prefix"),
            &LocalAction::Binary(PathBuf::from("/build/tide")),
            true,
        )
        .unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "/repo/install.sh",
                "--binary",
                "/build/tide",
                "--migrate-legacy"
            ]
        );
        let envs: Vec<_> = command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert!(envs.contains(&("TIDE_INSTALL_ROOT".into(), Some("/prefix/lib/tide".into()))));
        let uninstall = install_local_command(
            Path::new("/repo"),
            Path::new("/prefix"),
            &LocalAction::Uninstall,
            false,
        )
        .unwrap();
        assert_eq!(
            uninstall.get_args().next().unwrap().to_string_lossy(),
            "/prefix/lib/tide/current/uninstall.sh"
        );
    }
}
