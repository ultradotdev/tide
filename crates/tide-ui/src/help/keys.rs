//! Effective keybindings and their display names.
use std::collections::HashMap;

use tide_config::Keybinding;

use crate::app::{canonical_key, command_supported};
use crate::command_aliases::canonical;

/// Bindings that resolution can still reach: the last entry for each key and
/// context, without removals, in catalog order.
pub(crate) fn effective(bindings: &[Keybinding]) -> Vec<&Keybinding> {
    let mut last = HashMap::new();
    for (index, binding) in bindings.iter().enumerate() {
        last.insert((canonical_key(&binding.key), binding.when.clone()), index);
    }
    bindings
        .iter()
        .enumerate()
        .filter(|(index, binding)| {
            last.get(&(canonical_key(&binding.key), binding.when.clone())) == Some(index)
                && command_supported(&binding.command)
        })
        .map(|(_, binding)| binding)
        .collect()
}

fn is_user(binding: &Keybinding, user: &[Keybinding]) -> bool {
    let key = canonical_key(&binding.key);
    user.iter().any(|candidate| {
        canonical_key(&candidate.key) == key
            && candidate.when == binding.when
            && candidate.command == binding.command
    })
}

fn has_meta(binding: &Keybinding) -> bool {
    canonical_key(&binding.key)
        .split(' ')
        .any(|chord| chord.split('+').any(|part| part == "meta"))
}

/// The bindings that run `command`, most useful first: the user's own, then keys
/// without Cmd/Super (which most terminals cannot send), then catalog order. Cmd/Super
/// defaults are dropped when another key exists.
pub(crate) fn ranked<'a>(
    effective: &[&'a Keybinding],
    user: &[Keybinding],
    command: &str,
) -> Vec<&'a Keybinding> {
    let command = canonical(command);
    let mut found = effective
        .iter()
        .enumerate()
        .filter(|(_, binding)| canonical(&binding.command) == command)
        .map(|(index, binding)| (is_user(binding, user), has_meta(binding), index, *binding))
        .collect::<Vec<_>>();
    let plain = found.iter().any(|(_, meta, _, _)| !meta);
    found.retain(|(user, meta, _, _)| *user || !*meta || !plain);
    found.sort_by_key(|(user, meta, index, _)| (!*user, *meta, *index));
    found.into_iter().map(|(.., binding)| binding).collect()
}

/// A canonical key sequence as people write it, such as `Ctrl+Shift+P`.
pub(crate) fn display(key: &str, mac: bool) -> String {
    canonical_key(key)
        .split(' ')
        .map(|chord| {
            let parts = chord.split('+').collect::<Vec<_>>();
            let (name, modifiers) = parts.split_last().unwrap_or((&"", &[]));
            let mut shown = Vec::new();
            for (modifier, label) in [
                ("ctrl", "Ctrl"),
                ("alt", "Alt"),
                ("shift", "Shift"),
                ("meta", if mac { "Cmd" } else { "Super" }),
            ] {
                if modifiers.contains(&modifier) {
                    shown.push(label.to_owned());
                }
            }
            shown.push(match *name {
                "arrowup" => "Up".to_owned(),
                "arrowdown" => "Down".to_owned(),
                "arrowleft" => "Left".to_owned(),
                "arrowright" => "Right".to_owned(),
                "pageup" => "PageUp".to_owned(),
                "pagedown" => "PageDown".to_owned(),
                "escape" => "Esc".to_owned(),
                other if other.chars().count() == 1 && modifiers.is_empty() => other.to_owned(),
                other if other.chars().count() == 1 => other.to_uppercase(),
                other => {
                    let mut chars = other.chars();
                    chars.next().map_or_else(String::new, |first| {
                        first.to_uppercase().chain(chars).collect()
                    })
                }
            });
            shown.join("+")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The most useful key for `command` in plain text, for compact hints.
pub(crate) fn primary_key(
    bindings: &[Keybinding],
    user: &[Keybinding],
    command: &str,
) -> Option<String> {
    let effective = effective(bindings);
    ranked(&effective, user, command)
        .first()
        .map(|binding| display(&binding.key, cfg!(target_os = "macos")))
}

/// Ctrl+key presses a focused terminal keeps for its shell: `crate::app` routes these
/// to the child before workbench bindings.
pub(crate) fn terminal_owned(key: &str) -> bool {
    canonical_key(key)
        .strip_prefix("ctrl+")
        .is_some_and(|rest| crate::app::TERMINAL_CONTROL_KEYS.contains(&rest))
}

/// Ctrl+Shift+letter, which a terminal without the kitty keyboard protocol sends as plain
/// Ctrl+letter: in a focused terminal that goes to the shell instead.
pub(crate) fn needs_keyboard_protocol(key: &str) -> bool {
    canonical_key(key)
        .strip_prefix("ctrl+shift+")
        .is_some_and(|rest| rest.len() == 1 && rest.as_bytes()[0].is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(key: &str, command: &str, when: Option<&str>) -> Keybinding {
        Keybinding {
            key: key.into(),
            command: command.into(),
            when: when.map(Into::into),
            args: None,
        }
    }

    #[test]
    fn keys_display_with_conventional_modifier_order() {
        assert_eq!(display("shift+ctrl+p", false), "Ctrl+Shift+P");
        assert_eq!(display("meta+shift+p", true), "Shift+Cmd+P");
        assert_eq!(display("meta+s", false), "Super+S");
        assert_eq!(display("ctrl+`", false), "Ctrl+`");
        assert_eq!(display("ArrowUp", false), "Up");
        assert_eq!(display(" ", false), "Space");
        assert_eq!(display("ctrl+k ctrl+s", false), "Ctrl+K Ctrl+S");
        assert_eq!(display("F6", false), "F6");
        assert_eq!(display("escape", false), "Esc");
        assert_eq!(display("s", false), "s");
        assert_eq!(display("shift+s", false), "Shift+S");
    }

    #[test]
    fn later_entries_and_removals_win_their_key_and_context() {
        let bindings = [
            binding("ctrl+k", "workbench.commandPalette", None),
            binding("ctrl+j", "workbench.quickOpen", None),
            binding("ctrl+j", "-workbench.quickOpen", None),
            binding("ctrl+k", "search.find", Some("editorFocus")),
        ];
        let found = effective(&bindings)
            .iter()
            .map(|binding| binding.command.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            found,
            ["workbench.commandPalette", "search.find"],
            "a removal hides the key; a context binding does not replace the global one"
        );
    }

    #[test]
    fn user_keys_rank_first_and_cmd_keys_only_fill_in() {
        let user = [binding("f2", "workbench.commandPalette", None)];
        let mut bindings = vec![
            binding("meta+p", "workbench.commandPalette", None),
            binding("ctrl+k", "workbench.commandPalette", None),
            binding("meta+o", "file.open", None),
        ];
        bindings.extend(user.iter().cloned());
        let effective = effective(&bindings);
        let keys = |command| {
            ranked(&effective, &user, command)
                .iter()
                .map(|binding| binding.key.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(keys("workbench.commandPalette"), ["f2", "ctrl+k"]);
        assert_eq!(keys("file.open"), ["meta+o"]);
        assert_eq!(keys("aiCli.new"), Vec::<&str>::new());
    }

    #[test]
    fn shell_control_keys_belong_to_the_terminal() {
        assert!(terminal_owned("ctrl+k"));
        assert!(terminal_owned("ctrl+space"));
        assert!(terminal_owned("ctrl+t"));
        assert!(terminal_owned("ctrl+q"));
        assert!(needs_keyboard_protocol("ctrl+shift+p"));
        assert!(!needs_keyboard_protocol("ctrl+shift+f12"));
        assert!(!needs_keyboard_protocol("f1"));
        assert!(!terminal_owned("ctrl+shift+p"));
        assert!(!terminal_owned("f1"));
    }
}
