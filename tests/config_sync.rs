use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use ai_world::config_sync::{
    ConfigSource, ConfigSyncManager, SyncRequest, SyncStatus, SystemConfigSource,
};
use ai_world::registry::SecretRegistry;
use ai_world::shell_integration::Confirmer;

const FIRST: &str = r#"
[secrets.alpha]
env = "ALPHA_TOKEN"
url = "https://oauth.example/alpha"
"#;

const SECOND: &str = r#"
[secrets.alpha]
env = "ALPHA_TOKEN"
url = "https://oauth.example/alpha"

[secrets.beta]
env = "BETA_TOKEN"
url = "https://oauth.example/beta"
"#;

#[derive(Default)]
struct FakeSource {
    values: RefCell<BTreeMap<String, String>>,
}

impl ConfigSource for FakeSource {
    fn read(&self, location: &str) -> Result<Vec<u8>, io::Error> {
        self.values
            .borrow()
            .get(location)
            .map(|value| value.as_bytes().to_vec())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "test source is missing"))
    }
}

struct Confirmation(bool);

impl Confirmer for Confirmation {
    fn confirm(&self, _prompt: &str) -> Result<bool, io::Error> {
        Ok(self.0)
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
fn url_source_is_validated_installed_and_remembered() {
    let home = temporary_home("config-url");
    let location = "https://config.example/team.toml";
    let source = FakeSource {
        values: RefCell::new(BTreeMap::from([(location.into(), FIRST.into())])),
    };
    let manager = ConfigSyncManager::new(&home, &source, &Confirmation(true));

    let outcome = manager
        .sync(SyncRequest::new(location))
        .expect("configuration installs");

    assert_eq!(outcome.profile, "team");
    assert_eq!(outcome.status, SyncStatus::Installed);
    assert_eq!(outcome.added_accounts, 1);
    let registry = SecretRegistry::from_directory(&home.join(".config/ai-world/secrets.d"))
        .expect("installed registry loads");
    assert!(registry.find("ALPHA_TOKEN").is_some());
    let descriptor = fs::read_to_string(home.join(".config/ai-world/sources.d/team.toml"))
        .expect("source descriptor exists");
    assert!(descriptor.contains(location));
    assert!(descriptor.contains(&outcome.sha256));
    assert_eq!(outcome.sha256.len(), 64);
    assert!(!descriptor.contains("ALPHA_TOKEN"));
    fs::remove_dir_all(home).expect("temporary home is removed");
}

#[test]
fn configured_source_can_be_synchronized_again() {
    let home = temporary_home("config-repeat");
    let location = "https://config.example/team.toml";
    let source = FakeSource {
        values: RefCell::new(BTreeMap::from([(location.into(), FIRST.into())])),
    };
    let manager = ConfigSyncManager::new(&home, &source, &Confirmation(true));
    manager
        .sync(SyncRequest::new(location))
        .expect("initial configuration installs");
    source
        .values
        .borrow_mut()
        .insert(location.into(), SECOND.into());

    let outcomes = manager
        .sync_configured()
        .expect("configured source synchronizes");

    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].status, SyncStatus::Updated);
    assert_eq!(outcomes[0].added_accounts, 1);
    let registry = SecretRegistry::from_directory(&home.join(".config/ai-world/secrets.d"))
        .expect("updated registry loads");
    assert!(registry.find("BETA_TOKEN").is_some());
    fs::remove_dir_all(home).expect("temporary home is removed");
}

#[test]
fn invalid_source_never_replaces_the_active_configuration() {
    let home = temporary_home("config-invalid");
    let location = "https://config.example/team.toml";
    let source = FakeSource {
        values: RefCell::new(BTreeMap::from([(location.into(), FIRST.into())])),
    };
    let manager = ConfigSyncManager::new(&home, &source, &Confirmation(true));
    manager
        .sync(SyncRequest::new(location))
        .expect("initial configuration installs");
    source.values.borrow_mut().insert(
        location.into(),
        r#"
        [secrets.alpha]
        env = "ALPHA_TOKEN"
        url = "https://oauth.example/alpha"
        value = "must-not-be-stored"
        "#
        .into(),
    );

    let error = manager
        .sync_configured()
        .expect_err("secret value in configuration is rejected");

    assert!(!error.to_string().contains("must-not-be-stored"));
    assert_eq!(
        fs::read_to_string(home.join(".config/ai-world/secrets.d/team.toml"))
            .expect("active configuration remains"),
        FIRST
    );
    fs::remove_dir_all(home).expect("temporary home is removed");
}

#[test]
fn declined_change_leaves_configuration_untouched() {
    let home = temporary_home("config-declined");
    let location = "https://config.example/team.toml";
    let source = FakeSource {
        values: RefCell::new(BTreeMap::from([(location.into(), FIRST.into())])),
    };
    ConfigSyncManager::new(&home, &source, &Confirmation(true))
        .sync(SyncRequest::new(location))
        .expect("initial configuration installs");
    source
        .values
        .borrow_mut()
        .insert(location.into(), SECOND.into());

    let outcome = ConfigSyncManager::new(&home, &source, &Confirmation(false))
        .sync_configured()
        .expect("declined synchronization succeeds")
        .remove(0);

    assert_eq!(outcome.status, SyncStatus::Declined);
    assert_eq!(
        fs::read_to_string(home.join(".config/ai-world/secrets.d/team.toml"))
            .expect("active configuration remains"),
        FIRST
    );
    fs::remove_dir_all(home).expect("temporary home is removed");
}

#[test]
fn system_source_reads_a_local_file() {
    let home = temporary_home("config-local-source");
    let path = home.join("team.toml");
    fs::write(&path, FIRST).expect("local source exists");

    let content = SystemConfigSource
        .read(path.to_str().expect("path is UTF-8"))
        .expect("local source reads");

    assert_eq!(content, FIRST.as_bytes());
    fs::remove_dir_all(home).expect("temporary home is removed");
}

#[test]
fn candidate_is_rejected_when_it_duplicates_an_active_account() {
    let home = temporary_home("config-duplicate");
    let directory = home.join(".config/ai-world/secrets.d");
    fs::create_dir_all(&directory).expect("configuration directory exists");
    fs::write(directory.join("local.toml"), FIRST).expect("active profile exists");
    let location = "https://config.example/team.toml";
    let source = FakeSource {
        values: RefCell::new(BTreeMap::from([(location.into(), FIRST.into())])),
    };

    let error = ConfigSyncManager::new(&home, &source, &Confirmation(true))
        .sync(SyncRequest::new(location))
        .expect_err("duplicate account is rejected");

    assert_eq!(error.to_string(), "duplicate Keychain account 'alpha'");
    assert!(!directory.join("team.toml").exists());
    fs::remove_dir_all(home).expect("temporary home is removed");
}
