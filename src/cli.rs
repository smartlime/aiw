use std::error::Error;
use std::fmt;

use clap::{ArgGroup, Args, Parser, Subcommand};

use crate::acquisition::TokenAcquirer;
use crate::environment::{self, ShellKind};
use crate::registry::{SecretDefinition, SecretRegistry};
use crate::secret_store::{KeychainCoordinates, SecretStore};
use crate::shell_integration::ShellIntegrationManager;

const BACKUP_SERVICE: &str = "ai-world.backup";

#[derive(Debug, Parser)]
#[command(name = "aiw", version, about = "Local control plane for AI tooling")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Set up configuration, missing tokens, and shell integration.
    #[command(alias = "bs")]
    Bootstrap(ConfigSourceArgs),
    /// Refresh configuration metadata without changing token values.
    #[command(alias = "sy")]
    Sync(ConfigSourceArgs),
    /// List registered environment names without values.
    #[command(alias = "ls")]
    List,
    /// Show one registered value or all registered values.
    #[command(alias = "sh")]
    Show(SelectionArgs),
    /// Acquire and update one token or every token.
    #[command(alias = "up")]
    Update(SelectionArgs),
    /// Restore one token or every token from Keychain backups.
    #[command(alias = "rb")]
    Rollback(SelectionArgs),
    /// Emit shell-safe environment assignments.
    #[command(alias = "en")]
    Env(EnvArgs),
    /// Inspect or install managed shell integration.
    #[command(alias = "sl")]
    Shell(ShellArgs),
}

#[derive(Debug, Args, Clone)]
pub struct ConfigSourceArgs {
    /// HTTPS URL, local TOML file, or '-' for standard input.
    #[arg(short, long, value_name = "URL_OR_PATH")]
    pub source: Option<String>,
    /// Local profile name. Defaults to the source file name.
    #[arg(long, requires = "source", value_name = "NAME")]
    pub profile: Option<String>,
}

#[derive(Debug, Args)]
pub struct EnvArgs {
    #[arg(value_enum)]
    pub shell: ShellKind,
}

#[derive(Debug, Args)]
pub struct ShellArgs {
    #[command(subcommand)]
    pub command: ShellCommand,
}

#[derive(Debug, Subcommand)]
pub enum ShellCommand {
    #[command(alias = "st")]
    Status(ShellTargetArgs),
    #[command(alias = "in")]
    Install(ShellTargetArgs),
}

#[derive(Debug, Args)]
pub struct ShellTargetArgs {
    #[arg(value_enum)]
    pub shell: ShellKind,
}

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("selection")
        .required(true)
        .multiple(false)
        .args(["name", "all"])
))]
pub struct SelectionArgs {
    #[arg(value_name = "NAME")]
    pub name: Option<String>,
    #[arg(short = 'a', long)]
    pub all: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError(String);

struct PreparedUpdate<'a> {
    secret: &'a SecretDefinition,
    value: Vec<u8>,
    previous: Option<Vec<u8>>,
}

struct PreparedRollback<'a> {
    secret: &'a SecretDefinition,
    previous: Option<Vec<u8>>,
}

pub fn execute(
    command: Command,
    registry: &SecretRegistry,
    store: &dyn SecretStore,
    acquirer: &dyn TokenAcquirer,
    shell_manager: &ShellIntegrationManager<'_>,
) -> Result<String, CommandError> {
    match command {
        Command::Bootstrap(_) | Command::Sync(_) => Err(CommandError(
            "command requires configuration management context".into(),
        )),
        Command::List => list(registry),
        Command::Show(arguments) if arguments.all => show_all(registry, store),
        Command::Show(arguments) => {
            let env = arguments
                .name
                .expect("clap requires either a name or --all");
            let secret = find(registry, &env)?;
            show_secret(secret, &[env.as_str()], store)
        }
        Command::Update(arguments) => update(arguments, registry, store, acquirer),
        Command::Rollback(arguments) => rollback(arguments, registry, store),
        Command::Env(arguments) => environment::render(arguments.shell, registry, store)
            .map_err(|error| CommandError(error.to_string())),
        Command::Shell(arguments) => shell_command(arguments.command, shell_manager),
    }
}

fn shell_command(
    command: ShellCommand,
    manager: &ShellIntegrationManager<'_>,
) -> Result<String, CommandError> {
    match command {
        ShellCommand::Status(arguments) => {
            let status = manager
                .status(arguments.shell)
                .map_err(|error| CommandError(error.to_string()))?;
            Ok(format!("{}: {status}\n", arguments.shell))
        }
        ShellCommand::Install(arguments) => {
            let outcome = manager
                .install(arguments.shell)
                .map_err(|error| CommandError(error.to_string()))?;
            let shell = arguments.shell;
            if !outcome.changed {
                return Ok(format!("{shell}: {}\n", outcome.status));
            }
            let mut output = format!("{shell}: current\n");
            if let Some(backup) = outcome.backup {
                output.push_str(&format!(
                    "Backup: Keychain service '{}', account '{}'\n",
                    backup.service, backup.account
                ));
            }
            Ok(output)
        }
    }
}

fn list(registry: &SecretRegistry) -> Result<String, CommandError> {
    let mut output = String::new();
    for secret in registry.secrets() {
        for env in &secret.envs {
            output.push_str(env);
            output.push('\n');
        }
    }
    Ok(output)
}

fn update(
    arguments: SelectionArgs,
    registry: &SecretRegistry,
    store: &dyn SecretStore,
    acquirer: &dyn TokenAcquirer,
) -> Result<String, CommandError> {
    let selected = select(arguments, registry)?;
    update_definitions(selected, store, acquirer)
}

pub(crate) fn update_definitions(
    selected: Vec<&SecretDefinition>,
    store: &dyn SecretStore,
    acquirer: &dyn TokenAcquirer,
) -> Result<String, CommandError> {
    let mut prepared = Vec::new();
    for secret in selected {
        let envs = secret.envs.iter().map(String::as_str).collect::<Vec<_>>();
        let value = acquirer
            .acquire(&secret.acquisition, &envs)
            .map_err(|error| {
                CommandError(format!("cannot acquire {}: {error}", envs.join(", ")))
            })?;
        validate_value(&secret.envs, &value)?;
        let previous = store.get(&secret.keychain_coordinates()).map_err(|error| {
            CommandError(format!(
                "cannot read Keychain account '{}': {error}",
                secret.account
            ))
        })?;
        prepared.push(PreparedUpdate {
            secret,
            value,
            previous,
        });
    }

    for update in &prepared {
        store
            .set(
                &backup_coordinates(update.secret),
                &encode_previous(update.previous.as_deref()),
            )
            .map_err(|error| {
                CommandError(format!(
                    "cannot back up Keychain account '{}': {error}",
                    update.secret.account
                ))
            })?;
    }

    for (index, update) in prepared.iter().enumerate() {
        if let Err(error) = store.set(&update.secret.keychain_coordinates(), &update.value) {
            restore_updates(&prepared[..index], store)?;
            return Err(CommandError(format!(
                "cannot update Keychain account '{}': {error}",
                update.secret.account
            )));
        }
    }

    Ok(render_status(
        "Updated",
        prepared.iter().map(|update| update.secret),
    ))
}

fn rollback(
    arguments: SelectionArgs,
    registry: &SecretRegistry,
    store: &dyn SecretStore,
) -> Result<String, CommandError> {
    let selected = select(arguments, registry)?;
    let mut prepared = Vec::new();
    for secret in selected {
        let backup = store
            .get(&backup_coordinates(secret))
            .map_err(|error| {
                CommandError(format!(
                    "cannot read backup for Keychain account '{}': {error}",
                    secret.account
                ))
            })?
            .ok_or_else(|| {
                CommandError(format!(
                    "Keychain account '{}' has no backup",
                    secret.account
                ))
            })?;
        prepared.push(PreparedRollback {
            secret,
            previous: decode_previous(&backup, &secret.account)?,
        });
    }

    for rollback in &prepared {
        restore(rollback.secret, rollback.previous.as_deref(), store)?;
    }
    Ok(render_status(
        "Restored",
        prepared.iter().map(|rollback| rollback.secret),
    ))
}

fn select(
    arguments: SelectionArgs,
    registry: &SecretRegistry,
) -> Result<Vec<&SecretDefinition>, CommandError> {
    if arguments.all {
        Ok(registry.secrets().iter().collect())
    } else {
        let env = arguments
            .name
            .expect("clap requires either a name or --all");
        Ok(vec![find(registry, &env)?])
    }
}

fn find<'a>(registry: &'a SecretRegistry, env: &str) -> Result<&'a SecretDefinition, CommandError> {
    registry
        .find(env)
        .ok_or_else(|| CommandError(format!("secret '{env}' is not registered")))
}

fn restore_updates(
    updates: &[PreparedUpdate<'_>],
    store: &dyn SecretStore,
) -> Result<(), CommandError> {
    for update in updates.iter().rev() {
        restore(update.secret, update.previous.as_deref(), store)?;
    }
    Ok(())
}

fn restore(
    secret: &SecretDefinition,
    previous: Option<&[u8]>,
    store: &dyn SecretStore,
) -> Result<(), CommandError> {
    let coordinates = secret.keychain_coordinates();
    let result = match previous {
        Some(value) => store.set(&coordinates, value),
        None => store.delete(&coordinates),
    };
    result.map_err(|error| {
        CommandError(format!(
            "cannot restore Keychain account '{}': {error}",
            secret.account
        ))
    })
}

fn backup_coordinates(secret: &SecretDefinition) -> KeychainCoordinates {
    KeychainCoordinates::new(BACKUP_SERVICE, &secret.account)
}

fn encode_previous(previous: Option<&[u8]>) -> Vec<u8> {
    match previous {
        Some(value) => {
            let mut encoded = Vec::with_capacity(value.len() + 1);
            encoded.push(1);
            encoded.extend_from_slice(value);
            encoded
        }
        None => vec![0],
    }
}

fn decode_previous(encoded: &[u8], account: &str) -> Result<Option<Vec<u8>>, CommandError> {
    match encoded.split_first() {
        Some((0, [])) => Ok(None),
        Some((1, value)) => Ok(Some(value.to_vec())),
        _ => Err(CommandError(format!(
            "Keychain account '{account}' has an invalid backup"
        ))),
    }
}

fn render_status<'a>(
    status: &str,
    secrets: impl IntoIterator<Item = &'a SecretDefinition>,
) -> String {
    let mut output = String::new();
    for secret in secrets {
        for env in &secret.envs {
            output.push_str(&format!("{status} {env}\n"));
        }
    }
    output
}

fn show_all(registry: &SecretRegistry, store: &dyn SecretStore) -> Result<String, CommandError> {
    let mut output = String::new();
    for secret in registry.secrets() {
        match read_value(secret, store)? {
            Some(value) => {
                for env in &secret.envs {
                    output.push_str(&format!("{env}={value}\n"));
                }
            }
            None => {
                for env in &secret.envs {
                    output.push_str(&format!("{env}=<missing>\n"));
                }
            }
        }
    }
    Ok(output)
}

fn show_secret(
    secret: &SecretDefinition,
    envs: &[&str],
    store: &dyn SecretStore,
) -> Result<String, CommandError> {
    let value = read_value(secret, store)?.ok_or_else(|| {
        CommandError(format!(
            "Keychain account '{}' has no value",
            secret.account
        ))
    })?;
    let mut output = String::new();
    for env in envs {
        output.push_str(&format!("{env}={value}\n"));
    }
    Ok(output)
}

fn read_value(
    secret: &SecretDefinition,
    store: &dyn SecretStore,
) -> Result<Option<String>, CommandError> {
    let Some(bytes) = store.get(&secret.keychain_coordinates()).map_err(|error| {
        CommandError(format!(
            "cannot read Keychain account '{}': {error}",
            secret.account
        ))
    })?
    else {
        return Ok(None);
    };
    validate_value(&secret.envs, &bytes)?;
    String::from_utf8(bytes).map(Some).map_err(|_| {
        CommandError(format!(
            "Keychain account '{}' contains a non-UTF-8 value",
            secret.account
        ))
    })
}

fn validate_value(envs: &[String], value: &[u8]) -> Result<(), CommandError> {
    let display = envs.join(", ");
    if std::str::from_utf8(value).is_err() {
        return Err(CommandError(format!(
            "secret '{display}' contains a non-UTF-8 value"
        )));
    }
    if value.is_empty() {
        return Err(CommandError(format!(
            "secret '{display}' contains an empty value"
        )));
    }
    Ok(())
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for CommandError {}
