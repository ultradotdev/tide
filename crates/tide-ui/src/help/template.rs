//! The help template language: `{{name}}` or `{{name:argument}}` placeholders in Markdown.
//!
//! Inline values: `key`, `terminalkey`, `label` (command IDs), `setting`, `default`
//! (setting keys), `path`, `url`, `help` (page IDs), `doc` (`path|text` under `docs/`),
//! `version`, `license`, `platform`, `terminalkeys`. Blocks: `keybindings`, `commands` (optionally one
//! category), `aliases`, `settings`, `agents`, `languageservers`, `themes`,
//! `install`.
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use serde_json::Value;
use tide_ai::cli::CliStatus;
use tide_config::{CommandInfo, Keybinding, Paths, Settings, ThemeCatalog};

use super::keys::{display, effective, ranked, terminal_owned};
use super::{Install, Page, product, tilde};
use crate::command::Command;

pub struct Context<'a> {
    /// Every binding in resolution order, user entries merged in.
    pub keybindings: &'a [Keybinding],
    /// The entries from the user's keybindings file.
    pub user_keybindings: &'a [Keybinding],
    pub commands: &'a BTreeMap<String, CommandInfo>,
    pub settings: &'a Settings,
    pub themes: &'a ThemeCatalog,
    pub host: Host<'a>,
}

/// Where a page is rendered: the repository documentation (defaults only, nothing
/// specific to a machine) or the running workbench.
pub enum Host<'a> {
    Docs,
    Live(Live<'a>),
}

pub struct Live<'a> {
    pub paths: &'a Paths,
    pub home: Option<&'a Path>,
    pub install: &'a Install,
    pub agents: &'a [CliStatus],
}

pub struct Rendered {
    pub text: String,
    /// Placeholders that did not resolve; they are left in the text.
    pub errors: Vec<String>,
}

#[must_use]
pub fn render(page: &Page, context: &Context<'_>) -> Rendered {
    let renderer = Renderer {
        context,
        page,
        effective: effective(context.keybindings),
    };
    let mut text = String::new();
    let mut errors = Vec::new();
    let mut rest = page.template;
    while let Some(start) = rest.find("{{") {
        text.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            errors.push("unclosed {{".to_owned());
            text.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let inner = after[..end].trim();
        rest = &after[end + 2..];
        if let Some(section) = ["live", "docs"]
            .into_iter()
            .find(|section| *section == inner)
        {
            let close = format!("{{{{/{section}}}}}");
            let Some(length) = rest.find(&close) else {
                errors.push(format!("{{{{{section}}}}} is not closed"));
                continue;
            };
            let live = matches!(context.host, Host::Live(_));
            if live != (section == "live") {
                rest = &rest[length..];
            }
            continue;
        }
        if matches!(inner, "/live" | "/docs") {
            continue;
        }
        let (name, argument) = inner
            .split_once(':')
            .map_or((inner, None), |(name, argument)| (name, Some(argument)));
        match renderer.placeholder(name, argument) {
            Ok(value) => text.push_str(&value),
            Err(error) => {
                errors.push(format!("{{{{{inner}}}}}: {error}"));
                write!(text, "{{{{{inner}}}}}").ok();
            }
        }
    }
    text.push_str(rest);
    Rendered { text, errors }
}

/// A titled table of commands, each with its keys.
type Group = (String, Vec<(String, Vec<String>)>);

struct Renderer<'a> {
    context: &'a Context<'a>,
    page: &'a Page,
    effective: Vec<&'a Keybinding>,
}

/// A Markdown code span that survives a backtick inside the text.
fn code(text: &str) -> String {
    if text.contains('`') {
        format!("`` {text} ``")
    } else {
        format!("`{text}`")
    }
}

fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

fn plain(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// `fileTree.moveUp` → `Move up`, for actions without a palette label.
fn humanize(id: &str) -> String {
    let name = id.rsplit('.').next().unwrap_or(id);
    let mut words = String::new();
    for (index, ch) in name.chars().enumerate() {
        if ch.is_ascii_uppercase() && index > 0 {
            words.push(' ');
            words.push(ch.to_ascii_lowercase());
        } else if index == 0 {
            words.push(ch.to_ascii_uppercase());
        } else {
            words.push(ch);
        }
    }
    words
}

fn context_name(when: &str) -> String {
    match when {
        "editorFocus" => "While editing".into(),
        "!editorFocus" => "Outside the editor".into(),
        "editorHasMultipleCursors" => "Editor with multiple cursors".into(),
        "fileTreeFocus" => "Explorer".into(),
        "gitPanelFocus" => "Source Control".into(),
        "outlinePanelFocus" => "Outline".into(),
        "timelinePanelFocus" => "Timeline".into(),
        "sqlEditorFocus" => "SQL editor".into(),
        "queryResultsFocus" => "Query results".into(),
        "agentsFocus" => "Agents sidebar".into(),
        "terminalFocus" => "Terminal".into(),
        other => format!("When {}", code(other)),
    }
}

fn section_name(key: &str) -> &'static str {
    match key.split('.').next().unwrap_or_default() {
        "editor" => "Editor",
        "files" => "Files",
        "workbench" => "Workbench",
        "tui" => "Layout and panels",
        "terminal" => "Terminal",
        "git" => "Git",
        "ai" => "AI CLI agents",
        "session" => "Sessions",
        "lsp" => "Language servers",
        _ => "Other",
    }
}

const SECTIONS: &[&str] = &[
    "Editor",
    "Files",
    "Workbench",
    "Layout and panels",
    "Terminal",
    "Git",
    "AI CLI agents",
    "Sessions",
    "Language servers",
    "Other",
];

fn accepted(key: &str) -> String {
    let Some(rule) = crate::preferences::rule(key) else {
        return String::new();
    };
    if let Some(choices) = rule.get("enum").and_then(Value::as_array) {
        return choices
            .iter()
            .map(|choice| code(&plain(choice)))
            .collect::<Vec<_>>()
            .join(", ");
    }
    if let (Some(minimum), Some(maximum)) = (rule.get("minimum"), rule.get("maximum")) {
        return format!("{minimum}–{maximum}");
    }
    match rule.get("type").and_then(Value::as_str) {
        Some("boolean") => format!("{} or {}", code("true"), code("false")),
        Some("array") => "List of absolute paths".into(),
        Some("object") => format!("Glob pattern → {} or {}", code("true"), code("false")),
        _ => "Text".into(),
    }
}

fn path_of<'p>(paths: &'p Paths, name: &str) -> Option<&'p Path> {
    Some(match name {
        "config" => &paths.config_dir,
        "settings" => &paths.settings_file,
        "keybindings" => &paths.keybindings_file,
        "themes" => &paths.themes_dir,
        "connections" => &paths.connections_file,
        "data" => &paths.data_dir,
        "secrets" => &paths.secrets_file,
        "history" => &paths.query_history_dir,
        "state" => &paths.state_dir,
        "sessions" => &paths.sessions_dir,
        "log" => &paths.debug_log_file,
        "cache" => &paths.cache_dir,
        _ => return None,
    })
}

fn release_commands(set: &str) -> Vec<String> {
    tide_config::builtin::release()["commands"][set]["commands"]
        .as_array()
        .map(|commands| commands.iter().map(plain).collect())
        .unwrap_or_default()
}

/// How to update a copy installed with `install`, as plain text for a dialog.
#[must_use]
pub fn update_instruction(install: &Install) -> String {
    match install {
        Install::Homebrew { formula } => format!("Update with Homebrew:\n\n  brew update && brew upgrade {formula}"),
        Install::Managed { local: false } => format!(
            "Update by running the install command again:\n\n  {}",
            release_command("binaryInstall", 0)
        ),
        Install::Managed { local: true } => "Update by pulling the latest source and running ./install-from-source.sh again from the checkout.".into(),
        Install::Source { .. } => format!(
            "Update with git pull, then rebuild in the checkout:\n\n  {}",
            release_command("sourceBuild", 0)
        ),
        Install::Unknown { .. } => {
            "Update the way you installed Tide; Help: Getting Started lists the command for each method.".into()
        }
    }
}

fn release_command(set: &str, index: usize) -> String {
    release_commands(set)
        .get(index)
        .cloned()
        .unwrap_or_default()
}

impl Renderer<'_> {
    fn live(&self) -> Option<&Live<'_>> {
        match &self.context.host {
            Host::Live(live) => Some(live),
            Host::Docs => None,
        }
    }

    fn mac(&self) -> bool {
        self.live().is_none() || cfg!(target_os = "macos")
    }

    fn placeholder(&self, name: &str, argument: Option<&str>) -> Result<String, String> {
        let required = || argument.ok_or_else(|| format!("{name} needs an argument"));
        let none = |value: String| {
            if argument.is_some() {
                Err(format!("{name} takes no argument"))
            } else {
                Ok(value)
            }
        };
        match name {
            "key" => self.key(required()?, false),
            "terminalkey" => self.key(required()?, true),
            "label" => self.label(required()?).map(str::to_owned),
            "setting" | "default" => {
                let key = required()?;
                if !crate::preferences::supported(key) {
                    return Err(format!("unknown setting {key}"));
                }
                let settings = if name == "setting" {
                    self.context.settings.clone()
                } else {
                    crate::catalog::settings()
                };
                let value = settings.get(key).map(plain).unwrap_or_default();
                Ok(code(if value.is_empty() { "\"\"" } else { &value }))
            }
            "path" => self.path(required()?),
            "url" => self.url(required()?),
            "help" => self.help_link(required()?),
            "doc" => self.doc_link(required()?),
            "version" => none(env!("CARGO_PKG_VERSION").to_owned()),
            "license" => none(env!("CARGO_PKG_LICENSE").to_owned()),
            "platform" => none(if self.live().is_some() {
                super::platform()
            } else {
                "macOS or Linux".to_owned()
            }),
            "terminalkeys" => none(self.terminal_keys()),
            "keybindings" => none(self.keybindings()),
            "commands" => self.commands(argument),
            "aliases" => none(self.aliases()),
            "settings" => none(self.settings()),
            "agents" => none(self.agents()),
            "languageservers" => none(self.language_servers()),
            "themes" => none(self.themes()),
            "install" => none(self.install()),
            _ => Err(format!("unknown placeholder {name}")),
        }
    }

    fn label(&self, id: &str) -> Result<&str, String> {
        self.context
            .commands
            .get(id)
            .map(|info| info.label.as_str())
            .ok_or_else(|| format!("{id} is not a command palette entry"))
    }

    fn keys(&self, id: &str) -> Vec<String> {
        let mut shown = Vec::new();
        for binding in ranked(&self.effective, self.context.user_keybindings, id) {
            let key = display(&binding.key, self.mac());
            if !shown.contains(&key) {
                shown.push(key);
            }
        }
        shown
    }

    fn palette_key(&self) -> Option<String> {
        self.keys(Command::WorkbenchCommandPalette.as_str())
            .into_iter()
            .next()
    }

    fn key(&self, id: &str, terminal: bool) -> Result<String, String> {
        if !crate::app::command_supported(id) {
            return Err(format!("unknown command {id}"));
        }
        let keys = ranked(&self.effective, self.context.user_keybindings, id)
            .into_iter()
            .filter(|binding| !terminal || !terminal_owned(&binding.key))
            .map(|binding| display(&binding.key, self.mac()))
            .fold(Vec::new(), |mut keys, key| {
                if !keys.contains(&key) {
                    keys.push(key);
                }
                keys
            });
        if !keys.is_empty() {
            return Ok(keys
                .iter()
                .take(2)
                .map(|key| code(key))
                .collect::<Vec<_>>()
                .join(" or "));
        }
        let palette = (id != Command::WorkbenchCommandPalette.as_str())
            .then(|| self.palette_key())
            .flatten();
        Ok(match (self.info(id), palette) {
            (Some(info), Some(palette)) => format!(
                "“{}” from the command palette ({})",
                info.label,
                code(&palette)
            ),
            (Some(info), None) => format!("“{}” from the command palette", info.label),
            (None, _) => format!(
                "unbound (bind {} in {})",
                code(id),
                code("keybindings.jsonc")
            ),
        })
    }

    fn terminal_keys(&self) -> String {
        let shown = |key: &str| code(&display(&format!("ctrl+{key}"), self.mac()));
        let symbols = crate::app::TERMINAL_CONTROL_KEYS
            .iter()
            .filter(|key| !(key.len() == 1 && key.as_bytes()[0].is_ascii_lowercase()))
            .map(|key| shown(key))
            .collect::<Vec<_>>();
        let symbols = match symbols.split_last() {
            Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
            None => String::new(),
        };
        format!(
            "every Ctrl+letter key, such as {}, {}, {} and {}, plus {symbols}",
            shown("c"),
            shown("r"),
            shown("x"),
            shown("q"),
        )
    }

    fn path(&self, name: &str) -> Result<String, String> {
        let defaults;
        let (paths, home) = match self.live() {
            Some(live) => (live.paths, live.home),
            None => {
                defaults = Paths::from_env(Path::new("~"), |_| None);
                (&defaults, None)
            }
        };
        let path = path_of(paths, name).ok_or_else(|| format!("unknown path {name}"))?;
        Ok(code(&tilde(path, home)))
    }

    fn url(&self, name: &str) -> Result<String, String> {
        let release = tide_config::builtin::release();
        let url = match name {
            "repository" | "homepage" | "issues" => product(name),
            "documentation" | "license" | "security" | "contributing" => {
                plain(&release["urls"][name])
            }
            "releases" => format!("{}/releases", product("repository")),
            "newIssue" => format!("{}/new", product("issues")),
            _ => return Err(format!("unknown url {name}")),
        };
        if url.is_empty() {
            return Err(format!("{name} has no URL"));
        }
        Ok(url)
    }

    fn help_link(&self, id: &str) -> Result<String, String> {
        let target = super::page(id).ok_or_else(|| format!("unknown help page {id}"))?;
        let label = self.label(target.command.as_str())?;
        match (&self.context.host, self.page.doc, target.doc) {
            (Host::Live(_), _, _) => Ok(format!("**{label}**")),
            (Host::Docs, Some(from), Some(to)) => Ok(format!(
                "[{}]({})",
                label.trim_start_matches("Help: "),
                relative(from, to)
            )),
            _ => Err(format!("{id} has no documentation file to link to")),
        }
    }

    fn doc_link(&self, argument: &str) -> Result<String, String> {
        let (path, text) = argument.split_once('|').ok_or("doc needs path|text")?;
        let target = format!("docs/{path}");
        match (&self.context.host, self.page.doc) {
            (Host::Live(_), _) => Ok(format!(
                "{text} ({}/blob/main/{target})",
                product("repository")
            )),
            (Host::Docs, Some(from)) => Ok(format!("[{text}]({})", relative(from, &target))),
            (Host::Docs, None) => Err("page has no documentation file".into()),
        }
    }

    /// The palette entry for `id`, or for an alternative ID that runs it.
    fn info(&self, id: &str) -> Option<&CommandInfo> {
        self.context.commands.get(id).or_else(|| {
            self.context
                .commands
                .iter()
                .find(|(alias, _)| crate::command_aliases::canonical(alias) == id)
                .map(|(_, info)| info)
        })
    }

    fn command_label(&self, id: &str) -> String {
        self.info(id)
            .map_or_else(|| humanize(id), |info| info.label.clone())
    }

    fn keybindings(&self) -> String {
        let mut groups: Vec<Group> = Vec::new();
        let mut contexts: Vec<Group> = Vec::new();
        for binding in &self.effective {
            let id = crate::command_aliases::canonical(&binding.command).to_owned();
            let (list, group) = match &binding.when {
                Some(when) => (&mut contexts, context_name(when)),
                None => (
                    &mut groups,
                    self.info(&id)
                        .map_or_else(|| "Other actions".to_owned(), |info| info.category.clone()),
                ),
            };
            let rows = match list.iter().position(|(name, _)| *name == group) {
                Some(index) => &mut list[index].1,
                None => {
                    list.push((group, Vec::new()));
                    &mut list.last_mut().expect("just pushed").1
                }
            };
            let key = display(&binding.key, self.mac());
            match rows.iter_mut().find(|(command, _)| *command == id) {
                Some((_, keys)) if !keys.contains(&key) => keys.push(key),
                Some(_) => {}
                None => rows.push((id, vec![key])),
            }
        }
        let mut text = String::new();
        for (name, rows) in groups.iter().chain(&contexts) {
            write!(
                text,
                "### {name}\n\n| Keys | Action | Command |\n| --- | --- | --- |\n"
            )
            .ok();
            for (id, keys) in rows {
                writeln!(
                    text,
                    "| {} | {} | {} |",
                    cell(
                        &keys
                            .iter()
                            .map(|key| code(key))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    cell(&self.command_label(id)),
                    code(id)
                )
                .ok();
            }
            text.push('\n');
        }
        text.trim_end().to_owned()
    }

    fn keys_cell(&self, id: &str) -> String {
        let keys = self.keys(id);
        if keys.is_empty() {
            "—".into()
        } else {
            cell(
                &keys
                    .iter()
                    .take(2)
                    .map(|key| code(key))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        }
    }

    fn commands(&self, category: Option<&str>) -> Result<String, String> {
        let mut categories: BTreeMap<&str, Vec<(&String, &CommandInfo)>> = BTreeMap::new();
        for (id, info) in crate::catalog::palette_commands(self.context.commands) {
            if category.is_none_or(|wanted| wanted == info.category) {
                categories
                    .entry(&info.category)
                    .or_default()
                    .push((id, info));
            }
        }
        if categories.is_empty() {
            return Err(format!(
                "no commands in {}",
                category.unwrap_or("the catalog")
            ));
        }
        let mut text = String::new();
        for (name, mut commands) in categories {
            commands.sort_by(|a, b| a.1.label.cmp(&b.1.label));
            if category.is_none() {
                write!(text, "### {name}\n\n").ok();
            }
            text.push_str("| Command | Keys | ID |\n| --- | --- | --- |\n");
            for (id, info) in commands {
                writeln!(
                    text,
                    "| {} | {} | {} |",
                    cell(&info.label),
                    self.keys_cell(id),
                    code(id)
                )
                .ok();
            }
            text.push('\n');
        }
        Ok(text.trim_end().to_owned())
    }

    fn aliases(&self) -> String {
        let mut text = String::from("| Alias | Runs |\n| --- | --- |\n");
        for (alias, canonical) in crate::command_aliases::ALIASES {
            writeln!(text, "| {} | {} |", code(alias), code(canonical)).ok();
        }
        text.trim_end().to_owned()
    }

    fn settings(&self) -> String {
        let defaults = crate::catalog::settings();
        let descriptions = tide_config::builtin::setting_descriptions();
        let live = self.live().is_some();
        let mut text = String::new();
        for section in SECTIONS {
            let keys = crate::preferences::SUPPORTED
                .iter()
                .filter(|key| section_name(key) == *section)
                .collect::<std::collections::BTreeSet<_>>();
            if keys.is_empty() {
                continue;
            }
            write!(text, "### {section}\n\n").ok();
            if live {
                text.push_str("| Setting | Description | Current | Default | Accepted values |\n| --- | --- | --- | --- | --- |\n");
            } else {
                text.push_str("| Setting | Description | Default | Accepted values |\n| --- | --- | --- | --- |\n");
            }
            for key in keys {
                let default = defaults.get(key).map(Value::to_string).unwrap_or_default();
                let description = descriptions.get(*key).map_or("", String::as_str);
                let current = if live {
                    let value = self
                        .context
                        .settings
                        .get(key)
                        .map(Value::to_string)
                        .unwrap_or_default();
                    if value == default {
                        format!(" {} |", cell(&code(&value)))
                    } else {
                        format!(" **{}** |", cell(&code(&value)))
                    }
                } else {
                    String::new()
                };
                writeln!(
                    text,
                    "| {} | {} |{current} {} | {} |",
                    code(key),
                    cell(description),
                    cell(&code(&default)),
                    cell(&accepted(key))
                )
                .ok();
            }
            text.push('\n');
        }
        text.trim_end().to_owned()
    }

    fn agents(&self) -> String {
        let mut text = String::new();
        match self.live() {
            Some(live) => {
                text.push_str("| Agent | Executable | On this machine | Path override |\n| --- | --- | --- | --- |\n");
                for provider in tide_ai::cli::CLI_PROVIDERS {
                    let found = live
                        .agents
                        .iter()
                        .find(|status| status.provider == provider)
                        .and_then(|status| status.executable.as_deref());
                    writeln!(
                        text,
                        "| {} | {} | {} | {} |",
                        provider.display_name(),
                        code(provider.executable()),
                        found.map_or_else(
                            || "Not found".to_owned(),
                            |path| format!("Installed: {}", code(&tilde(path, live.home)))
                        ),
                        code(provider.env_override())
                    )
                    .ok();
                }
            }
            None => {
                text.push_str("| Agent | Executable | Path override |\n| --- | --- | --- |\n");
                for provider in tide_ai::cli::CLI_PROVIDERS {
                    writeln!(
                        text,
                        "| {} | {} | {} |",
                        provider.display_name(),
                        code(provider.executable()),
                        code(provider.env_override())
                    )
                    .ok();
                }
            }
        }
        text.trim_end().to_owned()
    }

    fn language_servers(&self) -> String {
        let live = self.live().is_some();
        let mut servers: Vec<(String, Vec<String>, Vec<&str>)> = Vec::new();
        let mut disabled = Vec::new();
        for default in tide_lsp::default_servers() {
            let language = default.language_id.clone();
            let server = if live {
                crate::controllers::lsp::server_for(self.context.settings, &language)
            } else {
                Some(default)
            };
            let Some(server) = server else {
                disabled.push(language);
                continue;
            };
            match servers
                .iter_mut()
                .find(|(command, args, _)| *command == server.command && *args == server.args)
            {
                Some((_, _, languages)) => languages.push(language_name(&language)),
                None => servers.push((
                    server.command.clone(),
                    server.args.clone(),
                    vec![language_name(&language)],
                )),
            }
        }
        let mut text = if live {
            String::from("| Languages | Server | On this machine |\n| --- | --- | --- |\n")
        } else {
            String::from("| Languages | Server |\n| --- | --- |\n")
        };
        for (command, args, languages) in servers {
            let mut languages = languages;
            languages.dedup();
            let invocation = std::iter::once(command.as_str())
                .chain(args.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            write!(text, "| {} | {} |", languages.join(", "), code(&invocation)).ok();
            if live {
                text.push_str(if tide_lsp::command_available(&command) {
                    " Installed |"
                } else {
                    " Not found |"
                });
            }
            text.push('\n');
        }
        if !disabled.is_empty() {
            write!(
                text,
                "\nDisabled by {}: {}.\n",
                code("lsp.servers"),
                disabled
                    .iter()
                    .map(|language| code(language))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .ok();
        }
        text.trim_end().to_owned()
    }

    fn themes(&self) -> String {
        let live = self.live().is_some();
        let selected = self.context.themes.selected_id();
        let mut text = String::from("| Theme | ID | Type |\n| --- | --- | --- |\n");
        for theme in self.context.themes.iter() {
            let current = if live && theme.id == selected {
                " (current)"
            } else {
                ""
            };
            writeln!(
                text,
                "| {}{current} | {} | {} |",
                cell(&theme.name),
                code(&theme.id),
                theme.theme_type
            )
            .ok();
        }
        text.trim_end().to_owned()
    }

    fn install(&self) -> String {
        let homebrew = |formula: &str| {
            format!(
                "Update with {} and uninstall with {}. Homebrew owns this copy, so {} refuses and names the {} command to use instead.",
                code(&format!("brew update && brew upgrade {formula}")),
                code(&format!("brew uninstall {formula}")),
                code("tide --uninstall"),
                code("brew")
            )
        };
        let script = format!(
            "Update by running the install command again: {}. Uninstall with {}; add {} to remove your settings and data as well.",
            code(&release_command("binaryInstall", 0)),
            code(&release_command("binaryUninstall", 0)),
            code("--purge")
        );
        let local = format!(
            "Update by pulling the latest source and running {} again from the checkout; {} returns to the previous build. Uninstall with {}.",
            code("./install-from-source.sh"),
            code("./install.sh --rollback"),
            code(&release_command("binaryUninstall", 0))
        );
        let source = format!(
            "Update with {} and then {} in the checkout. To remove it, delete the build; nothing else was installed.",
            code("git pull"),
            code(&release_command("sourceBuild", 0))
        );
        let all = format!(
            "- **Homebrew:** {}\n- **Install script:** {script}\n- **Managed install from source** ({}): {local}\n- **Source build** ({}): {source}",
            homebrew("tide"),
            code("./install-from-source.sh"),
            code("./target/release/tide")
        );
        let data = format!(
            "Updates and uninstalls keep your settings, keybindings, sessions and connections; only {} removes them.",
            code("tide --uninstall --purge")
        );
        let Some(live) = self.live() else {
            return format!("{all}\n\n{data}");
        };
        let body = match live.install {
            Install::Homebrew { formula } => {
                format!(
                    "This copy of Tide was installed with **Homebrew**. {}",
                    homebrew(formula)
                )
            }
            Install::Managed { local: false } => {
                format!("This copy of Tide was installed with the **install script**. {script}")
            }
            Install::Managed { local: true } => {
                format!("This copy of Tide is a **managed install from source**. {local}")
            }
            Install::Source { executable } => format!(
                "This copy of Tide is a **source build** at {}. {source}",
                code(&tilde(executable, live.home))
            ),
            Install::Unknown { executable } => format!(
                "Tide could not tell how this copy{} was installed. Use the commands for the method you used:\n\n{all}",
                executable
                    .as_deref()
                    .map(|path| format!(" ({})", code(&tilde(path, live.home))))
                    .unwrap_or_default()
            ),
        };
        format!("{body}\n\n{data}")
    }
}

fn language_name(id: &str) -> &'static str {
    match id {
        "typescript" => "TypeScript",
        "javascript" => "JavaScript",
        "typescriptreact" => "TSX",
        "javascriptreact" => "JSX",
        "rust" => "Rust",
        "python" => "Python",
        "go" => "Go",
        "ruby" => "Ruby",
        "c" => "C",
        "cpp" => "C++",
        "json" => "JSON",
        "html" => "HTML",
        "css" => "CSS",
        "sql" => "SQL",
        _ => "Other",
    }
}

/// The relative link from one repository Markdown file to another.
fn relative(from: &str, to: &str) -> String {
    let from = from.split('/').collect::<Vec<_>>();
    let to = to.split('/').collect::<Vec<_>>();
    let directory = &from[..from.len().saturating_sub(1)];
    let common = directory
        .iter()
        .zip(&to)
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts = vec![".."; directory.len() - common];
    parts.extend(&to[common..]);
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_relative_between_documentation_files() {
        assert_eq!(
            relative("docs/features/git.md", "docs/features/editor.md"),
            "editor.md"
        );
        assert_eq!(
            relative("docs/features/git.md", "docs/reference/commands.md"),
            "../reference/commands.md"
        );
        assert_eq!(
            relative("docs/welcome.md", "docs/features/ai.md"),
            "features/ai.md"
        );
    }

    #[test]
    fn humanized_action_names_read_as_sentences() {
        assert_eq!(humanize("fileTree.moveUp"), "Move up");
        assert_eq!(humanize("queryResults.showDetails"), "Show details");
    }

    #[test]
    fn code_spans_survive_backticks() {
        assert_eq!(code("Ctrl+K"), "`Ctrl+K`");
        assert_eq!(code("Ctrl+`"), "`` Ctrl+` ``");
    }
}
