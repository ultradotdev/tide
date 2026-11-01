//! AI CLI integration and the secrets store for Tide.
//!
//! - `cli`: external AI CLI detection, launch arguments, and session discovery.
//! - `native_cli`: native session creation, renaming, and removal through each CLI.
//! - `secrets`: keychain/env/encrypted-file secret storage.

pub mod cli;
pub mod native_cli;
pub mod secrets;
