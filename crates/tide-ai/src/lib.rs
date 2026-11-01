//! AI foundation for Tide.
//!
//! - `secrets`: keychain/env/encrypted-file secret storage.
//! - `models`: embedded model catalog and preferences.
//! - `provider`: streaming adapters for Claude, OpenAI, Gemini, and Ollama.
//! - `chat`: provider-neutral durable chat sessions.
//! - `cli`: external AI CLI detection, launch arguments, and session discovery.

pub mod chat;
pub mod cli;
pub mod models;
pub mod native_cli;
pub mod provider;
pub mod secrets;
