use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use ai_world::acquisition::{AcquisitionError, TokenAcquirer};
use ai_world::bootstrap::{BootstrapManager, BootstrapPrompt, BootstrapRequest};
use ai_world::config_sync::ConfigSource;
use ai_world::environment::ShellKind;
use ai_world::registry::Acquisition;
use ai_world::secret_store::{KeychainCoordinates, MemorySecretStore, SecretStore};
use ai_world::shell_integration::{Confirmer, IntegrationStatus};

const REGISTRY: &str = r#"
[secrets.present]
env = "PRESENT_TOKEN"
url = "https://oauth.example/present"

[secrets.missing]
envs = ["MISSING_TOKEN", "MISSING_TOKEN_ALIAS"]
url = "https://oauth.example/missing"
"#;

struct FakeSource {
    location: String,
    content: String,
}

impl ConfigSource for FakeSource {
    fn read(&self, location: &str) -> Result<Vec<u8>, io::Error> {
        if location != self.location {
            return Err(io::Error::new(io::ErrorKind::NotFound, "unknown source"));
        }
        Ok(self.content.as_bytes().to_vec())
    }
}

struct Yes;

impl Confirmer for Yes {
    fn confirm(&self, _prompt: &str) -> Result<bool, io::Error> {
        Ok(true)
    }
}

struct Prompt(String);

impl BootstrapPrompt for Prompt {
    fn configuration_source(&self) -> Result<String, io::Error> {
        Ok(self.0.clone())
    }
}

#[derive(Default)]
struct FakeAcquirer {
    calls: RefCell<Vec<String>>,
}

impl TokenAcquirer for FakeAcquirer {
    fn acquire(
        &self,
        acquisition: &Acquisition,
        _names: &[&str],
    ) -> Result<Vec<u8>, AcquisitionError> {
        self.calls.borrow_mut().push(acquisition.url.clone());
        Ok(b"new-value".to_vec())
    }
}

fn temporary_home(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is valid")
        .as_nanos();
    let home = std::env::temp_dir().join(format!("ai-world-{name}-{nonce}"));
    fs::create_dir(&home).expect("temporary home is created");
    home
}

#[test]
fn bootstrap_imports_configuration_and_acquires_only_missing_accounts() {
    let home = temporary_home("bootstrap-secrets");
    let location = "https://config.example/team.toml";
    let source = FakeSource {
        location: location.into(),
        content: REGISTRY.into(),
    };
    let store = MemorySecretStore::from_values(BTreeMap::from([(
        KeychainCoordinates::new("ai-world", "present"),
        "existing-value".into(),
    )]));
    let acquirer = FakeAcquirer::default();
    let prompt = Prompt(location.into());
    let manager = BootstrapManager::new(&home, &source, &store, &acquirer, &Yes, &prompt);

    let outcome = manager
        .run(BootstrapRequest::default(), &[])
        .expect("bootstrap succeeds");

    assert_eq!(outcome.secret_accounts, 2);
    assert_eq!(outcome.acquired_accounts, 1);
    assert_eq!(outcome.missing_accounts, 0);
    assert_eq!(
        acquirer.calls.borrow().as_slice(),
        ["https://oauth.example/missing"]
    );
    assert_eq!(
        store
            .get(&KeychainCoordinates::new("ai-world", "present"))
            .expect("present token reads")
            .expect("present token exists"),
        b"existing-value"
    );
    let rendered = outcome.render(false);
    assert!(rendered.contains("aiw bootstrap"));
    assert!(rendered.contains("2 account(s) available"));
    assert!(!rendered.contains("existing-value"));
    assert!(!rendered.contains("new-value"));
    fs::remove_dir_all(home).expect("temporary home is removed");
}

#[test]
fn bootstrap_installs_requested_shell_and_reports_current_state() {
    let home = temporary_home("bootstrap-shell");
    let secrets = home.join(".config/ai-world/secrets.d");
    fs::create_dir_all(&secrets).expect("configuration directory exists");
    fs::write(secrets.join("local.toml"), REGISTRY).expect("registry exists");
    let store = MemorySecretStore::from_values(BTreeMap::from([
        (
            KeychainCoordinates::new("ai-world", "present"),
            "present-value".into(),
        ),
        (
            KeychainCoordinates::new("ai-world", "missing"),
            "missing-value".into(),
        ),
    ]));
    let source = FakeSource {
        location: "unused".into(),
        content: String::new(),
    };
    let acquirer = FakeAcquirer::default();
    let prompt = Prompt("unused".into());
    let manager = BootstrapManager::new(&home, &source, &store, &acquirer, &Yes, &prompt);

    let outcome = manager
        .run(BootstrapRequest::default(), &[ShellKind::Zsh])
        .expect("bootstrap succeeds");

    assert_eq!(outcome.shells.len(), 1);
    assert_eq!(outcome.shells[0].shell, ShellKind::Zsh);
    assert_eq!(outcome.shells[0].status, IntegrationStatus::Current);
    assert!(home.join(".zshrc").exists());
    fs::remove_dir_all(home).expect("temporary home is removed");
}
