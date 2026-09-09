use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::registry::{SecretDefinition, SecretRegistry};
use crate::shell_integration::Confirmer;

const MAX_CONFIG_SIZE: usize = 1024 * 1024;

pub trait ConfigSource {
    fn read(&self, location: &str) -> Result<Vec<u8>, io::Error>;
}

#[derive(Default)]
pub struct SystemConfigSource;

impl ConfigSource for SystemConfigSource {
    fn read(&self, location: &str) -> Result<Vec<u8>, io::Error> {
        if location == "-" {
            let mut input = Vec::new();
            io::stdin()
                .lock()
                .take((MAX_CONFIG_SIZE + 1) as u64)
                .read_to_end(&mut input)?;
            return Ok(input);
        }
        if location.starts_with("https://") {
            let output = Command::new("/usr/bin/curl")
                .args([
                    "--fail",
                    "--silent",
                    "--show-error",
                    "--location",
                    "--proto",
                    "=https",
                    "--proto-redir",
                    "=https",
                    "--max-redirs",
                    "5",
                    "--max-filesize",
                    &MAX_CONFIG_SIZE.to_string(),
                    location,
                ])
                .output()?;
            if !output.status.success() {
                return Err(io::Error::other(format!(
                    "HTTPS download failed with status {}",
                    output.status
                )));
            }
            return Ok(output.stdout);
        }
        fs::read(location)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRequest {
    pub source: String,
    pub profile: Option<String>,
}

impl SyncRequest {
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            profile: None,
        }
    }

    pub fn with_profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStatus {
    Installed,
    Updated,
    Unchanged,
    Declined,
}

impl fmt::Display for SyncStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Installed => formatter.write_str("installed"),
            Self::Updated => formatter.write_str("updated"),
            Self::Unchanged => formatter.write_str("current"),
            Self::Declined => formatter.write_str("unchanged by user"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOutcome {
    pub profile: String,
    pub status: SyncStatus,
    pub added_accounts: usize,
    pub changed_accounts: usize,
    pub removed_accounts: usize,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigSyncError(String);

pub struct ConfigSyncManager<'a> {
    home: PathBuf,
    source: &'a dyn ConfigSource,
    confirmer: &'a dyn Confirmer,
}

impl<'a> ConfigSyncManager<'a> {
    pub fn new(
        home: impl Into<PathBuf>,
        source: &'a dyn ConfigSource,
        confirmer: &'a dyn Confirmer,
    ) -> Self {
        Self {
            home: home.into(),
            source,
            confirmer,
        }
    }

    pub fn sync(&self, request: SyncRequest) -> Result<SyncOutcome, ConfigSyncError> {
        let source = normalize_source(&request.source)?;
        let profile = request
            .profile
            .unwrap_or_else(|| profile_from_source(&source));
        validate_profile(&profile)?;
        let bytes = self.source.read(&source).map_err(|error| {
            ConfigSyncError(format!("cannot read configuration source: {error}"))
        })?;
        if bytes.is_empty() {
            return Err(ConfigSyncError("configuration source is empty".into()));
        }
        if bytes.len() > MAX_CONFIG_SIZE {
            return Err(ConfigSyncError(format!(
                "configuration exceeds the {MAX_CONFIG_SIZE}-byte limit"
            )));
        }
        let candidate = String::from_utf8(bytes)
            .map_err(|_| ConfigSyncError("configuration is not valid UTF-8".into()))?;
        let checksum = format!("{:x}", Sha256::digest(candidate.as_bytes()));
        let candidate_registry = SecretRegistry::from_toml(&candidate)
            .map_err(|error| ConfigSyncError(error.to_string()))?;
        self.validate_merged(&profile, &candidate)?;

        let target = self.registry_directory().join(format!("{profile}.toml"));
        let current = read_optional(&target)?;
        let diff = registry_diff(
            current
                .as_deref()
                .map(SecretRegistry::from_toml)
                .transpose()
                .map_err(|error| ConfigSyncError(error.to_string()))?
                .as_ref(),
            &candidate_registry,
        );
        let initial_status = if current.is_none() {
            SyncStatus::Installed
        } else if current.as_deref() == Some(candidate.as_str()) {
            SyncStatus::Unchanged
        } else {
            SyncStatus::Updated
        };

        if initial_status != SyncStatus::Unchanged {
            let action = if initial_status == SyncStatus::Installed {
                "Install"
            } else {
                "Update"
            };
            let prompt = format!(
                "{action} configuration profile '{profile}' (+{}, ~{}, -{} accounts)?",
                diff.added, diff.changed, diff.removed
            );
            let confirmed = self
                .confirmer
                .confirm(&prompt)
                .map_err(|error| ConfigSyncError(format!("cannot read confirmation: {error}")))?;
            if !confirmed {
                return Ok(SyncOutcome {
                    profile,
                    status: SyncStatus::Declined,
                    added_accounts: diff.added,
                    changed_accounts: diff.changed,
                    removed_accounts: diff.removed,
                    sha256: checksum,
                });
            }
        }

        if initial_status != SyncStatus::Unchanged {
            if let Some(previous) = current.as_deref() {
                self.back_up(&profile, previous)?;
            }
            write_atomic(&target, candidate.as_bytes())?;
        }
        if source != "-" {
            self.write_descriptor(&profile, &source, &checksum)?;
        }
        Ok(SyncOutcome {
            profile,
            status: initial_status,
            added_accounts: diff.added,
            changed_accounts: diff.changed,
            removed_accounts: diff.removed,
            sha256: checksum,
        })
    }

    pub fn sync_configured(&self) -> Result<Vec<SyncOutcome>, ConfigSyncError> {
        let directory = self.sources_directory();
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(ConfigSyncError(format!(
                    "cannot read source directory '{}': {error}",
                    directory.display()
                )));
            }
        };
        let mut paths = Vec::new();
        for entry in entries {
            let path = entry
                .map_err(|error| {
                    ConfigSyncError(format!("cannot read source directory entry: {error}"))
                })?
                .path();
            if path
                .extension()
                .is_some_and(|extension| extension == "toml")
            {
                if fs::symlink_metadata(&path)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
                {
                    return Err(ConfigSyncError(format!(
                        "refusing to read symbolic link '{}'",
                        path.display()
                    )));
                }
                paths.push(path);
            }
        }
        paths.sort();

        let mut outcomes = Vec::new();
        for path in paths {
            let input = fs::read_to_string(&path).map_err(|error| {
                ConfigSyncError(format!(
                    "cannot read source descriptor '{}': {error}",
                    path.display()
                ))
            })?;
            let descriptor: SourceDescriptor = toml::from_str(&input).map_err(|error| {
                ConfigSyncError(format!(
                    "invalid source descriptor '{}': {}",
                    path.display(),
                    error.message()
                ))
            })?;
            if descriptor.source.location == "-" {
                return Err(ConfigSyncError(format!(
                    "source descriptor '{}' cannot use standard input",
                    path.display()
                )));
            }
            outcomes.push(
                self.sync(
                    SyncRequest::new(descriptor.source.location)
                        .with_profile(descriptor.source.profile),
                )?,
            );
        }
        Ok(outcomes)
    }

    fn validate_merged(&self, profile: &str, candidate: &str) -> Result<(), ConfigSyncError> {
        let directory = self.registry_directory();
        let mut owned = Vec::new();
        match fs::read_dir(&directory) {
            Ok(entries) => {
                for entry in entries {
                    let path = entry
                        .map_err(|error| {
                            ConfigSyncError(format!("cannot read registry entry: {error}"))
                        })?
                        .path();
                    if path
                        .extension()
                        .is_some_and(|extension| extension == "toml")
                        && path.file_stem().and_then(|stem| stem.to_str()) != Some(profile)
                    {
                        let content = fs::read_to_string(&path).map_err(|error| {
                            ConfigSyncError(format!(
                                "cannot read configuration file '{}': {error}",
                                path.display()
                            ))
                        })?;
                        owned.push((path.display().to_string(), content));
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(ConfigSyncError(format!(
                    "cannot read configuration directory '{}': {error}",
                    directory.display()
                )));
            }
        }
        owned.push((format!("{profile}.toml"), candidate.to_owned()));
        SecretRegistry::from_sources(
            owned
                .iter()
                .map(|(name, content)| (name.as_str(), content.as_str())),
        )
        .map(|_| ())
        .map_err(|error| ConfigSyncError(error.to_string()))
    }

    fn write_descriptor(
        &self,
        profile: &str,
        source: &str,
        sha256: &str,
    ) -> Result<(), ConfigSyncError> {
        let descriptor = SourceDescriptor {
            source: SourceDetails {
                profile: profile.to_owned(),
                location: source.to_owned(),
                sha256: sha256.to_owned(),
            },
        };
        let content = toml::to_string(&descriptor).map_err(|error| {
            ConfigSyncError(format!("cannot encode source descriptor: {error}"))
        })?;
        write_atomic(
            &self.sources_directory().join(format!("{profile}.toml")),
            content.as_bytes(),
        )
    }

    fn back_up(&self, profile: &str, content: &str) -> Result<(), ConfigSyncError> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| ConfigSyncError(format!("system clock is invalid: {error}")))?
            .as_nanos();
        let path = self
            .home
            .join(".config/ai-world/backups/config")
            .join(timestamp.to_string())
            .join(format!("{profile}.toml"));
        write_atomic(&path, content.as_bytes())
    }

    fn registry_directory(&self) -> PathBuf {
        self.home.join(".config/ai-world/secrets.d")
    }

    fn sources_directory(&self) -> PathBuf {
        self.home.join(".config/ai-world/sources.d")
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceDescriptor {
    source: SourceDetails,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceDetails {
    profile: String,
    location: String,
    #[serde(default)]
    sha256: String,
}

struct RegistryDiff {
    added: usize,
    changed: usize,
    removed: usize,
}

fn registry_diff(current: Option<&SecretRegistry>, candidate: &SecretRegistry) -> RegistryDiff {
    let current = current.map(definitions).unwrap_or_default();
    let candidate = definitions(candidate);
    let current_accounts = current.keys().cloned().collect::<BTreeSet<_>>();
    let candidate_accounts = candidate.keys().cloned().collect::<BTreeSet<_>>();
    RegistryDiff {
        added: candidate_accounts.difference(&current_accounts).count(),
        removed: current_accounts.difference(&candidate_accounts).count(),
        changed: current_accounts
            .intersection(&candidate_accounts)
            .filter(|account| current.get(*account) != candidate.get(*account))
            .count(),
    }
}

fn definitions(registry: &SecretRegistry) -> BTreeMap<String, SecretDefinition> {
    registry
        .secrets()
        .iter()
        .map(|secret| (secret.account.clone(), secret.clone()))
        .collect()
}

fn normalize_source(source: &str) -> Result<String, ConfigSyncError> {
    if source == "-" {
        return Ok(source.to_owned());
    }
    if let Some(authority_and_path) = source.strip_prefix("https://") {
        let authority = authority_and_path
            .split_once('/')
            .map_or(authority_and_path, |(authority, _)| authority);
        if authority.is_empty() || authority.contains('@') {
            return Err(ConfigSyncError(
                "HTTPS configuration source has an invalid authority".into(),
            ));
        }
        return Ok(source.to_owned());
    }
    fs::canonicalize(source)
        .map(|path| path.display().to_string())
        .map_err(|error| ConfigSyncError(format!("cannot resolve configuration source: {error}")))
}

fn profile_from_source(source: &str) -> String {
    if source == "-" {
        return "pasted".into();
    }
    let without_query = source.split(['?', '#']).next().unwrap_or(source);
    Path::new(without_query)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("imported")
        .to_owned()
}

fn validate_profile(profile: &str) -> Result<(), ConfigSyncError> {
    if profile.is_empty()
        || !profile
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(ConfigSyncError(format!(
            "invalid configuration profile '{profile}'"
        )));
    }
    Ok(())
}

fn read_optional(path: &Path) -> Result<Option<String>, ConfigSyncError> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ConfigSyncError(format!(
            "cannot read '{}': {error}",
            path.display()
        ))),
    }
}

fn write_atomic(path: &Path, content: &[u8]) -> Result<(), ConfigSyncError> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(ConfigSyncError(format!(
            "refusing to replace symbolic link '{}'",
            path.display()
        )));
    }
    let parent = path.parent().ok_or_else(|| {
        ConfigSyncError(format!(
            "configuration path '{}' has no parent",
            path.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        ConfigSyncError(format!(
            "cannot create directory '{}': {error}",
            parent.display()
        ))
    })?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| ConfigSyncError(format!("system clock is invalid: {error}")))?
        .as_nanos();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config");
    let temporary = parent.join(format!(
        ".{name}.ai-world.tmp.{}-{nonce}",
        std::process::id()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(|error| {
        ConfigSyncError(format!("cannot create temporary configuration: {error}"))
    })?;
    let cleanup = TemporaryFile(temporary.clone());
    file.write_all(content).map_err(|error| {
        ConfigSyncError(format!("cannot write temporary configuration: {error}"))
    })?;
    file.sync_all().map_err(|error| {
        ConfigSyncError(format!("cannot sync temporary configuration: {error}"))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600)).map_err(|error| {
            ConfigSyncError(format!("cannot protect temporary configuration: {error}"))
        })?;
    }
    fs::rename(&temporary, path).map_err(|error| {
        ConfigSyncError(format!("cannot replace '{}': {error}", path.display()))
    })?;
    drop(cleanup);
    Ok(())
}

struct TemporaryFile(PathBuf);

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

impl fmt::Display for ConfigSyncError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ConfigSyncError {}

pub fn render_sync(outcomes: &[SyncOutcome], color: bool) -> String {
    let mut output = String::from("╭─ aiw sync\n│\n├─ Configuration\n");
    if outcomes.is_empty() {
        output.push_str("│  ○ No configured sources\n");
    }
    for outcome in outcomes {
        let marker = if outcome.status == SyncStatus::Declined {
            paint("○", "33", color)
        } else {
            paint("✓", "32", color)
        };
        output.push_str(&format!(
            "│  {marker} {}: {} (+{}, ~{}, -{})\n",
            outcome.profile,
            outcome.status,
            outcome.added_accounts,
            outcome.changed_accounts,
            outcome.removed_accounts
        ));
    }
    output.push_str("│\n╰─ Complete\n");
    output
}

fn paint(text: &str, code: &str, color: bool) -> String {
    if color {
        format!("\u{1b}[{code}m{text}\u{1b}[0m")
    } else {
        text.to_owned()
    }
}
