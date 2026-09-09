use std::error::Error;
use std::fmt;

use clap::ValueEnum;

use crate::registry::SecretRegistry;
use crate::secret_store::SecretStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ShellKind {
    Zsh,
    Fish,
}

impl fmt::Display for ShellKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zsh => formatter.write_str("zsh"),
            Self::Fish => formatter.write_str("fish"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentError(String);

pub fn render(
    shell: ShellKind,
    registry: &SecretRegistry,
    store: &dyn SecretStore,
) -> Result<String, EnvironmentError> {
    let mut output = String::new();
    for secret in registry.secrets() {
        let Some(bytes) = store.get(&secret.keychain_coordinates()).map_err(|error| {
            EnvironmentError(format!(
                "cannot read Keychain account '{}': {error}",
                secret.account
            ))
        })?
        else {
            continue;
        };
        let value = String::from_utf8(bytes).map_err(|_| {
            EnvironmentError(format!(
                "Keychain account '{}' contains a non-UTF-8 value",
                secret.account
            ))
        })?;
        if value.is_empty() {
            return Err(EnvironmentError(format!(
                "Keychain account '{}' contains an empty value",
                secret.account
            )));
        }
        for env in &secret.envs {
            match shell {
                ShellKind::Zsh => {
                    output.push_str(&format!("export {env}='{}'\n", quote_zsh(&value)));
                }
                ShellKind::Fish => {
                    output.push_str(&format!("set -gx {env} '{}';\n", quote_fish(&value)));
                }
            }
        }
    }
    Ok(output)
}

fn quote_zsh(value: &str) -> String {
    value.replace('\'', "'\\''")
}

fn quote_fish(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

impl fmt::Display for EnvironmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for EnvironmentError {}
