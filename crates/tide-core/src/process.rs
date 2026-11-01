//! Environment hygiene and PID conversions for child processes.
//!
//! Tide reads its master password, `TIDE_SECRET_*` values and provider API
//! keys from the environment. Every spawn removes the Tide secrets; spawns that
//! run model-chosen commands also remove the provider keys. Tokio commands can
//! use these through `Command::as_std_mut`, and other builders through
//! [`inherited_vars`].

use std::ffi::{OsStr, OsString};
use std::process::Command;

pub const MASTER_PASSWORD_VAR: &str = "TIDE_MASTER_PASSWORD";
pub const SECRET_VAR_PREFIX: &str = "TIDE_SECRET_";

/// Provider key variables Tide reads, including the unprefixed forms of the
/// `ai.*.api-key` secrets.
pub const PROVIDER_KEY_VARS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "AI_CLAUDE_API_KEY",
    "AI_OPENAI_API_KEY",
    "AI_GEMINI_API_KEY",
];

pub fn is_tide_secret(name: &OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| name == MASTER_PASSWORD_VAR || name.starts_with(SECRET_VAR_PREFIX))
}

pub fn is_provider_key(name: &OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| PROVIDER_KEY_VARS.contains(&name))
}

/// Names in the current process environment that match `predicate`.
pub fn inherited_vars(predicate: impl Fn(&OsStr) -> bool) -> Vec<OsString> {
    std::env::vars_os()
        .map(|(name, _)| name)
        .filter(|name| predicate(name))
        .collect()
}

/// Remove the master password and every `TIDE_SECRET_*` variable. Apply to
/// every child process.
pub fn scrub_tide_secrets(command: &mut Command) -> &mut Command {
    scrub(command, is_tide_secret)
}

/// Remove provider API keys. Apply to commands an AI model can choose, not to
/// terminals or AI CLIs that need the user's environment.
pub fn scrub_provider_keys(command: &mut Command) -> &mut Command {
    scrub(command, is_provider_key)
}

fn scrub(command: &mut Command, predicate: fn(&OsStr) -> bool) -> &mut Command {
    let mut names = inherited_vars(predicate);
    names.extend(
        command
            .get_envs()
            .map(|(name, _)| name.to_os_string())
            .filter(|name| predicate(name)),
    );
    for name in names {
        command.env_remove(name);
    }
    command
}

/// A child PID (`std::process::Child::id`) as a raw `pid_t`.
///
/// `None` for 0 or a value too large for `pid_t`. Neither is a real child PID,
/// and a wrapped value would name an unrelated process.
#[must_use]
pub fn raw_pid(id: u32) -> Option<i32> {
    i32::try_from(id).ok().filter(|pid| *pid > 0)
}

/// The `kill(2)` target that signals the whole process group led by `id`.
///
/// `None` when `id` is not a usable PID: a wrapped value could signal an
/// unrelated group, and 0 would signal Tide's own group.
#[must_use]
pub fn process_group_target(id: u32) -> Option<i32> {
    raw_pid(id).map(|pid| -pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_conversions_reject_values_that_are_not_child_pids() {
        assert_eq!(raw_pid(4242), Some(4242));
        assert_eq!(raw_pid(0), None);
        assert_eq!(raw_pid(u32::MAX), None);
        assert_eq!(process_group_target(4242), Some(-4242));
        assert_eq!(process_group_target(0), None);
        assert_eq!(process_group_target(1 << 31), None);
    }

    fn child_env(command: &mut Command) -> String {
        String::from_utf8(command.output().unwrap().stdout).unwrap()
    }

    #[test]
    fn classifies_secret_and_provider_names() {
        assert!(is_tide_secret(OsStr::new("TIDE_MASTER_PASSWORD")));
        assert!(is_tide_secret(OsStr::new("TIDE_SECRET_DB_PASSWORD")));
        assert!(!is_tide_secret(OsStr::new("TIDE_LOG")));
        assert!(is_provider_key(OsStr::new("OPENAI_API_KEY")));
        assert!(is_provider_key(OsStr::new("AI_GEMINI_API_KEY")));
        assert!(!is_provider_key(OsStr::new("PATH")));
    }

    #[cfg(unix)]
    #[test]
    fn removes_inherited_and_explicit_secrets() {
        temp_env::with_vars(
            [
                ("TIDE_MASTER_PASSWORD", Some("master")),
                ("TIDE_SECRET_AI_OPENAI_API_KEY", Some("prefixed")),
                ("OPENAI_API_KEY", Some("provider")),
                ("TIDE_SCRUB_VISIBLE", Some("visible")),
            ],
            || {
                let mut command = Command::new("env");
                command.env("TIDE_SECRET_EXPLICIT", "explicit");
                let env = child_env(scrub_tide_secrets(&mut command));
                assert!(!env.contains("master"));
                assert!(!env.contains("prefixed"));
                assert!(!env.contains("explicit"));
                assert!(env.contains("OPENAI_API_KEY=provider"));
                assert!(env.contains("TIDE_SCRUB_VISIBLE=visible"));

                let mut command = Command::new("env");
                scrub_tide_secrets(&mut command);
                let env = child_env(scrub_provider_keys(&mut command));
                assert!(!env.contains("provider"));
                assert!(!env.contains("master"));
                assert!(env.contains("TIDE_SCRUB_VISIBLE=visible"));
            },
        );
    }
}
