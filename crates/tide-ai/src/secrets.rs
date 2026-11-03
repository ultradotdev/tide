//! Secret storage.
//!
//! Providers are queried in priority order: system keychain, read-only
//! environment variables, then the encrypted file fallback. The encrypted file
//! is a version 2 JSON envelope: scrypt (N=2^17, recorded in the header) and
//! AES-256-GCM.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const MASTER_PASSWORD_ENV: &str = "TIDE_MASTER_PASSWORD";
const ENV_PREFIX: &str = "TIDE_SECRET_";
const KEYCHAIN_SERVICE: &str = "tide";

/// scrypt cost (log2 N) of every envelope.
const SCRYPT_LOG_N: u8 = 17;

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("encrypted secret store is locked")]
    Locked,
    #[error("unsupported encrypted secret-store format")]
    UnsupportedFormat,
    #[error("invalid encrypted secret-store payload")]
    InvalidPayload,
    #[error("unable to unlock encrypted secret store: {0}")]
    Unlock(String),
    #[error("no writable secret provider is available")]
    NoWritableProvider,
    #[error("provider is read-only: {0}")]
    ReadOnly(String),
    #[error("secret storage error: {0}")]
    Io(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretOptions {
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredSecret {
    #[serde(
        serialize_with = "serialize_secret",
        deserialize_with = "deserialize_secret"
    )]
    pub value: Zeroizing<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl std::fmt::Debug for StoredSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredSecret")
            .field("value", &"<redacted>")
            .field("created_at", &self.created_at)
            .field("expires_at", &self.expires_at)
            .field("description", &self.description)
            .finish()
    }
}

fn serialize_secret<S: serde::Serializer>(
    value: &Zeroizing<String>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(value)
}

fn deserialize_secret<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Zeroizing<String>, D::Error> {
    String::deserialize(deserializer).map(Zeroizing::new)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretInfo {
    pub key: String,
    pub provider: String,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlaintextStore {
    version: u32,
    secrets: BTreeMap<String, StoredSecret>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EnvelopeV2 {
    version: u32,
    kdf: KdfDescriptor,
    cipher: CipherDescriptor,
    ciphertext: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KdfDescriptor {
    name: String,
    salt: String,
    #[serde(rename = "keyLength")]
    key_length: u32,
    /// scrypt N, always 2^17. Any other cost is rejected before deriving, so a
    /// crafted header cannot demand unbounded memory.
    cost: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CipherDescriptor {
    name: String,
    iv: String,
    #[serde(rename = "authTag")]
    auth_tag: String,
}

/// A secret backend.
pub trait SecretProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn is_read_only(&self) -> bool;
    fn is_available(&self) -> bool;
    fn get(&self, key: &str) -> Result<Option<String>, SecretError>;
    fn set(&self, key: &str, value: &str, options: &SecretOptions) -> Result<(), SecretError>;
    fn delete(&self, key: &str) -> Result<bool, SecretError>;
    fn list(&self) -> Result<Vec<String>, SecretError>;
    fn as_any(&self) -> &dyn std::any::Any;

    fn has(&self, key: &str) -> Result<bool, SecretError> {
        Ok(self.get(key)?.is_some())
    }

    fn info(&self, _key: &str) -> Result<Option<SecretInfo>, SecretError> {
        Ok(None)
    }
}

/// Priority-ordered secret service.
pub struct SecretService {
    providers: Vec<Box<dyn SecretProvider>>,
}

impl SecretService {
    pub fn new(providers: Vec<Box<dyn SecretProvider>>) -> Self {
        Self { providers }
    }

    /// Default service: keychain, environment, encrypted file.
    pub fn local(secrets_file: PathBuf) -> Self {
        let mut encrypted = EncryptedFileStore::new(secrets_file);
        let _ = encrypted.init_from_env();
        Self::new(vec![
            Box::new(KeychainStore::default()),
            Box::new(EnvStore::default()),
            Box::new(encrypted),
        ])
    }

    pub fn encrypted_only(secrets_file: PathBuf) -> Self {
        let mut encrypted = EncryptedFileStore::new(secrets_file);
        let _ = encrypted.init_from_env();
        Self::new(vec![Box::new(encrypted)])
    }

    pub fn get(&self, key: &str) -> Option<String> {
        for provider in &self.providers {
            if !provider.is_available() {
                continue;
            }
            if let Ok(Some(value)) = provider.get(key) {
                return Some(value);
            }
        }
        None
    }

    pub fn set(
        &self,
        key: &str,
        value: &str,
        options: &SecretOptions,
    ) -> Result<String, SecretError> {
        for provider in &self.providers {
            if provider.is_read_only() || !provider.is_available() {
                continue;
            }
            provider.set(key, value, options)?;
            return Ok(provider.id().to_owned());
        }
        Err(SecretError::NoWritableProvider)
    }

    pub fn delete(&self, key: &str) -> Result<bool, SecretError> {
        let mut deleted = false;
        for provider in &self.providers {
            if provider.is_read_only() || !provider.is_available() {
                continue;
            }
            deleted |= provider.delete(key)?;
        }
        Ok(deleted)
    }

    pub fn list(&self, prefix: Option<&str>) -> Vec<String> {
        let mut keys = BTreeMap::new();
        for provider in &self.providers {
            if !provider.is_available() {
                continue;
            }
            if let Ok(list) = provider.list() {
                for key in list {
                    keys.insert(key, ());
                }
            }
        }
        let mut keys = keys.into_keys().collect::<Vec<_>>();
        if let Some(prefix) = prefix {
            keys.retain(|key| key.starts_with(prefix));
        }
        keys
    }

    pub fn info(&self, key: &str) -> Option<SecretInfo> {
        for provider in &self.providers {
            if !provider.is_available() {
                continue;
            }
            if let Ok(info) = provider.info(key)
                && info.is_some()
            {
                return info;
            }
        }
        None
    }

    /// Unlock the encrypted file provider (no-op when absent).
    pub fn unlock(&self, password: &str) -> Result<(), SecretError> {
        if let Some(store) = self
            .providers
            .iter()
            .find_map(|provider| provider.as_any().downcast_ref::<EncryptedFileStore>())
        {
            return store.unlock(password);
        }
        Ok(())
    }

    pub fn is_unlocked(&self) -> bool {
        self.providers
            .iter()
            .any(|provider| provider.id() == "encrypted-file" && provider.is_available())
    }
}

/// Read-only environment variable provider.
pub struct EnvStore {
    prefix: String,
    check_without_prefix: bool,
}

impl Default for EnvStore {
    fn default() -> Self {
        Self {
            prefix: ENV_PREFIX.to_owned(),
            check_without_prefix: true,
        }
    }
}

fn key_to_env_name(key: &str, prefix: &str) -> String {
    let name = key.replace(['.', '-'], "_").to_uppercase();
    format!("{prefix}{name}")
}

impl SecretProvider for EnvStore {
    fn id(&self) -> &'static str {
        "env"
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_available(&self) -> bool {
        true
    }

    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        let prefixed = key_to_env_name(key, &self.prefix);
        if let Ok(value) = std::env::var(&prefixed) {
            return Ok(Some(value));
        }
        if self.check_without_prefix {
            let direct = key_to_env_name(key, "");
            if let Ok(value) = std::env::var(&direct) {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    fn set(&self, _key: &str, _value: &str, _options: &SecretOptions) -> Result<(), SecretError> {
        Err(SecretError::ReadOnly(self.id().to_owned()))
    }

    fn delete(&self, _key: &str) -> Result<bool, SecretError> {
        Err(SecretError::ReadOnly(self.id().to_owned()))
    }

    fn list(&self) -> Result<Vec<String>, SecretError> {
        let mut keys = Vec::new();
        for (name, _) in std::env::vars() {
            if let Some(stripped) = name.strip_prefix(&self.prefix) {
                keys.push(stripped.to_lowercase().replace('_', "."));
            }
        }
        Ok(keys)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// System keychain provider. macOS uses the Security framework in-process, so
/// no secret ever reaches a child's argv; elsewhere `secret-tool` is run from a
/// fixed absolute path with the secret on stdin.
pub struct KeychainStore {
    service: String,
}

impl Default for KeychainStore {
    fn default() -> Self {
        Self::with_service(KEYCHAIN_SERVICE)
    }
}

impl KeychainStore {
    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }
}

impl SecretProvider for KeychainStore {
    fn id(&self) -> &'static str {
        "keychain"
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_available(&self) -> bool {
        keychain::available()
    }

    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        keychain::get(&self.service, key)
    }

    fn set(&self, key: &str, value: &str, _options: &SecretOptions) -> Result<(), SecretError> {
        keychain::set(&self.service, key, value)
    }

    fn delete(&self, key: &str) -> Result<bool, SecretError> {
        keychain::delete(&self.service, key)
    }

    fn list(&self) -> Result<Vec<String>, SecretError> {
        Ok(Vec::new())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(target_os = "macos")]
mod keychain {
    use security_framework::passwords::{
        PasswordOptions, delete_generic_password, generic_password, set_generic_password,
    };
    use zeroize::Zeroizing;

    use super::SecretError;

    const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

    pub(super) fn available() -> bool {
        true
    }

    pub(super) fn get(service: &str, key: &str) -> Result<Option<String>, SecretError> {
        match generic_password(PasswordOptions::new_generic_password(service, key)) {
            Ok(bytes) => {
                let bytes = Zeroizing::new(bytes);
                Ok(std::str::from_utf8(&bytes)
                    .ok()
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned))
            }
            Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(error) => Err(SecretError::Io(error.to_string())),
        }
    }

    pub(super) fn set(service: &str, key: &str, value: &str) -> Result<(), SecretError> {
        set_generic_password(service, key, value.as_bytes())
            .map_err(|error| SecretError::Io(error.to_string()))
    }

    pub(super) fn delete(service: &str, key: &str) -> Result<bool, SecretError> {
        match delete_generic_password(service, key) {
            Ok(()) => Ok(true),
            Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(false),
            Err(error) => Err(SecretError::Io(error.to_string())),
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod keychain {
    use std::process::Stdio;

    use super::{SecretError, SecretToolAction, secret_tool_command, secret_tool_path};

    pub(super) fn available() -> bool {
        secret_tool_path().is_some()
    }

    pub(super) fn get(service: &str, key: &str) -> Result<Option<String>, SecretError> {
        let Some(tool) = secret_tool_path() else {
            return Ok(None);
        };
        let (mut command, _) = secret_tool_command(tool, service, key, &SecretToolAction::Lookup);
        let output = command
            .stderr(Stdio::null())
            .output()
            .map_err(|error| SecretError::Io(error.to_string()))?;
        if !output.status.success() {
            return Ok(None);
        }
        let stdout = zeroize::Zeroizing::new(output.stdout);
        let value = String::from_utf8_lossy(&stdout).trim().to_owned();
        Ok((!value.is_empty()).then_some(value))
    }

    pub(super) fn set(service: &str, key: &str, value: &str) -> Result<(), SecretError> {
        use std::io::Write as _;
        let tool = secret_tool_path()
            .ok_or_else(|| SecretError::Io("no keychain tool available".to_owned()))?;
        let (mut command, payload) =
            secret_tool_command(tool, service, key, &SecretToolAction::Store(value));
        let mut child = command
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| SecretError::Io(error.to_string()))?;
        let written = child
            .stdin
            .take()
            .zip(payload)
            .map(|(mut stdin, payload)| stdin.write_all(payload.as_bytes()));
        let status = child
            .wait()
            .map_err(|error| SecretError::Io(error.to_string()))?;
        if let Some(Err(error)) = written {
            return Err(SecretError::Io(error.to_string()));
        }
        if status.success() {
            Ok(())
        } else {
            Err(SecretError::Io(format!("keychain exited with {status}")))
        }
    }

    pub(super) fn delete(service: &str, key: &str) -> Result<bool, SecretError> {
        let Some(tool) = secret_tool_path() else {
            return Ok(false);
        };
        let (mut command, _) = secret_tool_command(tool, service, key, &SecretToolAction::Clear);
        let status = command.stdout(Stdio::null()).stderr(Stdio::null()).status();
        Ok(status.is_ok_and(|status| status.success()))
    }
}

/// Trusted locations for `secret-tool`; `PATH` is never consulted.
#[cfg(any(not(target_os = "macos"), test))]
const SECRET_TOOL_PATHS: [&str; 2] = ["/usr/bin/secret-tool", "/bin/secret-tool"];

#[cfg(any(not(target_os = "macos"), test))]
fn secret_tool_path() -> Option<&'static Path> {
    SECRET_TOOL_PATHS
        .iter()
        .map(Path::new)
        .find(|path| path.is_file())
}

#[cfg(any(not(target_os = "macos"), test))]
enum SecretToolAction<'a> {
    Lookup,
    Store(&'a str),
    Clear,
}

/// Build a `secret-tool` invocation and the bytes the caller must write to its
/// stdin. A stored value only ever travels on stdin, never in the argv.
#[cfg(any(not(target_os = "macos"), test))]
fn secret_tool_command<'a>(
    tool: &Path,
    service: &str,
    key: &str,
    action: &SecretToolAction<'a>,
) -> (std::process::Command, Option<&'a str>) {
    let mut command = std::process::Command::new(tool);
    tide_core::process::scrub_tide_secrets(&mut command);
    let stdin = match action {
        SecretToolAction::Lookup => {
            command.args(["lookup", "service", service, "account", key]);
            None
        }
        SecretToolAction::Store(value) => {
            command.args([
                "store",
                "--label",
                "Tide secret",
                "service",
                service,
                "account",
                key,
            ]);
            Some(*value)
        }
        SecretToolAction::Clear => {
            command.args(["clear", "service", service, "account", key]);
            None
        }
    };
    command.stdin(if stdin.is_some() {
        std::process::Stdio::piped()
    } else {
        std::process::Stdio::null()
    });
    (command, stdin)
}

struct EncryptedInner {
    password: Option<Zeroizing<String>>,
    cache: Option<BTreeMap<String, StoredSecret>>,
}

/// AES-256-GCM encrypted file store.
pub struct EncryptedFileStore {
    path: PathBuf,
    inner: Mutex<EncryptedInner>,
}

impl EncryptedFileStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            inner: Mutex::new(EncryptedInner {
                password: None,
                cache: None,
            }),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn init_from_env(&mut self) -> Result<(), SecretError> {
        if let Ok(password) = std::env::var(MASTER_PASSWORD_ENV).map(Zeroizing::new)
            && !password.trim().is_empty()
        {
            self.unlock(&password)?;
        }
        Ok(())
    }

    pub fn unlock(&self, password: &str) -> Result<(), SecretError> {
        if password.trim().is_empty() {
            return Err(SecretError::Unlock("master password is empty".to_owned()));
        }
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.password = Some(Zeroizing::new(password.to_owned()));
        match self.load_locked(&mut inner) {
            Ok(()) => Ok(()),
            Err(error) => {
                inner.password = None;
                inner.cache = None;
                Err(SecretError::Unlock(error.to_string()))
            }
        }
    }

    pub fn lock(&mut self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.password = None;
        inner.cache = None;
    }

    fn load_locked(&self, inner: &mut EncryptedInner) -> Result<(), SecretError> {
        if !self.path.exists() {
            inner.cache = Some(BTreeMap::new());
            return Ok(());
        }
        let bytes =
            std::fs::read(&self.path).map_err(|error| SecretError::Io(error.to_string()))?;
        let password = inner.password.as_ref().ok_or(SecretError::Locked)?;
        inner.cache = Some(decrypt_v2(&bytes, password)?.secrets);
        Ok(())
    }

    fn save_locked(&self, inner: &mut EncryptedInner) -> Result<(), SecretError> {
        let Some(cache) = inner.cache.as_ref() else {
            return Err(SecretError::Locked);
        };
        let plaintext = PlaintextStore {
            version: 2,
            secrets: cache.clone(),
        };
        let Some(password) = inner.password.as_ref() else {
            return Err(SecretError::Locked);
        };
        let envelope = encrypt_v2(&plaintext, password)?;
        let bytes =
            serde_json::to_vec(&envelope).map_err(|error| SecretError::Io(error.to_string()))?;
        if let Some(parent) = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|error| SecretError::Io(error.to_string()))?;
            set_mode(parent, 0o700);
        }
        write_private_atomic(&self.path, &bytes).map_err(|error| SecretError::Io(error.to_string()))
    }

    pub fn purge_expired(&self) -> Result<(), SecretError> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        self.purge_expired_locked(&mut inner)
    }

    fn purge_expired_locked(&self, inner: &mut EncryptedInner) -> Result<(), SecretError> {
        let now = chrono::Utc::now();
        let expired = inner
            .cache
            .as_ref()
            .map(|cache| {
                cache
                    .iter()
                    .filter(|(_, secret)| is_expired(secret.expires_at.as_deref(), now))
                    .map(|(key, _)| key.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if expired.is_empty() {
            return Ok(());
        }
        if let Some(cache) = inner.cache.as_mut() {
            for key in expired {
                cache.remove(&key);
            }
        }
        self.save_locked(inner)
    }
}

fn is_expired(expires_at: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> bool {
    expires_at.is_some_and(|expires| {
        chrono::DateTime::parse_from_rfc3339(expires).is_ok_and(|expires| expires < now)
    })
}

impl SecretProvider for EncryptedFileStore {
    fn id(&self) -> &'static str {
        "encrypted-file"
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_available(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cache
            .is_some()
    }

    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let cache = inner.cache.as_ref().ok_or(SecretError::Locked)?;
        let Some(secret) = cache.get(key) else {
            return Ok(None);
        };
        if is_expired(secret.expires_at.as_deref(), chrono::Utc::now()) {
            return Ok(None);
        }
        Ok(Some(secret.value.as_str().to_owned()))
    }

    fn set(&self, key: &str, value: &str, options: &SecretOptions) -> Result<(), SecretError> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.cache.is_none() {
            return Err(SecretError::Locked);
        }
        if let Some(cache) = inner.cache.as_mut() {
            cache.insert(
                key.to_owned(),
                StoredSecret {
                    value: Zeroizing::new(value.to_owned()),
                    created_at: chrono::Utc::now().to_rfc3339(),
                    expires_at: options.expires_at.clone(),
                    description: options.description.clone(),
                },
            );
        }
        self.save_locked(&mut inner)
    }

    fn delete(&self, key: &str) -> Result<bool, SecretError> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.cache.is_none() {
            return Err(SecretError::Locked);
        }
        let deleted = inner
            .cache
            .as_mut()
            .is_some_and(|cache| cache.remove(key).is_some());
        if deleted {
            self.save_locked(&mut inner)?;
        }
        Ok(deleted)
    }

    fn list(&self) -> Result<Vec<String>, SecretError> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        Ok(inner
            .cache
            .as_ref()
            .ok_or(SecretError::Locked)?
            .keys()
            .cloned()
            .collect())
    }

    fn info(&self, key: &str) -> Result<Option<SecretInfo>, SecretError> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let cache = inner.cache.as_ref().ok_or(SecretError::Locked)?;
        Ok(cache.get(key).map(|secret| SecretInfo {
            key: key.to_owned(),
            provider: self.id().to_owned(),
            created_at: secret.created_at.clone(),
            expires_at: secret.expires_at.clone(),
            description: secret.description.clone(),
        }))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn encrypt_v2(store: &PlaintextStore, password: &str) -> Result<EnvelopeV2, SecretError> {
    let mut salt = [0_u8; 16];
    let mut iv = [0_u8; 12];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut iv);
    let key = derive_key(password, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(key.as_slice())
        .map_err(|_| SecretError::Io("encryption failed".to_owned()))?;
    let plaintext = Zeroizing::new(
        serde_json::to_vec(store).map_err(|error| SecretError::Io(error.to_string()))?,
    );
    let ciphertext = cipher
        .encrypt(&Nonce::from(iv), plaintext.as_slice())
        .map_err(|_| SecretError::Io("encryption failed".to_owned()))?;
    let (ciphertext, auth_tag) = ciphertext.split_at(ciphertext.len() - 16);
    Ok(EnvelopeV2 {
        version: 2,
        kdf: KdfDescriptor {
            name: "scrypt".to_owned(),
            salt: BASE64.encode(salt),
            key_length: 32,
            cost: 1 << SCRYPT_LOG_N,
        },
        cipher: CipherDescriptor {
            name: "aes-256-gcm".to_owned(),
            iv: BASE64.encode(iv),
            auth_tag: BASE64.encode(auth_tag),
        },
        ciphertext: BASE64.encode(ciphertext),
    })
}

fn decrypt_v2(bytes: &[u8], password: &str) -> Result<PlaintextStore, SecretError> {
    let envelope: EnvelopeV2 =
        serde_json::from_slice(bytes).map_err(|_| SecretError::UnsupportedFormat)?;
    if envelope.version != 2
        || envelope.kdf.name != "scrypt"
        || envelope.kdf.key_length != 32
        || envelope.kdf.cost != 1 << SCRYPT_LOG_N
        || envelope.cipher.name != "aes-256-gcm"
    {
        return Err(SecretError::UnsupportedFormat);
    }
    let salt = BASE64
        .decode(&envelope.kdf.salt)
        .map_err(|_| SecretError::UnsupportedFormat)?;
    let iv = BASE64
        .decode(&envelope.cipher.iv)
        .map_err(|_| SecretError::UnsupportedFormat)?;
    let auth_tag = BASE64
        .decode(&envelope.cipher.auth_tag)
        .map_err(|_| SecretError::UnsupportedFormat)?;
    let mut ciphertext = BASE64
        .decode(&envelope.ciphertext)
        .map_err(|_| SecretError::UnsupportedFormat)?;
    let nonce = Nonce::try_from(iv.as_slice()).map_err(|_| SecretError::UnsupportedFormat)?;
    let key = derive_key(password, &salt)?;
    ciphertext.extend_from_slice(&auth_tag);
    let cipher = Aes256Gcm::new_from_slice(key.as_slice())
        .map_err(|_| SecretError::Unlock("decryption failed".to_owned()))?;
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(&nonce, ciphertext.as_slice())
            .map_err(|_| SecretError::Unlock("decryption failed".to_owned()))?,
    );
    let store: PlaintextStore =
        serde_json::from_slice(&plaintext).map_err(|_| SecretError::InvalidPayload)?;
    if store.version != 2 {
        return Err(SecretError::InvalidPayload);
    }
    Ok(store)
}

fn derive_key(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, SecretError> {
    let params = scrypt::Params::new(SCRYPT_LOG_N, 8, 1)
        .map_err(|error| SecretError::Io(error.to_string()))?;
    let mut key = Zeroizing::new([0_u8; 32]);
    scrypt::scrypt(password.as_bytes(), salt, &params, key.as_mut_slice())
        .map_err(|error| SecretError::Io(error.to_string()))?;
    Ok(key)
}

/// Atomically replace `path` with `bytes`. The temp file is created 0600 with
/// `create_new`, so it is never readable by others and never follows a
/// pre-existing link; data and the directory entry are synced before return.
fn write_private_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    fn finish(
        mut file: std::fs::File,
        temp: &Path,
        path: &Path,
        bytes: &[u8],
    ) -> std::io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(temp, path)?;
        sync_dir(parent_dir(path))
    }
    let (temp, file) = create_private_temp(path)?;
    let result = finish(file, &temp, path, bytes);
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn parent_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn create_private_temp(path: &Path) -> std::io::Result<(PathBuf, std::fs::File)> {
    let name = path
        .file_name()
        .map_or_else(|| "secrets".into(), |name| name.to_string_lossy());
    for _ in 0..16 {
        let mut suffix = [0_u8; 8];
        rand::thread_rng().fill_bytes(&mut suffix);
        let suffix = suffix
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let temp = parent_dir(path).join(format!(".{name}.{suffix}.tmp"));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        match options.open(&temp) {
            Ok(file) => return Ok((temp, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a unique secrets temp file",
    ))
}

fn sync_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::fs::File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

fn set_mode(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, EncryptedFileStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = EncryptedFileStore::new(dir.path().join("secrets.enc"));
        store.unlock("correct horse battery staple").unwrap();
        (dir, store)
    }

    #[test]
    fn stores_and_lists_secrets_round_trip() {
        let (dir, store) = store();
        store
            .set("api.token", "token-test", &SecretOptions::default())
            .unwrap();
        store
            .set(
                "db.password",
                "hunter2",
                &SecretOptions {
                    description: Some("dev".to_owned()),
                    ..SecretOptions::default()
                },
            )
            .unwrap();
        assert_eq!(
            store.get("api.token").unwrap().as_deref(),
            Some("token-test")
        );
        assert_eq!(store.list().unwrap().len(), 2);
        assert_eq!(
            store
                .info("db.password")
                .unwrap()
                .unwrap()
                .description
                .as_deref(),
            Some("dev")
        );

        let reopened = EncryptedFileStore::new(dir.path().join("secrets.enc"));
        reopened.unlock("correct horse battery staple").unwrap();
        assert_eq!(
            reopened.get("db.password").unwrap().as_deref(),
            Some("hunter2")
        );
    }

    #[test]
    fn rejects_the_wrong_password() {
        let (dir, store) = store();
        store.set("k", "v", &SecretOptions::default()).unwrap();
        let reopened = EncryptedFileStore::new(dir.path().join("secrets.enc"));
        assert!(reopened.unlock("wrong").is_err());
        assert!(!reopened.is_available());
    }

    #[test]
    fn writes_the_v2_envelope_shape() {
        let (dir, store) = store();
        store.set("k", "v", &SecretOptions::default()).unwrap();
        let bytes = std::fs::read(dir.path().join("secrets.enc")).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["version"], 2);
        assert_eq!(value["kdf"]["name"], "scrypt");
        assert_eq!(value["kdf"]["keyLength"], 32);
        assert_eq!(value["kdf"]["cost"], 131_072);
        assert_eq!(value["cipher"]["name"], "aes-256-gcm");
        assert!(value["cipher"]["authTag"].is_string());
    }

    #[test]
    fn missing_or_unrecognised_scrypt_costs_are_rejected_without_deriving() {
        let (dir, store) = store();
        store.set("k", "v", &SecretOptions::default()).unwrap();
        let path = dir.path().join("secrets.enc");
        let envelope: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for cost in [Some(1_u64 << 14), Some(1_u64 << 30), None] {
            let mut changed = envelope.clone();
            match cost {
                Some(cost) => changed["kdf"]["cost"] = serde_json::json!(cost),
                None => {
                    changed["kdf"].as_object_mut().unwrap().remove("cost");
                }
            }
            assert!(matches!(
                decrypt_v2(
                    &serde_json::to_vec(&changed).unwrap(),
                    "correct horse battery staple"
                ),
                Err(SecretError::UnsupportedFormat)
            ));
        }
    }

    /// Written by Tide 0.11.1 (aes-gcm 0.10), so a crate upgrade that changed the
    /// envelope or cipher output would show up here before it locked anyone out.
    const SECRETS_WRITTEN_BY_0_11_1: &str = r#"{"version":2,"kdf":{"name":"scrypt","salt":"LcySElFV7VTrlvzTDlU4tQ==","keyLength":32,"cost":131072},"cipher":{"name":"aes-256-gcm","iv":"MSIljytlWl7cl4EX","authTag":"3RhPpu0HSdByOoXMtiPnIg=="},"ciphertext":"rN6Kq4IQayOKHRMzfD+y+hBO0z8U2FLjFggdPEmnYe8PK3nvhsRcq0u/fBeEr6f0/s3RmwWyfihkfYUByW2GTm3aWUSJRF3h4sSiRt4pD2Yma734cUwPPCATxW8yjNqoAg042n/Rt0Jho/M="}"#;

    #[test]
    fn opens_a_secret_store_written_by_an_earlier_release() {
        let store = decrypt_v2(
            SECRETS_WRITTEN_BY_0_11_1.as_bytes(),
            "correct horse battery staple",
        )
        .unwrap();
        assert_eq!(store.secrets["api.token"].value.as_str(), "token-test");
        assert!(matches!(
            decrypt_v2(SECRETS_WRITTEN_BY_0_11_1.as_bytes(), "wrong"),
            Err(SecretError::Unlock(_))
        ));
    }

    #[test]
    fn a_store_with_a_malformed_iv_is_rejected_not_a_crash() {
        let mut envelope: serde_json::Value =
            serde_json::from_str(SECRETS_WRITTEN_BY_0_11_1).unwrap();
        envelope["cipher"]["iv"] = serde_json::json!(BASE64.encode([0_u8; 5]));
        assert!(matches!(
            decrypt_v2(
                &serde_json::to_vec(&envelope).unwrap(),
                "correct horse battery staple"
            ),
            Err(SecretError::UnsupportedFormat)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn secrets_temp_file_is_private_from_creation() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("secrets.enc");
        let (temp, _file) = create_private_temp(&target).unwrap();
        assert_eq!(
            std::fs::metadata(&temp).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(temp.parent(), Some(dir.path()));
        std::fs::remove_file(temp).unwrap();

        let store = EncryptedFileStore::new(&target);
        store.unlock("pw").unwrap();
        store.set("k", "v", &SecretOptions::default()).unwrap();
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let entries = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec![std::ffi::OsString::from("secrets.enc")]);
    }

    #[test]
    fn debug_output_never_contains_secret_values() {
        let secret = StoredSecret {
            value: Zeroizing::new("sk-super-secret".to_owned()),
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            expires_at: None,
            description: Some("dev".to_owned()),
        };
        let store = PlaintextStore {
            version: 2,
            secrets: BTreeMap::from([("k".to_owned(), secret.clone())]),
        };
        for text in [format!("{secret:?}"), format!("{store:?}")] {
            assert!(!text.contains("sk-super-secret"), "{text}");
            assert!(text.contains("<redacted>"));
        }
        let json = serde_json::to_string(&secret).unwrap();
        assert!(json.contains("\"value\":\"sk-super-secret\""));
        let round_trip: StoredSecret = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip, secret);
    }

    #[test]
    fn secret_tool_receives_the_secret_on_stdin_not_argv() {
        let tool = Path::new(SECRET_TOOL_PATHS[0]);
        for (action, expected_stdin) in [
            (
                SecretToolAction::Store("sk-argv-leak"),
                Some("sk-argv-leak"),
            ),
            (SecretToolAction::Lookup, None),
            (SecretToolAction::Clear, None),
        ] {
            let (command, stdin) = secret_tool_command(tool, "tide", "db.local.password", &action);
            assert_eq!(stdin, expected_stdin);
            assert!(Path::new(command.get_program()).is_absolute());
            let args = command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert!(args.iter().all(|arg| !arg.contains("sk-argv-leak")));
            assert!(args.contains(&"db.local.password".to_owned()));
        }
    }

    #[test]
    fn secret_tool_is_never_resolved_through_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("secret-tool"), "#!/bin/sh\n").unwrap();
        temp_env::with_var("PATH", Some(dir.path()), || {
            let resolved = secret_tool_path();
            assert!(resolved.is_none_or(|path| {
                SECRET_TOOL_PATHS
                    .iter()
                    .any(|trusted| Path::new(trusted) == path)
            }));
        });
        assert!(
            SECRET_TOOL_PATHS
                .iter()
                .all(|path| Path::new(path).is_absolute())
        );
    }

    /// Touches the real system keychain, so it only runs when
    /// `TIDE_KEYCHAIN_INTEGRATION=1` (set on the macOS CI job).
    #[test]
    fn system_keychain_round_trip_when_enabled() {
        if std::env::var("TIDE_KEYCHAIN_INTEGRATION").as_deref() != Ok("1") {
            return;
        }
        let store =
            KeychainStore::with_service(format!("tide-integration-{}", crate::cli::new_uuid()));
        assert!(store.is_available());
        assert_eq!(store.get("probe").unwrap(), None);
        store
            .set("probe", "value with spaces", &SecretOptions::default())
            .unwrap();
        store
            .set("probe", "updated-value", &SecretOptions::default())
            .unwrap();
        assert_eq!(
            store.get("probe").unwrap().as_deref(),
            Some("updated-value")
        );
        assert!(store.delete("probe").unwrap());
        assert!(!store.delete("probe").unwrap());
        assert_eq!(store.get("probe").unwrap(), None);
    }

    #[test]
    fn expired_secrets_are_not_returned() {
        let (_dir, store) = store();
        store
            .set(
                "old",
                "value",
                &SecretOptions {
                    expires_at: Some("2000-01-01T00:00:00Z".to_owned()),
                    ..SecretOptions::default()
                },
            )
            .unwrap();
        assert_eq!(store.get("old").unwrap(), None);
        store.purge_expired().unwrap();
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn env_provider_maps_dotted_keys() {
        temp_env::with_var("TIDE_SECRET_DB_LOCAL_PASSWORD", Some("env-secret"), || {
            let store = EnvStore::default();
            assert_eq!(
                store.get("db.local.password").unwrap().as_deref(),
                Some("env-secret")
            );
        });
    }

    #[test]
    fn service_prefers_environment_over_encrypted_file() {
        temp_env::with_var("TIDE_SECRET_DB_LOCAL_PASSWORD", Some("env-secret"), || {
            let dir = tempfile::tempdir().unwrap();
            let encrypted = EncryptedFileStore::new(dir.path().join("s.enc"));
            encrypted.unlock("pw").unwrap();
            encrypted
                .set(
                    "db.local.password",
                    "file-secret",
                    &SecretOptions::default(),
                )
                .unwrap();
            let service =
                SecretService::new(vec![Box::new(EnvStore::default()), Box::new(encrypted)]);
            assert_eq!(
                service.get("db.local.password").as_deref(),
                Some("env-secret")
            );
        });
    }
}
