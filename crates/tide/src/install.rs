//! Managed local/release installation. User data is never part of activation.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Payload {
    path: String,
    sha256: String,
    mode: String,
    size: u64,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u8,
    product: String,
    version: String,
    source_commit: String,
    target: String,
    platform: String,
    architecture: String,
    prerelease: bool,
    implementation: String,
    #[serde(default)]
    local: bool,
    payloads: Vec<Payload>,
}

pub fn hash(path: &Path) -> Result<String> {
    let meta = std::fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && meta.len() <= 512 * 1024 * 1024,
        "Expected a regular file no larger than 512 MiB: {}",
        path.display()
    );
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}
fn architecture() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    }
}
fn target() -> String {
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86_64"
    };
    format!(
        "{arch}-{}",
        if cfg!(target_os = "macos") {
            "apple-darwin"
        } else if cfg!(target_env = "musl") {
            "unknown-linux-musl"
        } else {
            "unknown-linux-gnu"
        }
    )
}
fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c))
}
fn safe_directory(path: &Path) -> Result<()> {
    ensure!(path.is_absolute(), "Installation paths must be absolute");
    let mut current = PathBuf::new();
    for part in path.components() {
        ensure!(
            matches!(part, Component::RootDir | Component::Normal(_)),
            "Unsafe installation path"
        );
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) => ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "Refusing non-directory or symlink: {}",
                current.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current)?
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
fn existing_link(path: &Path) -> Result<Option<PathBuf>> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            ensure!(
                meta.file_type().is_symlink(),
                "Refusing non-link: {}",
                path.display()
            );
            Ok(Some(std::fs::read_link(path)?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
fn release_id(link: &Path) -> Result<String> {
    let pieces: Vec<_> = link.components().collect();
    ensure!(
        pieces.len() == 2 && pieces[0].as_os_str() == "releases",
        "Installation link escapes release store"
    );
    let id = pieces[1]
        .as_os_str()
        .to_str()
        .context("Invalid release identity")?;
    ensure!(safe_id(id), "Invalid release identity");
    Ok(id.into())
}
fn replace_link(path: &Path, target: Option<&Path>) -> Result<()> {
    existing_link(path)?;
    if let Some(target) = target {
        let temporary = path.with_extension(format!("next-{}", std::process::id()));
        std::os::unix::fs::symlink(target, &temporary)
            .context("create exclusive activation link")?;
        if let Err(error) = std::fs::rename(&temporary, path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error.into());
        }
    } else if path.is_symlink() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

pub fn verify_payload(path: &Path) -> Result<()> {
    verify(path).map(|_| ())
}
fn verify(path: &Path) -> Result<Manifest> {
    ensure!(
        std::fs::symlink_metadata(path)?.is_dir(),
        "Release must be a real directory"
    );
    let manifest_path = path.join("VERSION.json");
    ensure!(
        std::fs::symlink_metadata(&manifest_path)?.is_file()
            && std::fs::metadata(&manifest_path)?.len() <= 65536,
        "Invalid VERSION.json"
    );
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(manifest_path)?)?;
    ensure!(
        manifest.schema_version == 2
            && manifest.product == "Tide"
            && manifest.implementation == "rust"
            && safe_id(&manifest.version),
        "Unsupported Tide installation manifest"
    );
    ensure!(
        manifest.platform == platform()
            && manifest.architecture == architecture()
            && manifest.target == target(),
        "Release target does not match this executable"
    );
    ensure!(
        manifest.local
            || (manifest.source_commit.len() == 40
                && manifest
                    .source_commit
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))),
        "Published payload requires a complete source commit"
    );
    let expected: BTreeSet<_> = ["ATTRIBUTIONS.md", "LICENSE", "tide", "uninstall.sh"]
        .into_iter()
        .collect();
    let actual: BTreeSet<_> = manifest.payloads.iter().map(|p| p.path.as_str()).collect();
    ensure!(
        actual == expected && manifest.payloads.len() == expected.len(),
        "Unexpected release payload paths"
    );
    let found: BTreeSet<_> = std::fs::read_dir(path)?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<std::io::Result<_>>()?;
    let expected_files: BTreeSet<_> = expected
        .into_iter()
        .chain(["VERSION.json"])
        .map(String::from)
        .collect();
    ensure!(
        found == expected_files,
        "Unexpected release directory entries"
    );
    for entry in &manifest.payloads {
        use std::os::unix::fs::PermissionsExt;
        let file = path.join(&entry.path);
        let meta = std::fs::symlink_metadata(&file)?;
        let mode = if matches!(entry.path.as_str(), "tide" | "uninstall.sh") {
            "0755"
        } else {
            "0644"
        };
        ensure!(
            entry.mode == mode
                && meta.permissions().mode() & 0o777 == u32::from_str_radix(mode, 8)?,
            "Incorrect payload permissions: {}",
            entry.path
        );
        ensure!(
            meta.is_file() && meta.len() == entry.size && hash(&file)? == entry.sha256,
            "Payload verification failed: {}",
            entry.path
        );
    }
    Ok(manifest)
}

struct Lock(PathBuf);
impl Lock {
    fn acquire(path: PathBuf) -> Result<Self> {
        match std::fs::create_dir(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                // A lock whose recorded owner no longer exists was left by a crash.
                let owner = std::fs::read_to_string(path.join("pid"))
                    .ok()
                    .and_then(|pid| pid.trim().parse::<i32>().ok())
                    .and_then(rustix::process::Pid::from_raw);
                ensure!(
                    owner.is_some_and(|pid| rustix::process::test_kill_process(pid).is_err()),
                    "Another install is active, or an interrupted install.lock needs inspection"
                );
                std::fs::remove_file(path.join("pid"))?;
                std::fs::remove_dir(&path)?;
                std::fs::create_dir(&path)
                    .context("Another install started while a stale lock was removed")?;
            }
            result => result.context("Create install.lock")?,
        }
        let lock = Self(path);
        std::fs::write(lock.0.join("pid"), std::process::id().to_string())?;
        Ok(lock)
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0.join("pid"));
        let _ = std::fs::remove_dir(&self.0);
    }
}
/// The Homebrew formula that owns `executable` when it is a keg binary,
/// `<prefix>/Cellar/<formula>/<version>/bin/<name>`. Callers pass the
/// canonical path, so `<prefix>/bin` and `opt` symlinks resolve into the Cellar.
fn homebrew_formula(executable: &Path) -> Option<String> {
    let names: Vec<&str> = executable
        .components()
        .map(|part| part.as_os_str().to_str())
        .collect::<Option<_>>()?;
    match names.as_slice() {
        [.., "Cellar", formula, _version, "bin", _name] => Some((*formula).to_owned()),
        _ => None,
    }
}

/// Managed-installation commands that a package manager's copy must not run.
#[derive(Clone, Copy, Debug)]
pub enum ManagedCommand {
    Install,
    Uninstall,
}

fn homebrew_refusal(executable: &Path, command: ManagedCommand) -> Result<()> {
    let Some(formula) = homebrew_formula(executable) else {
        return Ok(());
    };
    let shown = executable.display();
    match command {
        ManagedCommand::Uninstall => anyhow::bail!(
            "Tide was installed with Homebrew ({shown}); remove it with `brew uninstall {formula}`"
        ),
        ManagedCommand::Install => anyhow::bail!(
            "Tide was installed with Homebrew ({shown}); update it with `brew upgrade {formula}`, or run `brew uninstall {formula}` before using Tide's own installer"
        ),
    }
}

/// Refuses a managed-installation command from a Homebrew-installed executable:
/// Homebrew owns that copy, its upgrades and its removal.
pub fn refuse_package_managed(command: ManagedCommand) -> Result<()> {
    homebrew_refusal(&std::fs::canonicalize(std::env::current_exe()?)?, command)
}

#[derive(Debug)]
pub struct Installation {
    root: PathBuf,
    bin: PathBuf,
}
impl Installation {
    pub fn discover() -> Result<Self> {
        let home = PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?);
        let root = std::env::var_os("TIDE_INSTALL_ROOT")
            .map_or_else(|| home.join(".local/lib/tide"), PathBuf::from);
        let bin =
            std::env::var_os("TIDE_BIN_DIR").map_or_else(|| home.join(".local/bin"), PathBuf::from);
        ensure!(
            [&root, &bin]
                .iter()
                .all(|p| p.to_str().is_some_and(|s| !s.chars().any(char::is_control))),
            "Installation paths must be valid text without control characters"
        );
        ensure!(
            root.is_absolute()
                && root.file_name().is_some_and(|name| name == "tide")
                && bin.is_absolute(),
            "Invalid installation root or binary directory"
        );
        Ok(Self { root, bin })
    }
    fn lock(&self) -> Result<Lock> {
        safe_directory(&self.root.join("releases"))?;
        safe_directory(&self.bin)?;
        use std::os::unix::fs::PermissionsExt;
        for directory in [&self.root, &self.root.join("releases")] {
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
        }
        Lock::acquire(self.root.join("install.lock"))
    }
    fn owned_launcher(&self) -> Result<()> {
        let link = existing_link(&self.bin.join("tide"))?;
        ensure!(
            link.is_none_or(|link| link == self.root.join("current/tide")),
            "Refusing another executable at {}",
            self.bin.join("tide").display()
        );
        Ok(())
    }
    fn marker(&self, name: &str) -> Result<Option<String>> {
        existing_link(&self.root.join(name))?
            .map(|p| release_id(&p))
            .transpose()
    }
    fn activate(&self, id: &str) -> Result<()> {
        self.owned_launcher()?;
        verify(&self.root.join("releases").join(id))?;
        let current = existing_link(&self.root.join("current"))?;
        let previous = existing_link(&self.root.join("previous"))?;
        let launcher = existing_link(&self.bin.join("tide"))?;
        let target = PathBuf::from("releases").join(id);
        let bin_record = self.root.join("BIN_DIR");
        ensure!(
            !bin_record.is_symlink(),
            "Refusing a symlinked BIN_DIR record"
        );
        let bin_previous = match std::fs::read_to_string(&bin_record) {
            Ok(previous) => Some(previous),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let result = (|| {
            std::fs::write(
                &bin_record,
                self.bin.to_str().context("Invalid binary directory")?,
            )?;
            if current.as_ref().is_some_and(|p| p != &target) {
                replace_link(&self.root.join("previous"), current.as_deref())?;
            }
            replace_link(&self.root.join("current"), Some(&target))?;
            replace_link(
                &self.bin.join("tide"),
                Some(&self.root.join("current/tide")),
            )
        })();
        if let Err(error) = result {
            // Attempt every restore so one failure does not strand the others.
            let mut failed = Vec::new();
            for (path, old) in [
                (self.root.join("current"), current),
                (self.root.join("previous"), previous),
                (self.bin.join("tide"), launcher),
            ] {
                if existing_link(&path).is_ok_and(|link| link == old) {
                    continue;
                }
                if let Err(rollback) = replace_link(&path, old.as_deref()) {
                    failed.push(format!("{}: {rollback}", path.display()));
                }
            }
            let restored = match &bin_previous {
                Some(previous) => std::fs::write(&bin_record, previous),
                None => std::fs::remove_file(&bin_record).or_else(|e| {
                    (e.kind() == std::io::ErrorKind::NotFound)
                        .then_some(())
                        .ok_or(e)
                }),
            };
            if let Err(rollback) = restored {
                failed.push(format!("{}: {rollback}", bin_record.display()));
            }
            ensure!(
                failed.is_empty(),
                "Activation failed ({error}); rollback also failed for {}",
                failed.join("; ")
            );
            return Err(error);
        }
        println!("{}", self.bin.join("tide").display());
        Ok(())
    }
    pub fn install_local(&self, source: &Path) -> Result<()> {
        let _lock = self.lock()?;
        self.owned_launcher()?;
        // Existing marker syntax is validated before any replacement.
        self.marker("current")?;
        self.marker("previous")?;
        let id = self.stage_local(source, env!("CARGO_PKG_VERSION"))?;
        self.activate(&id)
    }
    fn stage_local(&self, source: &Path, version: &str) -> Result<String> {
        use std::os::unix::fs::PermissionsExt;
        let source_hash = hash(source)?;
        ensure!(
            std::fs::metadata(source)?.permissions().mode() & 0o111 != 0,
            "Local binary is not executable"
        );
        let id = format!("local-{source_hash}");
        let release = self.root.join("releases").join(&id);
        if release.exists() || release.is_symlink() {
            verify(&release)?;
            return Ok(id);
        }
        let staging = self
            .root
            .join("releases")
            .join(format!(".staging-{}", std::process::id()));
        std::fs::create_dir(&staging)?;
        let result = (|| {
            std::fs::copy(source, staging.join("tide"))?;
            ensure!(
                hash(&staging.join("tide"))? == source_hash,
                "Local binary changed during copying"
            );
            std::fs::write(
                staging.join("uninstall.sh"),
                include_bytes!("../../../uninstall.sh"),
            )?;
            std::fs::write(staging.join("LICENSE"), include_bytes!("../../../LICENSE"))?;
            std::fs::write(
                staging.join("ATTRIBUTIONS.md"),
                include_bytes!("../../../docs/ATTRIBUTIONS.md"),
            )?;
            let mut payloads = Vec::new();
            for name in ["ATTRIBUTIONS.md", "LICENSE", "tide", "uninstall.sh"] {
                let file = staging.join(name);
                let mode = if matches!(name, "tide" | "uninstall.sh") {
                    0o755
                } else {
                    0o644
                };
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode))?;
                payloads.push(Payload {
                    path: name.into(),
                    sha256: hash(&file)?,
                    mode: format!("{mode:04o}"),
                    size: std::fs::metadata(file)?.len(),
                });
            }
            let manifest = Manifest {
                schema_version: 2,
                product: "Tide".into(),
                version: version.into(),
                source_commit: String::new(),
                target: target(),
                platform: platform().into(),
                architecture: architecture().into(),
                prerelease: false,
                implementation: "rust".into(),
                local: true,
                payloads,
            };
            std::fs::write(
                staging.join("VERSION.json"),
                serde_json::to_vec_pretty(&manifest)?,
            )?;
            verify(&staging)?;
            std::fs::rename(&staging, &release)?;
            Ok(id)
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&staging);
        }
        result
    }
    pub fn activate_existing(&self, id: &str) -> Result<()> {
        ensure!(safe_id(id), "Invalid installed release identity");
        let _lock = self.lock()?;
        self.marker("current")?;
        self.marker("previous")?;
        self.activate(id)
    }

    pub fn install_release(&self, payload: &Path) -> Result<()> {
        let _lock = self.lock()?;
        self.owned_launcher()?;
        self.marker("current")?;
        self.marker("previous")?;
        let manifest = verify(payload)?;
        ensure!(!manifest.local, "Use --binary for a local build");
        let id = manifest.version;
        let release = self.root.join("releases").join(&id);
        if release.exists() || release.is_symlink() {
            verify(&release)?;
            ensure!(
                hash(&release.join("VERSION.json"))? == hash(&payload.join("VERSION.json"))?,
                "Installed version has a different build identity"
            );
        } else {
            let staging = self
                .root
                .join("releases")
                .join(format!(".staging-{}", std::process::id()));
            std::fs::create_dir(&staging)?;
            let result: Result<()> = (|| {
                for name in [
                    "ATTRIBUTIONS.md",
                    "LICENSE",
                    "tide",
                    "uninstall.sh",
                    "VERSION.json",
                ] {
                    std::fs::copy(payload.join(name), staging.join(name))?;
                }
                verify(&staging)?;
                std::fs::rename(&staging, release)?;
                Ok(())
            })();
            if result.is_err() {
                let _ = std::fs::remove_dir_all(staging);
            }
            result?;
        }
        self.activate(&id)
    }
}

pub fn uninstall(purge: bool, yes: bool) -> Result<()> {
    let executable = std::fs::canonicalize(std::env::current_exe()?)?;
    homebrew_refusal(&executable, ManagedCommand::Uninstall)?;
    let release = executable.parent().context("Executable has no parent")?;
    verify(release).context("This executable is not a verified managed installation; use its package manager or remove the source build manually")?;
    let status = tide_core::process::scrub_tide_secrets(&mut std::process::Command::new("/bin/sh"))
        .arg(release.join("uninstall.sh"))
        .args(if purge { vec!["--purge"] } else { vec![] })
        .args(if yes { vec!["--yes"] } else { vec![] })
        .status()?;
    ensure!(status.success(), "Uninstaller did not complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    fn fixture(root: &Path) -> Installation {
        let root = root.canonicalize().unwrap();
        Installation {
            root: root.join("lib/tide"),
            bin: root.join("bin"),
        }
    }
    fn binary(root: &Path, name: &str, content: &str) -> PathBuf {
        let path = root.join(name);
        std::fs::write(&path, content).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
    #[test]
    fn local_upgrades_keep_exact_previous_build_and_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let store = fixture(dir.path());
        let first = binary(dir.path(), "first", "first build");
        let second = binary(dir.path(), "second", "second build");
        store.install_local(&first).unwrap();
        let old = store.marker("current").unwrap().unwrap();
        store.install_local(&second).unwrap();
        let current = store.marker("current").unwrap().unwrap();
        assert_ne!(old, current);
        assert_eq!(
            store.marker("previous").unwrap().as_deref(),
            Some(old.as_str())
        );
        store.install_local(&second).unwrap();
        assert_eq!(
            store.marker("previous").unwrap().as_deref(),
            Some(old.as_str())
        );
        verify(&store.root.join("releases").join(old)).unwrap();
        assert_eq!(
            std::fs::read_link(store.bin.join("tide")).unwrap(),
            store.root.join("current/tide")
        );
    }
    #[test]
    fn foreign_launchers_and_symlinked_stores_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let store = fixture(dir.path());
        std::fs::create_dir_all(&store.bin).unwrap();
        let source = binary(dir.path(), "source", "new build");
        let launcher = binary(&store.bin, "tide", "foreign executable");
        assert!(store.install_local(&source).is_err());
        assert_eq!(
            std::fs::read_to_string(&launcher).unwrap(),
            "foreign executable"
        );
        std::fs::remove_file(launcher).unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::remove_dir_all(&store.root).unwrap();
        symlink(&outside, &store.root).unwrap();
        assert!(store.install_local(&source).is_err());
        assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
    }
    #[test]
    fn changed_payloads_and_escaping_markers_cannot_be_activated() {
        let dir = tempfile::tempdir().unwrap();
        let store = fixture(dir.path());
        let source = binary(dir.path(), "source", "original build");
        store.install_local(&source).unwrap();
        let current = store.marker("current").unwrap().unwrap();
        std::fs::write(
            store.root.join("releases").join(&current).join("tide"),
            "changed",
        )
        .unwrap();
        assert!(store.install_local(&source).is_err());
        assert_eq!(
            store.marker("current").unwrap().as_deref(),
            Some(current.as_str())
        );
        replace_link(
            &store.root.join("previous"),
            Some(Path::new("releases/../../outside")),
        )
        .unwrap();
        assert!(store.install_local(&source).is_err());
    }
    #[test]
    fn stale_locks_from_dead_installs_are_recovered_but_live_ones_block() {
        let dir = tempfile::tempdir().unwrap();
        let store = fixture(dir.path());
        let source = binary(dir.path(), "source", "build");
        let lock = store.root.join("install.lock");
        std::fs::create_dir_all(&lock).unwrap();
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let dead = child.id();
        child.wait().unwrap();
        std::fs::write(lock.join("pid"), dead.to_string()).unwrap();
        store.install_local(&source).unwrap();
        assert!(!lock.exists());
        std::fs::create_dir(&lock).unwrap();
        std::fs::write(lock.join("pid"), std::process::id().to_string()).unwrap();
        assert!(store.install_local(&source).is_err());
        std::fs::remove_file(lock.join("pid")).unwrap();
        assert!(store.install_local(&source).is_err());
    }
    #[test]
    fn failed_activation_restores_links_and_the_bin_dir_record() {
        let dir = tempfile::tempdir().unwrap();
        let store = fixture(dir.path());
        let first = binary(dir.path(), "first", "first build");
        let second = binary(dir.path(), "second", "second build");
        store.install_local(&first).unwrap();
        let current = store.marker("current").unwrap();
        let record = store.root.join("BIN_DIR");
        std::fs::write(&record, "/elsewhere/bin").unwrap();
        std::fs::set_permissions(&store.bin, std::fs::Permissions::from_mode(0o500)).unwrap();
        let result = store.install_local(&second);
        std::fs::set_permissions(&store.bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        let error = result.unwrap_err().to_string();
        assert!(error.contains("activation link"), "{error}");
        assert!(!error.contains("rollback also failed"), "{error}");
        assert_eq!(store.marker("current").unwrap(), current);
        assert_eq!(std::fs::read_to_string(&record).unwrap(), "/elsewhere/bin");
    }
    #[test]
    fn homebrew_kegs_are_recognized_by_their_cellar_path() {
        for (path, formula) in [
            ("/opt/homebrew/Cellar/tide/0.10.0/bin/tide", Some("tide")),
            ("/usr/local/Cellar/tide/0.10.0_1/bin/tide", Some("tide")),
            (
                "/home/linuxbrew/.linuxbrew/Cellar/tide/0.10.0/bin/tide",
                Some("tide"),
            ),
            ("/home/dev/.local/lib/tide/releases/0.10.0/tide", None),
            ("/opt/homebrew/bin/tide", None),
            ("/opt/homebrew/Cellar/tide/0.10.0/libexec/tide", None),
            ("/Cellar/bin/tide", None),
        ] {
            assert_eq!(
                homebrew_formula(Path::new(path)).as_deref(),
                formula,
                "{path}"
            );
        }
    }
    #[test]
    fn homebrew_installs_are_left_to_brew() {
        let keg = Path::new("/opt/homebrew/Cellar/tide/0.10.0/bin/tide");
        let removal = homebrew_refusal(keg, ManagedCommand::Uninstall)
            .unwrap_err()
            .to_string();
        assert!(removal.contains("`brew uninstall tide`"), "{removal}");
        let update = homebrew_refusal(keg, ManagedCommand::Install)
            .unwrap_err()
            .to_string();
        assert!(update.contains("`brew upgrade tide`"), "{update}");
        let managed = Path::new("/home/dev/.local/lib/tide/releases/0.10.0/tide");
        homebrew_refusal(managed, ManagedCommand::Uninstall).unwrap();
        homebrew_refusal(managed, ManagedCommand::Install).unwrap();
    }
    #[test]
    fn manifests_reject_duplicate_fields_symlinks_and_extra_payloads() {
        let dir = tempfile::tempdir().unwrap();
        let store = fixture(dir.path());
        let source = binary(dir.path(), "source", "build");
        store.install_local(&source).unwrap();
        let release = store
            .root
            .join("releases")
            .join(store.marker("current").unwrap().unwrap());
        let manifest = release.join("VERSION.json");
        let original = std::fs::read_to_string(&manifest).unwrap();
        std::fs::write(
            &manifest,
            original.replacen('{', "{\"product\":\"Tide\",", 1),
        )
        .unwrap();
        assert!(verify(&release).is_err());
        std::fs::write(&manifest, original).unwrap();
        std::fs::write(release.join("extra"), "unexpected").unwrap();
        assert!(verify(&release).is_err());
        std::fs::remove_file(release.join("extra")).unwrap();
        std::fs::remove_file(release.join("LICENSE")).unwrap();
        symlink(&source, release.join("LICENSE")).unwrap();
        assert!(verify(&release).is_err());
    }
}
