use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use ai_world::acquisition::{AcquisitionError, TokenAcquirer};
use ai_world::cli::{
    Cli, Command, CommandError, SelectionArgs, ShellCommand, execute as execute_command,
};
use ai_world::environment::ShellKind;
use ai_world::registry::{Acquisition, SecretRegistry};
use ai_world::secret_store::{
    KeychainCoordinates, MemorySecretStore, SecretStore, SecretStoreError,
};
use ai_world::shell_integration::{Confirmer, ShellIntegrationManager};
use clap::Parser;

const REGISTRY: &str = r#"
[secrets.alpha]
env = "ALPHA_TOKEN"
url = "https://oauth.example/alpha"

[secrets.shared]
envs = ["PRIMARY_TOKEN", "PRIMARY_TOKEN_ALIAS"]
url = "https://oauth.example/shared"
"#;

fn fixture() -> (SecretRegistry, MemorySecretStore) {
    let registry = SecretRegistry::from_toml(REGISTRY).expect("valid registry");
    let store = MemorySecretStore::from_values(BTreeMap::from([
        (
            KeychainCoordinates::new("ai-world", "alpha"),
            "alpha-value".into(),
        ),
        (
            KeychainCoordinates::new("ai-world", "shared"),
            "shared-value".into(),
        ),
    ]));
    (registry, store)
}

struct NoConfirmation;

impl Confirmer for NoConfirmation {
    fn confirm(&self, _prompt: &str) -> Result<bool, std::io::Error> {
        Ok(false)
    }
}

fn execute(
    command: Command,
    registry: &SecretRegistry,
    store: &dyn SecretStore,
    acquirer: &dyn TokenAcquirer,
) -> Result<String, CommandError> {
    let confirmer = NoConfirmation;
    let shell_manager = ShellIntegrationManager::new("/unused", &confirmer, store);
    execute_command(command, registry, store, acquirer, &shell_manager)
}

#[derive(Default)]
struct FakeAcquirer {
    values: BTreeMap<String, String>,
    calls: RefCell<Vec<String>>,
    fail_on: Option<String>,
}

impl TokenAcquirer for FakeAcquirer {
    fn acquire(
        &self,
        acquisition: &Acquisition,
        _envs: &[&str],
    ) -> Result<Vec<u8>, AcquisitionError> {
        self.calls.borrow_mut().push(acquisition.url.clone());
        if self.fail_on.as_deref() == Some(acquisition.url.as_str()) {
            return Err(AcquisitionError::new("test acquisition failed"));
        }
        self.values
            .get(&acquisition.url)
            .map(|value| value.as_bytes().to_vec())
            .ok_or_else(|| AcquisitionError::new("test value is missing"))
    }
}

struct FailOnSetStore {
    inner: MemorySecretStore,
    account: String,
    failed: Cell<bool>,
}

impl SecretStore for FailOnSetStore {
    fn get(&self, coordinates: &KeychainCoordinates) -> Result<Option<Vec<u8>>, SecretStoreError> {
        self.inner.get(coordinates)
    }

    fn set(&self, coordinates: &KeychainCoordinates, value: &[u8]) -> Result<(), SecretStoreError> {
        if coordinates.service == "ai-world"
            && coordinates.account == self.account
            && !self.failed.replace(true)
        {
            return Err(SecretStoreError::new("test write failed"));
        }
        self.inner.set(coordinates, value)
    }

    fn delete(&self, coordinates: &KeychainCoordinates) -> Result<(), SecretStoreError> {
        self.inner.delete(coordinates)
    }
}

#[test]
fn list_prints_every_environment_without_reading_values() {
    let (registry, store) = fixture();

    let output =
        execute(Command::List, &registry, &store, &FakeAcquirer::default()).expect("list succeeds");

    assert_eq!(output, "ALPHA_TOKEN\nPRIMARY_TOKEN\nPRIMARY_TOKEN_ALIAS\n");
    assert!(!output.contains("alpha-value"));
    assert_eq!(store.read_count(), 0);
}

#[test]
fn show_resolves_one_environment_alias() {
    let (registry, store) = fixture();

    let output = execute(
        Command::Show(SelectionArgs {
            name: Some("PRIMARY_TOKEN_ALIAS".into()),
            all: false,
        }),
        &registry,
        &store,
        &FakeAcquirer::default(),
    )
    .expect("show succeeds");

    assert_eq!(output, "PRIMARY_TOKEN_ALIAS=shared-value\n");
}

#[test]
fn show_all_reads_one_account_and_prints_all_aliases() {
    let (registry, store) = fixture();

    let output = execute(
        Command::Show(SelectionArgs {
            name: None,
            all: true,
        }),
        &registry,
        &store,
        &FakeAcquirer::default(),
    )
    .expect("show all succeeds");

    assert_eq!(
        output,
        "ALPHA_TOKEN=alpha-value\nPRIMARY_TOKEN=shared-value\nPRIMARY_TOKEN_ALIAS=shared-value\n"
    );
    assert_eq!(store.read_count(), 2);
}

#[test]
fn show_all_marks_missing_values_and_continues() {
    let registry = SecretRegistry::from_toml(REGISTRY).expect("valid registry");
    let store = MemorySecretStore::from_values(BTreeMap::from([(
        KeychainCoordinates::new("ai-world", "shared"),
        "shared-value".into(),
    )]));

    let output = execute(
        Command::Show(SelectionArgs {
            name: None,
            all: true,
        }),
        &registry,
        &store,
        &FakeAcquirer::default(),
    )
    .expect("show all succeeds with missing values");

    assert_eq!(
        output,
        "ALPHA_TOKEN=<missing>\nPRIMARY_TOKEN=shared-value\nPRIMARY_TOKEN_ALIAS=shared-value\n"
    );
}

#[test]
fn two_letter_command_aliases_parse() {
    assert!(matches!(
        Cli::try_parse_from(["aiw", "ls"])
            .expect("list alias parses")
            .command,
        Command::List
    ));
    assert!(matches!(
        Cli::try_parse_from(["aiw", "sh", "-a"])
            .expect("show alias parses")
            .command,
        Command::Show(_)
    ));
    assert!(matches!(
        Cli::try_parse_from(["aiw", "up", "-a"])
            .expect("update alias parses")
            .command,
        Command::Update(_)
    ));
    assert!(matches!(
        Cli::try_parse_from(["aiw", "rb", "PRIMARY_TOKEN"])
            .expect("rollback alias parses")
            .command,
        Command::Rollback(_)
    ));
    assert!(matches!(
        Cli::try_parse_from(["aiw", "en", "zsh"])
            .expect("environment alias parses")
            .command,
        Command::Env(arguments) if arguments.shell == ShellKind::Zsh
    ));
    assert!(matches!(
        Cli::try_parse_from(["aiw", "sl", "st", "fish"])
            .expect("shell aliases parse")
            .command,
        Command::Shell(arguments)
            if matches!(&arguments.command, ShellCommand::Status(target) if target.shell == ShellKind::Fish)
    ));
    assert!(matches!(
        Cli::try_parse_from(["aiw", "sy"])
            .expect("sync alias parses")
            .command,
        Command::Sync(arguments) if arguments.source.is_none()
    ));
    assert!(matches!(
        Cli::try_parse_from([
            "aiw",
            "bs",
            "--source",
            "https://config.example/team.toml"
        ])
        .expect("bootstrap alias parses")
        .command,
        Command::Bootstrap(arguments)
            if arguments.source.as_deref() == Some("https://config.example/team.toml")
    ));
}

#[test]
fn update_by_alias_changes_the_shared_keychain_account() {
    let (registry, store) = fixture();
    let acquirer = FakeAcquirer {
        values: BTreeMap::from([("https://oauth.example/shared".into(), "new-value".into())]),
        ..FakeAcquirer::default()
    };

    let update = Cli::try_parse_from(["aiw", "update", "PRIMARY_TOKEN_ALIAS"])
        .expect("update parses")
        .command;
    let output = execute(update, &registry, &store, &acquirer).expect("update succeeds");

    assert_eq!(
        output,
        "Updated PRIMARY_TOKEN\nUpdated PRIMARY_TOKEN_ALIAS\n"
    );
    let shown = execute(
        Cli::try_parse_from(["aiw", "show", "PRIMARY_TOKEN"])
            .expect("show parses")
            .command,
        &registry,
        &store,
        &acquirer,
    )
    .expect("show succeeds");
    assert_eq!(shown, "PRIMARY_TOKEN=new-value\n");
}

#[test]
fn update_all_acquires_each_keychain_account() {
    let registry = SecretRegistry::from_toml(
        r#"
        [secrets.one]
        env = "ONE_TOKEN"
        url = "https://oauth.example/shared"
        [secrets.two]
        env = "TWO_TOKEN"
        url = "https://oauth.example/shared"
        "#,
    )
    .expect("registry is valid");
    let store = MemorySecretStore::default();
    let acquirer = FakeAcquirer {
        values: BTreeMap::from([("https://oauth.example/shared".into(), "value".into())]),
        ..FakeAcquirer::default()
    };

    let update = Cli::try_parse_from(["aiw", "update", "--all"])
        .expect("update all parses")
        .command;
    execute(update, &registry, &store, &acquirer).expect("update succeeds");

    assert_eq!(acquirer.calls.borrow().len(), 2);
}

#[test]
fn update_all_does_not_write_before_every_value_is_acquired() {
    let (registry, store) = fixture();
    let acquirer = FakeAcquirer {
        values: BTreeMap::from([("https://oauth.example/alpha".into(), "new-alpha".into())]),
        fail_on: Some("https://oauth.example/shared".into()),
        ..FakeAcquirer::default()
    };

    let update = Cli::try_parse_from(["aiw", "update", "--all"])
        .expect("update all parses")
        .command;
    execute(update, &registry, &store, &acquirer).expect_err("second acquisition fails");

    let shown = execute(
        Cli::try_parse_from(["aiw", "show", "ALPHA_TOKEN"])
            .expect("show parses")
            .command,
        &registry,
        &store,
        &acquirer,
    )
    .expect("show succeeds");
    assert_eq!(shown, "ALPHA_TOKEN=alpha-value\n");
}

#[test]
fn update_all_rolls_back_values_after_a_write_failure() {
    let registry = SecretRegistry::from_toml(
        r#"
        [secrets.one]
        env = "ONE_TOKEN"
        url = "https://oauth.example/one"
        [secrets.two]
        env = "TWO_TOKEN"
        url = "https://oauth.example/two"
        "#,
    )
    .expect("registry is valid");
    let store = FailOnSetStore {
        inner: MemorySecretStore::from_values(BTreeMap::from([
            (
                KeychainCoordinates::new("ai-world", "one"),
                "old-one".into(),
            ),
            (
                KeychainCoordinates::new("ai-world", "two"),
                "old-two".into(),
            ),
        ])),
        account: "two".into(),
        failed: Cell::new(false),
    };
    let acquirer = FakeAcquirer {
        values: BTreeMap::from([
            ("https://oauth.example/one".into(), "new-one".into()),
            ("https://oauth.example/two".into(), "new-two".into()),
        ]),
        ..FakeAcquirer::default()
    };

    let update = Cli::try_parse_from(["aiw", "update", "--all"])
        .expect("update all parses")
        .command;
    execute(update, &registry, &store, &acquirer).expect_err("second write fails");

    let shown = execute(
        Cli::try_parse_from(["aiw", "show", "--all"])
            .expect("show all parses")
            .command,
        &registry,
        &store,
        &acquirer,
    )
    .expect("show all succeeds");
    assert_eq!(shown, "ONE_TOKEN=old-one\nTWO_TOKEN=old-two\n");
}

#[test]
fn rollback_restores_the_persistent_keychain_backup() {
    let (registry, store) = fixture();
    let acquirer = FakeAcquirer {
        values: BTreeMap::from([("https://oauth.example/shared".into(), "new-value".into())]),
        ..FakeAcquirer::default()
    };
    let update = Cli::try_parse_from(["aiw", "update", "PRIMARY_TOKEN_ALIAS"])
        .expect("update parses")
        .command;
    execute(update, &registry, &store, &acquirer).expect("update succeeds");

    let rollback = Cli::try_parse_from(["aiw", "rollback", "PRIMARY_TOKEN"])
        .expect("rollback parses")
        .command;
    let output = execute(rollback, &registry, &store, &acquirer).expect("rollback succeeds");

    assert_eq!(
        output,
        "Restored PRIMARY_TOKEN\nRestored PRIMARY_TOKEN_ALIAS\n"
    );
    let shown = execute(
        Cli::try_parse_from(["aiw", "show", "PRIMARY_TOKEN_ALIAS"])
            .expect("show parses")
            .command,
        &registry,
        &store,
        &acquirer,
    )
    .expect("show succeeds");
    assert_eq!(shown, "PRIMARY_TOKEN_ALIAS=shared-value\n");
}
