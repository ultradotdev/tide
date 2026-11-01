//! Compatibility checks against files produced by the TypeScript runtime.

use tide_ai::secrets::{EncryptedFileStore, SECRET_KEY_OPENAI, SecretProvider as _};

#[test]
fn reads_a_node_generated_v2_envelope() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.enc");
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/secrets-v2.json"
        ),
        &path,
    )
    .unwrap();
    let store = EncryptedFileStore::new(&path);
    store.unlock("tide-test-password").unwrap();
    assert_eq!(
        store.get(SECRET_KEY_OPENAI).unwrap().as_deref(),
        Some("sk-test")
    );
}
