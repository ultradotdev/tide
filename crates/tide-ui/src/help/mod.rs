//! Help pages: Markdown templates rendered from the live keymap, command catalog,
//! settings and environment, or from the defaults for the repository's documentation.
mod keys;
mod template;

use std::path::{Path, PathBuf};

use crate::command::Command;

pub(crate) use keys::primary_key;
pub use template::{Context, Host, Live, Rendered, render, update_instruction};

/// A help page and the documentation file its static rendering produces.
#[derive(Debug)]
pub struct Page {
    pub id: &'static str,
    pub command: Command,
    /// The tab title.
    pub title: &'static str,
    /// The repository documentation rendered from this page, if any.
    pub doc: Option<&'static str>,
    pub template: &'static str,
}

macro_rules! page {
    ($id:literal, $command:ident, $title:literal, $doc:expr) => {
        Page {
            id: $id,
            command: Command::$command,
            title: $title,
            doc: $doc,
            template: include_str!(concat!("pages/", $id, ".md")),
        }
    };
}

pub static PAGES: &[Page] = &[
    page!("welcome", HelpWelcome, "Welcome", Some("docs/welcome.md")),
    page!(
        "getting-started",
        HelpGettingStarted,
        "Help: Getting Started",
        Some("docs/getting-started.md")
    ),
    page!(
        "workbench",
        HelpWorkbench,
        "Help: Workbench",
        Some("docs/features/workbench.md")
    ),
    page!(
        "editor",
        HelpEditor,
        "Help: Editor",
        Some("docs/features/editor.md")
    ),
    page!("git", HelpGit, "Help: Git", Some("docs/features/git.md")),
    page!(
        "terminal",
        HelpTerminal,
        "Help: Terminals",
        Some("docs/features/terminal.md")
    ),
    page!(
        "postgresql",
        HelpDatabase,
        "Help: PostgreSQL",
        Some("docs/features/postgresql.md")
    ),
    page!(
        "agents",
        HelpAgents,
        "Help: AI CLI Agents",
        Some("docs/features/ai.md")
    ),
    page!(
        "themes",
        HelpThemes,
        "Help: Themes and Customization",
        Some("docs/features/themes.md")
    ),
    page!(
        "keyboard-shortcuts",
        HelpKeyboardShortcuts,
        "Help: Keyboard Shortcuts",
        Some("docs/reference/keyboard-shortcuts.md")
    ),
    page!(
        "commands",
        HelpCommands,
        "Help: Commands",
        Some("docs/reference/commands.md")
    ),
    page!(
        "settings",
        HelpSettings,
        "Help: Settings",
        Some("docs/reference/configuration.md")
    ),
    page!(
        "troubleshooting",
        HelpTroubleshooting,
        "Help: Troubleshooting",
        Some("docs/troubleshooting.md")
    ),
    page!("about", HelpAbout, "Help: About Tide", None),
];

#[must_use]
pub fn page(id: &str) -> Option<&'static Page> {
    PAGES.iter().find(|page| page.id == id)
}

#[must_use]
pub fn for_command(command: Command) -> Option<&'static Page> {
    PAGES.iter().find(|page| page.command == command)
}

/// How the running executable was installed, which decides its update and removal commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    /// A Homebrew keg owned by `formula`.
    Homebrew {
        formula: String,
    },
    /// The managed store written by `install.sh`; `local` when installed from a source build.
    Managed {
        local: bool,
    },
    /// A Cargo build inside a source checkout.
    Source {
        executable: PathBuf,
    },
    Unknown {
        executable: Option<PathBuf>,
    },
}

impl Install {
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Homebrew { .. } => "Homebrew",
            Self::Managed { local: false } => "the install script",
            Self::Managed { local: true } => "a managed install from source",
            Self::Source { .. } => "a source build",
            Self::Unknown { .. } => "an unrecognized method",
        }
    }
}

/// The file whose presence records that Welcome has been shown once.
#[must_use]
pub fn welcome_marker(paths: &tide_config::Paths) -> PathBuf {
    paths.state_dir.join("welcome-shown")
}

/// Records that Welcome was shown. An unwritable state directory only means it shows again.
pub(crate) fn record_welcome(paths: &tide_config::Paths) {
    let marker = welcome_marker(paths);
    let written = marker
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&marker, env!("CARGO_PKG_VERSION")));
    if let Err(error) = written {
        tracing::warn!(%error, "could not record that Welcome was shown");
    }
}

/// How often the automatic update check may ask GitHub.
pub const UPDATE_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// The file recording when the automatic update check last asked, in Unix seconds.
#[must_use]
pub fn update_check_marker(paths: &tide_config::Paths) -> PathBuf {
    paths.state_dir.join("update-check")
}

fn unix_seconds(time: std::time::SystemTime) -> u64 {
    time.duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Whether the automatic check is due at `now`: never checked, a day has passed, or the
/// recorded time is unreadable or in the future (a clock that moved back).
#[must_use]
pub fn update_check_due(paths: &tide_config::Paths, now: std::time::SystemTime) -> bool {
    let Some(last) = std::fs::read_to_string(update_check_marker(paths))
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
    else {
        return true;
    };
    let now = unix_seconds(now);
    last > now || now - last >= UPDATE_CHECK_INTERVAL.as_secs()
}

/// Records an automatic check at `now`. An unwritable state directory only means the next
/// start checks again.
pub(crate) fn record_update_check(paths: &tide_config::Paths, now: std::time::SystemTime) {
    let marker = update_check_marker(paths);
    let written = marker
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&marker, unix_seconds(now).to_string()));
    if let Err(error) = written {
        tracing::warn!(%error, "could not record the update check");
    }
}

pub(crate) fn product(key: &str) -> String {
    tide_config::builtin::product()[key]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[must_use]
pub fn documentation_url() -> String {
    tide_config::builtin::release()["urls"]["documentation"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    };
    format!("{os} ({})", std::env::consts::ARCH)
}

/// A new-issue URL prefilled with the version, platform and install method, never paths.
#[must_use]
pub fn new_issue_url(install: &Install) -> String {
    let body = format!(
        "**Tide version:** {}\n**Platform:** {}\n**Installed with:** {}\n\n**What happened?**\n\n\n**What did you expect?**\n\n\n**Steps to reproduce**\n\n1. \n",
        env!("CARGO_PKG_VERSION"),
        platform(),
        install.name()
    );
    url::Url::parse_with_params(&format!("{}/new", product("issues")), [("body", body)])
        .map_or_else(|_| product("issues"), String::from)
}

/// Reads the tag of the latest release from a `/releases/latest` URL. Tests substitute one
/// that never touches the network.
pub type LatestRelease =
    fn(String) -> futures_util::future::BoxFuture<'static, Result<String, String>>;

/// Where GitHub redirects to the latest release, which skips prereleases.
#[must_use]
pub fn latest_release_url() -> String {
    format!("{}/releases/latest", product("repository"))
}

/// One HTTPS request to `url`: GitHub answers with a redirect to `/releases/tag/v…`, and
/// the tag is read from its location without following it, so no API rate limit applies.
pub fn latest_release(
    url: String,
) -> futures_util::future::BoxFuture<'static, Result<String, String>> {
    Box::pin(async move {
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(15))
            .user_agent(concat!("tide/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| error.to_string())?;
        let response = client
            .head(&url)
            .send()
            .await
            .map_err(|error| format!("could not reach GitHub: {error}"))?;
        let location = if response.status().is_redirection() {
            response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        } else if response.status().is_success() {
            Some(response.url().to_string())
        } else {
            None
        };
        location
            .as_deref()
            .and_then(release_tag)
            .ok_or_else(|| format!("no published release was found ({})", response.status()))
    })
}

/// The tag in a release URL such as `…/releases/tag/v1.2.3`.
#[must_use]
pub fn release_tag(location: &str) -> Option<String> {
    let path = location.split(['?', '#']).next()?;
    let tag = path.rsplit_once("/releases/tag/")?.1.trim_end_matches('/');
    (!tag.is_empty() && !tag.contains('/')).then(|| tag.to_owned())
}

/// Whether release tag `latest` is a later version than `current`.
#[must_use]
pub fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |version: &str| -> Option<Vec<u64>> {
        version
            .trim_start_matches('v')
            .split(['-', '+'])
            .next()?
            .split('.')
            .map(|part| part.parse().ok())
            .collect()
    };
    match (parse(latest), parse(current)) {
        (Some(mut latest), Some(mut current)) => {
            let length = latest.len().max(current.len());
            latest.resize(length, 0);
            current.resize(length, 0);
            latest > current
        }
        _ => false,
    }
}

/// Opens `url` in the desktop browser without waiting for it.
pub fn open_url(url: &str) -> Result<(), String> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    spawn_desktop(program, &[std::ffi::OsStr::new(url)])
}

/// Shows `path` in the desktop file manager without waiting for it: selected in
/// Finder on macOS, or its folder opened elsewhere.
pub fn reveal_path(path: &std::path::Path) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        spawn_desktop("open", &[std::ffi::OsStr::new("-R"), path.as_os_str()])
    } else {
        let folder = path.parent().unwrap_or(path);
        spawn_desktop("xdg-open", &[folder.as_os_str()])
    }
}

/// Runs a desktop helper detached from Tide's terminal, without Tide's secrets.
fn spawn_desktop(program: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let mut command = std::process::Command::new(program);
    tide_core::process::scrub_tide_secrets(&mut command)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("{program}: {error}"))?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Every page that has a documentation file, rendered with the default keymap and
/// settings and without machine-specific values, as `(repository path, Markdown)`.
#[must_use]
pub fn static_docs() -> Vec<(&'static str, String)> {
    let keybindings = default_keybindings();
    let commands = crate::catalog::commands();
    let settings = crate::catalog::settings();
    let themes = tide_config::builtin::themes();
    let context = Context {
        keybindings: &keybindings,
        user_keybindings: &[],
        commands: &commands,
        settings: &settings,
        themes: &themes,
        host: Host::Docs,
    };
    PAGES
        .iter()
        .filter_map(|page| {
            let doc = page.doc?;
            let rendered = render(page, &context);
            Some((
                doc,
                format!(
                    "<!-- Generated from crates/tide-ui/src/help/pages/{}.md by `cargo xtask generate-rust-reference --write`. Edit the template, not this file. -->\n{}",
                    page.id, rendered.text
                ),
            ))
        })
        .collect()
}

/// The built-in keybindings with the terminal-safe alternates, as the workbench installs them.
pub(crate) fn default_keybindings() -> Vec<tide_config::Keybinding> {
    let mut keybindings = tide_config::builtin::keybindings();
    crate::catalog::install_bindings(&mut keybindings);
    keybindings
}

/// `path` with the home directory shortened to `~`.
pub(crate) fn tilde(path: &Path, home: Option<&Path>) -> String {
    home.and_then(|home| path.strip_prefix(home).ok())
        .map_or_else(
            || path.display().to_string(),
            |rest| Path::new("~").join(rest).display().to_string(),
        )
}

#[cfg(test)]
mod tests;
