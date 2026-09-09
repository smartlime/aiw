use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::secret_store::KeychainCoordinates;

pub const KEYCHAIN_SERVICE: &str = "ai-world";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRegistry {
    secrets: Vec<SecretDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretDefinition {
    pub account: String,
    pub envs: Vec<String>,
    pub acquisition: Acquisition,
}

impl SecretDefinition {
    pub fn keychain_coordinates(&self) -> KeychainCoordinates {
        KeychainCoordinates::new(KEYCHAIN_SERVICE, &self.account)
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Acquisition {
    pub url: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryFile {
    secrets: BTreeMap<String, RegistryEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryEntry {
    env: Option<String>,
    envs: Option<Vec<String>>,
    url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryError(String);

impl SecretRegistry {
    pub fn from_toml(input: &str) -> Result<Self, RegistryError> {
        Self::from_sources([("registry", input)])
    }

    pub fn from_directory(path: &Path) -> Result<Self, RegistryError> {
        let entries = fs::read_dir(path).map_err(|error| {
            RegistryError(format!(
                "cannot read configuration directory '{}': {error}",
                path.display()
            ))
        })?;
        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                RegistryError(format!(
                    "cannot read configuration directory entry '{}': {error}",
                    path.display()
                ))
            })?;
            let file_path = entry.path();
            if file_path
                .extension()
                .is_some_and(|extension| extension == "toml")
            {
                paths.push(file_path);
            }
        }
        paths.sort();

        let mut sources = Vec::new();
        for file_path in &paths {
            let input = fs::read_to_string(file_path).map_err(|error| {
                RegistryError(format!(
                    "cannot read configuration file '{}': {error}",
                    file_path.display()
                ))
            })?;
            sources.push((file_path.display().to_string(), input));
        }
        Self::from_sources(
            sources
                .iter()
                .map(|(name, input)| (name.as_str(), input.as_str())),
        )
    }

    pub fn secrets(&self) -> &[SecretDefinition] {
        &self.secrets
    }

    pub fn find(&self, env: &str) -> Option<&SecretDefinition> {
        self.secrets
            .iter()
            .find(|secret| secret.envs.iter().any(|candidate| candidate == env))
    }

    pub(crate) fn from_sources<'a>(
        sources: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, RegistryError> {
        let mut secrets = Vec::new();
        let mut accounts = HashSet::new();
        let mut envs = HashSet::new();

        for (source, input) in sources {
            let file: RegistryFile = toml::from_str(input).map_err(|error| {
                RegistryError(format!(
                    "invalid secret configuration '{source}': {}",
                    error.message()
                ))
            })?;
            for (account, entry) in file.secrets {
                if account.is_empty() || account.chars().any(char::is_whitespace) {
                    return Err(RegistryError(format!(
                        "invalid Keychain account '{account}'"
                    )));
                }
                if !accounts.insert(account.clone()) {
                    return Err(RegistryError(format!(
                        "duplicate Keychain account '{account}'"
                    )));
                }
                let entry_envs = match (entry.env, entry.envs) {
                    (Some(env), None) => vec![env],
                    (None, Some(envs)) if !envs.is_empty() => envs,
                    (Some(_), Some(_)) => {
                        return Err(RegistryError(format!(
                            "secret '{account}' must use either 'env' or 'envs'"
                        )));
                    }
                    _ => {
                        return Err(RegistryError(format!(
                            "secret '{account}' must define 'env' or non-empty 'envs'"
                        )));
                    }
                };
                for env in &entry_envs {
                    if !is_environment_name(env) {
                        return Err(RegistryError(format!("invalid environment name '{env}'")));
                    }
                    if !envs.insert(env.clone()) {
                        return Err(RegistryError(format!("duplicate environment name '{env}'")));
                    }
                }
                if !entry.url.starts_with("https://") || entry.url.chars().any(char::is_whitespace)
                {
                    return Err(RegistryError(format!(
                        "secret '{account}' has an invalid acquisition URL"
                    )));
                }
                secrets.push(SecretDefinition {
                    account,
                    envs: entry_envs,
                    acquisition: Acquisition { url: entry.url },
                });
            }
        }
        Ok(Self { secrets })
    }
}

fn is_environment_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('A'..='Z' | '_'))
        && chars.all(|character| matches!(character, 'A'..='Z' | '0'..='9' | '_'))
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for RegistryError {}
