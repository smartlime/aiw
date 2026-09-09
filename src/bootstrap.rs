use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::acquisition::TokenAcquirer;
use crate::cli::update_definitions;
use crate::config_sync::{
    ConfigSource, ConfigSyncError, ConfigSyncManager, SyncOutcome, SyncRequest,
};
use crate::environment::ShellKind;
use crate::registry::{SecretDefinition, SecretRegistry};
use crate::secret_store::SecretStore;
use crate::shell_integration::{Confirmer, IntegrationStatus, ShellIntegrationManager};

pub trait BootstrapPrompt {
    fn configuration_source(&self) -> Result<String, io::Error>;
}

#[derive(Default)]
pub struct StdinBootstrapPrompt;

impl BootstrapPrompt for StdinBootstrapPrompt {
    fn configuration_source(&self) -> Result<String, io::Error> {
        if io::stdout().is_terminal() {
            print!("\u{1b}[36m◆\u{1b}[0m Configuration URL, local path, or '-' to paste TOML: ");
        } else {
            print!("Configuration URL, local path, or '-' to paste TOML: ");
        }
        io::stdout().flush()?;
        let mut source = String::new();
        io::stdin().read_line(&mut source)?;
        Ok(source.trim().to_owned())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BootstrapRequest {
    pub source: Option<String>,
    pub profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapShellOutcome {
    pub shell: ShellKind,
    pub status: IntegrationStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapOutcome {
    pub configurations: Vec<SyncOutcome>,
    pub secret_accounts: usize,
    pub acquired_accounts: usize,
    pub missing_accounts: usize,
    pub shells: Vec<BootstrapShellOutcome>,
}

pub struct BootstrapManager<'a> {
    home: PathBuf,
    source: &'a dyn ConfigSource,
    store: &'a dyn SecretStore,
    acquirer: &'a dyn TokenAcquirer,
    confirmer: &'a dyn Confirmer,
    prompt: &'a dyn BootstrapPrompt,
}

impl<'a> BootstrapManager<'a> {
    pub fn new(
        home: impl Into<PathBuf>,
        source: &'a dyn ConfigSource,
        store: &'a dyn SecretStore,
        acquirer: &'a dyn TokenAcquirer,
        confirmer: &'a dyn Confirmer,
        prompt: &'a dyn BootstrapPrompt,
    ) -> Self {
        Self {
            home: home.into(),
            source,
            store,
            acquirer,
            confirmer,
            prompt,
        }
    }

    pub fn run(
        &self,
        request: BootstrapRequest,
        shells: &[ShellKind],
    ) -> Result<BootstrapOutcome, BootstrapError> {
        let sync_manager = ConfigSyncManager::new(&self.home, self.source, self.confirmer);
        let configurations = match request.source {
            Some(source) => vec![sync_manager.sync(sync_request(source, request.profile))?],
            None => {
                let configured = sync_manager.sync_configured()?;
                if configured.is_empty() && !has_registry_files(&self.home)? {
                    let source = self.prompt.configuration_source().map_err(|error| {
                        BootstrapError(format!("cannot read configuration source: {error}"))
                    })?;
                    if source.is_empty() {
                        return Err(BootstrapError(
                            "configuration source must not be empty".into(),
                        ));
                    }
                    vec![sync_manager.sync(SyncRequest::new(source))?]
                } else {
                    configured
                }
            }
        };

        let registry = SecretRegistry::from_directory(&self.registry_directory())
            .map_err(|error| BootstrapError(error.to_string()))?;
        if registry.secrets().is_empty() {
            return Err(BootstrapError(
                "secret configuration contains no accounts".into(),
            ));
        }
        let missing = missing_secrets(&registry, self.store)?;
        let mut acquired_accounts = 0;
        if !missing.is_empty()
            && self
                .confirmer
                .confirm(&format!(
                    "Acquire {} missing Keychain account(s)?",
                    missing.len()
                ))
                .map_err(|error| BootstrapError(format!("cannot read confirmation: {error}")))?
        {
            acquired_accounts = missing.len();
            update_definitions(missing, self.store, self.acquirer)
                .map_err(|error| BootstrapError(error.to_string()))?;
        }

        let shell_manager = ShellIntegrationManager::new(&self.home, self.confirmer, self.store);
        let mut shell_outcomes = Vec::new();
        for shell in shells {
            let status = shell_manager
                .status(*shell)
                .map_err(|error| BootstrapError(error.to_string()))?;
            if status != IntegrationStatus::Current {
                shell_manager
                    .install(*shell)
                    .map_err(|error| BootstrapError(error.to_string()))?;
            }
            shell_outcomes.push(BootstrapShellOutcome {
                shell: *shell,
                status: shell_manager
                    .status(*shell)
                    .map_err(|error| BootstrapError(error.to_string()))?,
            });
        }
        let missing_accounts = missing_secrets(&registry, self.store)?.len();
        Ok(BootstrapOutcome {
            configurations,
            secret_accounts: registry.secrets().len(),
            acquired_accounts,
            missing_accounts,
            shells: shell_outcomes,
        })
    }

    fn registry_directory(&self) -> PathBuf {
        self.home.join(".config/ai-world/secrets.d")
    }
}

fn sync_request(source: String, profile: Option<String>) -> SyncRequest {
    match profile {
        Some(profile) => SyncRequest::new(source).with_profile(profile),
        None => SyncRequest::new(source),
    }
}

fn has_registry_files(home: &Path) -> Result<bool, BootstrapError> {
    let directory = home.join(".config/ai-world/secrets.d");
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(BootstrapError(format!(
                "cannot inspect configuration directory '{}': {error}",
                directory.display()
            )));
        }
    };
    for entry in entries {
        let path = entry
            .map_err(|error| BootstrapError(format!("cannot inspect configuration: {error}")))?
            .path();
        if path
            .extension()
            .is_some_and(|extension| extension == "toml")
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn missing_secrets<'a>(
    registry: &'a SecretRegistry,
    store: &dyn SecretStore,
) -> Result<Vec<&'a SecretDefinition>, BootstrapError> {
    let mut missing = Vec::new();
    for secret in registry.secrets() {
        let value = store.get(&secret.keychain_coordinates()).map_err(|error| {
            BootstrapError(format!(
                "cannot read Keychain account '{}': {error}",
                secret.account
            ))
        })?;
        if value.is_none() {
            missing.push(secret);
        }
    }
    Ok(missing)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapError(String);

impl From<ConfigSyncError> for BootstrapError {
    fn from(error: ConfigSyncError) -> Self {
        Self(error.to_string())
    }
}

impl fmt::Display for BootstrapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for BootstrapError {}

impl BootstrapOutcome {
    pub fn render(&self, color: bool) -> String {
        let mut output = String::from("╭─ aiw bootstrap\n│\n├─ Configuration\n");
        if self.configurations.is_empty() {
            output.push_str("│  ✓ Existing local configuration\n");
        }
        for configuration in &self.configurations {
            output.push_str(&format!(
                "│  {} {}: {} (+{}, ~{}, -{})\n",
                mark(
                    configuration.status != crate::config_sync::SyncStatus::Declined,
                    color
                ),
                configuration.profile,
                configuration.status,
                configuration.added_accounts,
                configuration.changed_accounts,
                configuration.removed_accounts
            ));
        }
        output.push_str("│\n├─ Secrets\n");
        output.push_str(&format!(
            "│  {} {} account(s) available\n",
            mark(self.missing_accounts == 0, color),
            self.secret_accounts - self.missing_accounts
        ));
        if self.acquired_accounts > 0 {
            output.push_str(&format!(
                "│  {} {} missing account(s) acquired\n",
                mark(true, color),
                self.acquired_accounts
            ));
        }
        if self.missing_accounts > 0 {
            output.push_str(&format!(
                "│  {} {} account(s) remain missing\n",
                mark(false, color),
                self.missing_accounts
            ));
        }
        output.push_str("│\n├─ Shell integration\n");
        if self.shells.is_empty() {
            output.push_str("│  ○ No supported shell detected\n");
        }
        for shell in &self.shells {
            output.push_str(&format!(
                "│  {} {}: {}\n",
                mark(shell.status == IntegrationStatus::Current, color),
                shell.shell,
                shell.status
            ));
        }
        let ready = self.missing_accounts == 0
            && self
                .shells
                .iter()
                .all(|shell| shell.status == IntegrationStatus::Current);
        output.push_str(&format!(
            "│\n╰─ {}\n",
            if ready { "Ready" } else { "Incomplete" }
        ));
        output
    }
}

pub fn detect_shells(home: &Path) -> Vec<ShellKind> {
    let shell = std::env::var_os("SHELL")
        .and_then(|value| PathBuf::from(value).file_name().map(|name| name.to_owned()));
    let path = std::env::var_os("PATH").unwrap_or_default();
    let has_zsh = shell.as_deref().is_some_and(|name| name == "zsh")
        || home.join(".zshrc").exists()
        || executable_on_path("zsh", &path);
    let has_fish = shell.as_deref().is_some_and(|name| name == "fish")
        || home.join(".config/fish").exists()
        || executable_on_path("fish", &path);
    let mut shells = Vec::new();
    if has_zsh {
        shells.push(ShellKind::Zsh);
    }
    if has_fish {
        shells.push(ShellKind::Fish);
    }
    shells
}

fn executable_on_path(name: &str, path: &std::ffi::OsStr) -> bool {
    std::env::split_paths(path).any(|directory| directory.join(name).is_file())
}

fn mark(success: bool, color: bool) -> String {
    let (marker, code) = if success {
        ("✓", "32")
    } else {
        ("○", "33")
    };
    if color {
        format!("\u{1b}[{code}m{marker}\u{1b}[0m")
    } else {
        marker.to_owned()
    }
}
