use std::error::Error;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use ai_world::acquisition::BrowserTokenAcquirer;
use ai_world::bootstrap::{
    BootstrapManager, BootstrapRequest, StdinBootstrapPrompt, detect_shells,
};
use ai_world::cli::{Cli, Command, ConfigSourceArgs, execute};
use ai_world::config_sync::{ConfigSyncManager, SyncRequest, SystemConfigSource, render_sync};
use ai_world::registry::SecretRegistry;
use ai_world::secret_store::MacOSKeychainAdapter;
use ai_world::shell_integration::{ShellIntegrationManager, StdinConfirmer};
use clap::Parser;

fn main() -> ExitCode {
    match run() {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("aiw: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<String, Box<dyn Error>> {
    let cli = Cli::parse();
    let home = home_directory()?;
    let store = MacOSKeychainAdapter;
    let acquirer = BrowserTokenAcquirer;
    let confirmer = StdinConfirmer;
    let source = SystemConfigSource;
    let color = std::io::stdout().is_terminal();
    match cli.command {
        Command::Sync(arguments) => sync(&home, arguments, &source, &confirmer, color),
        Command::Bootstrap(arguments) => {
            let prompt = StdinBootstrapPrompt;
            let manager =
                BootstrapManager::new(&home, &source, &store, &acquirer, &confirmer, &prompt);
            let outcome = manager.run(
                BootstrapRequest {
                    source: arguments.source,
                    profile: arguments.profile,
                },
                &detect_shells(&home),
            )?;
            Ok(outcome.render(color))
        }
        command => {
            let registry =
                SecretRegistry::from_directory(&home.join(".config/ai-world/secrets.d"))?;
            let shell_manager = ShellIntegrationManager::new(&home, &confirmer, &store);
            Ok(execute(
                command,
                &registry,
                &store,
                &acquirer,
                &shell_manager,
            )?)
        }
    }
}

fn sync(
    home: &std::path::Path,
    arguments: ConfigSourceArgs,
    source: &SystemConfigSource,
    confirmer: &StdinConfirmer,
    color: bool,
) -> Result<String, Box<dyn Error>> {
    let manager = ConfigSyncManager::new(home, source, confirmer);
    let outcomes = match arguments.source {
        Some(source) => {
            let request = match arguments.profile {
                Some(profile) => SyncRequest::new(source).with_profile(profile),
                None => SyncRequest::new(source),
            };
            vec![manager.sync(request)?]
        }
        None => manager.sync_configured()?,
    };
    if outcomes.is_empty() {
        return Err("no configured sources; use --source URL_OR_PATH".into());
    }
    Ok(render_sync(&outcomes, color))
}

fn home_directory() -> Result<PathBuf, Box<dyn Error>> {
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    Ok(PathBuf::from(home))
}
