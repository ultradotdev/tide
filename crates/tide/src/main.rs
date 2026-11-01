mod install;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "tide",
    version = concat!("v", env!("CARGO_PKG_VERSION")),
    about = "Tide by Ultra.dev — a terminal-native code editor",
    disable_help_subcommand = true,
    group(clap::ArgGroup::new("installation_action").args(["uninstall", "install_local", "install_release", "verify_install_manifest", "activate_existing"]).multiple(false))
)]
struct Cli {
    /// File or folder to open
    #[arg(value_name = "file|folder")]
    path: Option<PathBuf>,

    /// Enable debug logging
    #[arg(long)]
    debug: bool,

    /// Open a named session
    #[arg(long, value_name = "name")]
    session: Option<String>,

    /// Start without restoring a session
    #[arg(long)]
    no_session: bool,

    /// Unsupported: shared TypeScript/Rust session writes are disabled until
    /// bidirectional compatibility and concurrent-writer tests pass
    #[arg(long)]
    shared_sessions: bool,

    /// Verify the packaged terminal runtime
    #[arg(long)]
    check_terminal: bool,

    /// Verify the packaged AI model catalog
    #[arg(long)]
    check_models: bool,

    /// Report available and missing language servers
    #[arg(long)]
    check_lsp: bool,

    /// Report detected AI CLI agents
    #[arg(long)]
    check_ai_cli: bool,

    /// Remove Tide binaries and installed releases
    #[arg(long)]
    uninstall: bool,

    /// Confirm removal of user data with --uninstall --purge
    #[arg(long, requires = "purge")]
    yes: bool,

    #[arg(long, hide = true)]
    install_local: bool,
    #[arg(long, hide = true, requires = "install_local")]
    migrate_legacy: bool,
    #[arg(long, hide = true)]
    install_release: Option<PathBuf>,
    #[arg(long, hide = true)]
    activate_existing: Option<String>,
    #[arg(long, hide = true)]
    verify_install_manifest: Option<PathBuf>,

    /// Validate the embedded settings/keybinding/command/theme catalogs
    #[arg(long)]
    check_catalogs: bool,

    /// With --uninstall, also remove Tide-owned user data
    #[arg(long, requires = "uninstall")]
    purge: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(cli.debug)?;

    if cli.uninstall {
        install::uninstall(cli.purge, cli.yes)?;
        return Ok(());
    }
    if let Some(path) = &cli.verify_install_manifest {
        install::verify_payload(path)?;
        return Ok(());
    }
    if cli.install_local || cli.install_release.is_some() || cli.activate_existing.is_some() {
        install::refuse_package_managed(install::ManagedCommand::Install)?;
    }
    if let Some(id) = &cli.activate_existing {
        install::Installation::discover()?.activate_existing(id)?;
        return Ok(());
    }
    if cli.install_local {
        install::Installation::discover()?
            .install_local(&std::env::current_exe()?, cli.migrate_legacy)?;
        return Ok(());
    }
    if let Some(path) = &cli.install_release {
        install::Installation::discover()?.install_release(path)?;
        return Ok(());
    }

    if cli.check_catalogs {
        let settings = tide_config::builtin::settings();
        let keybindings = tide_config::builtin::keybindings();
        let commands = tide_config::builtin::commands();
        let themes = tide_config::builtin::themes();
        let mut missing = std::collections::BTreeSet::new();
        let mut experimental = std::collections::BTreeSet::new();
        let mut excluded = std::collections::BTreeSet::new();
        let mut supported = std::collections::BTreeSet::new();
        for binding in &keybindings {
            if binding.command.starts_with("ensemble.") {
                excluded.insert(binding.command.clone());
            } else if binding.command.starts_with("aiChat.") && binding.command != "aiChat.openCli"
            {
                experimental.insert(binding.command.clone());
            } else if tide_ui::command_supported(&binding.command) {
                supported.insert(binding.command.clone());
            } else {
                missing.insert(binding.command.clone());
            }
        }
        println!(
            "catalogs: {} settings, {} keybindings, {} commands, {} themes",
            settings.len(),
            keybindings.len(),
            commands.len(),
            themes.len()
        );
        println!(
            "keybinding commands: {} supported, {} experimental/AI-Chat-deferred, {} excluded, {} unresolved",
            supported.len(),
            experimental.len(),
            excluded.len(),
            missing.len()
        );
        for command in &experimental {
            println!("  experimental (disabled by default): {command}");
        }
        for command in &excluded {
            println!("  excluded from scope: {command}");
        }
        if !missing.is_empty() {
            for command in missing.iter().take(20) {
                eprintln!("keybinding references an unsupported command: {command}");
            }
            anyhow::bail!(
                "{} keybinding command reference(s) are unresolved",
                missing.len()
            );
        }
        return Ok(());
    }

    if cli.check_terminal {
        let output = tide_terminal::check_terminal()?;
        println!("Terminal runtime OK: {}", output.trim());
        return Ok(());
    }

    if cli.check_models {
        let paths = tide_config::Paths::discover();
        let registry = tide_ai::models::ModelRegistry::load_or_bundled(&paths.models_file)?;
        let problems = registry.validate();
        if !problems.is_empty() {
            for problem in &problems {
                eprintln!("model catalog: {problem}");
            }
            anyhow::bail!("AI model catalog has {} problem(s)", problems.len());
        }
        println!(
            "AI model catalog OK: {} models, {} providers, defaults={}",
            registry.models().len(),
            registry
                .config()
                .provider_defaults
                .iter()
                .filter(|(_, model)| !model.is_empty())
                .count(),
            registry.config().provider_defaults.len()
        );
        return Ok(());
    }

    if cli.check_ai_cli {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        for status in tide_ai::cli::detect_all() {
            let sessions = status.installed.then(|| {
                tide_ai::cli::discover_sessions_with_cli(status.provider, &home, &workspace).len()
            });
            println!(
                "{:<32} {}{}",
                status.provider.display_name(),
                status
                    .executable
                    .as_ref()
                    .map_or_else(|| "missing".to_owned(), |path| path.display().to_string()),
                sessions.map_or_else(String::new, |count| format!(" ({count} sessions)"))
            );
        }
        return Ok(());
    }

    if cli.check_lsp {
        let mut seen = std::collections::BTreeSet::new();
        for server in tide_lsp::default_servers() {
            if !seen.insert(server.command.clone()) {
                continue;
            }
            let status = if tide_lsp::command_available(&server.command) {
                "available"
            } else {
                "missing"
            };
            println!("{:<32} {status}", server.command);
        }
        return Ok(());
    }

    if cli.shared_sessions {
        anyhow::bail!(
            "shared TypeScript session writes are disabled: compatibility and concurrent-writer tests have not passed; Rust-owned sessions remain the default"
        );
    }

    let (workspace, initial_file) = resolve_workspace(cli.path.as_deref());
    tracing::info!(?workspace, ?initial_file, "starting Tide");
    if !std::io::stdout().is_terminal() {
        anyhow::bail!(
            "Tide's TUI requires a terminal; try `tide --check-terminal` for the PTY smoke check"
        );
    }

    let app = tide_ui::App::new(tide_ui::AppConfig {
        workspace,
        initial_file,
        session_name: cli.session,
        restore_session: !cli.no_session,
        shared_sessions: cli.shared_sessions,
        paths: None,
        jobs: tide_ui::Jobs::runtime(tokio::runtime::Handle::current()),
        agent_discovery: tide_ui::discover_agent_sessions,
    })?;
    tide_ui::run(app).await
}

fn resolve_workspace(path: Option<&Path>) -> (PathBuf, Option<PathBuf>) {
    let Some(path) = path else {
        return (
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            None,
        );
    };
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if path.is_dir() {
        return (path, None);
    }
    if path.is_file() {
        return (
            path.parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
            Some(path.to_path_buf()),
        );
    }
    (
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        None,
    )
}

fn init_logging(debug: bool) -> Result<()> {
    if !debug {
        return Ok(());
    }
    let paths = tide_config::Paths::discover();
    std::fs::create_dir_all(&paths.logs_dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.debug_log_file)?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("tide=debug")),
        )
        .with_writer(file)
        .with_ansi(false)
        .init();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_relative_file_arguments_are_resolved_once() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let file = dir.path().join("src/main.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            "fn main() {}
",
        )
        .unwrap();
        let (workspace, initial) = resolve_workspace(Some(&file));
        assert!(workspace.is_absolute());
        assert!(initial.as_ref().unwrap().is_absolute());
        assert_eq!(initial.unwrap(), std::fs::canonicalize(file).unwrap());
    }
}
