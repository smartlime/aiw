use std::cell::Cell;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use ai_world::environment::ShellKind;
use ai_world::secret_store::{
    KeychainCoordinates, MemorySecretStore, SecretStore, SecretStoreError,
};
use ai_world::shell_integration::{Confirmer, IntegrationStatus, ShellIntegrationManager};

struct TestConfirmer {
    answer: bool,
    calls: Cell<usize>,
}

impl Confirmer for TestConfirmer {
    fn confirm(&self, _prompt: &str) -> Result<bool, std::io::Error> {
        self.calls.set(self.calls.get() + 1);
        Ok(self.answer)
    }
}

fn temporary_home(label: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is valid")
        .as_nanos();
    let home = std::env::temp_dir().join(format!("ai-world-{label}-{unique}"));
    fs::create_dir(&home).expect("temporary home is created");
    home
}

#[test]
fn zsh_install_detects_current_code_and_backs_up_drift() {
    let home = temporary_home("zsh-install");
    let zshrc = home.join(".zshrc");
    fs::write(&zshrc, "export ORIGINAL=1\n").expect("zshrc is written");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&zshrc, fs::Permissions::from_mode(0o640))
            .expect("zshrc permissions are set");
    }
    let confirmer = TestConfirmer {
        answer: true,
        calls: Cell::new(0),
    };
    let store = MemorySecretStore::default();
    let manager = ShellIntegrationManager::new(&home, &confirmer, &store);

    assert_eq!(
        manager.status(ShellKind::Zsh).unwrap(),
        IntegrationStatus::Missing
    );
    let first_install = manager
        .install(ShellKind::Zsh)
        .expect("integration installs");
    let first_backup = first_install.backup.expect("existing zshrc is backed up");
    assert_eq!(
        store.get(&first_backup).unwrap(),
        Some(b"export ORIGINAL=1\n".to_vec())
    );
    assert_eq!(
        manager.status(ShellKind::Zsh).unwrap(),
        IntegrationStatus::Current
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&zshrc).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
    assert_eq!(confirmer.calls.get(), 1);

    manager
        .install(ShellKind::Zsh)
        .expect("current integration is unchanged");
    assert_eq!(confirmer.calls.get(), 1);

    let drifted = fs::read_to_string(&zshrc)
        .expect("zshrc reads")
        .replace("aiw env zsh", "aiw old-command");
    fs::write(&zshrc, drifted).expect("drift is introduced");
    assert_eq!(
        manager.status(ShellKind::Zsh).unwrap(),
        IntegrationStatus::Drifted
    );

    let drift_install = manager.install(ShellKind::Zsh).expect("drift is replaced");
    assert_eq!(
        manager.status(ShellKind::Zsh).unwrap(),
        IntegrationStatus::Current
    );
    assert_eq!(confirmer.calls.get(), 2);
    let backup = store
        .get(&drift_install.backup.expect("drift is backed up"))
        .unwrap()
        .expect("backup exists");
    let backup = String::from_utf8(backup).expect("backup is text");
    assert!(backup.contains("aiw old-command"));
    assert!(backup.contains("ORIGINAL"));
    fs::remove_dir_all(home).expect("temporary home is removed");
}

#[test]
fn fish_install_respects_a_declined_confirmation() {
    let home = temporary_home("fish-decline");
    let confirmer = TestConfirmer {
        answer: false,
        calls: Cell::new(0),
    };
    let store = MemorySecretStore::default();
    let manager = ShellIntegrationManager::new(&home, &confirmer, &store);

    let outcome = manager
        .install(ShellKind::Fish)
        .expect("decline is handled");

    assert!(!outcome.changed);
    assert!(!home.join(".config/fish/conf.d/ai-world.fish").exists());
    assert_eq!(confirmer.calls.get(), 1);
    fs::remove_dir_all(home).expect("temporary home is removed");
}

#[test]
fn zsh_install_refuses_a_malformed_managed_block() {
    let home = temporary_home("zsh-malformed");
    let zshrc = home.join(".zshrc");
    let original = "export ORIGINAL=1\n# >>> ai-world >>>\naiw old-command\n";
    fs::write(&zshrc, original).expect("zshrc is written");
    let confirmer = TestConfirmer {
        answer: true,
        calls: Cell::new(0),
    };
    let store = MemorySecretStore::default();
    let manager = ShellIntegrationManager::new(&home, &confirmer, &store);

    assert_eq!(
        manager.status(ShellKind::Zsh).unwrap(),
        IntegrationStatus::Malformed
    );
    manager
        .install(ShellKind::Zsh)
        .expect_err("malformed integration must not be replaced");

    assert_eq!(confirmer.calls.get(), 0);
    assert_eq!(fs::read_to_string(&zshrc).unwrap(), original);
    fs::remove_dir_all(home).expect("temporary home is removed");
}

struct MutatingConfirmer {
    path: std::path::PathBuf,
    replacement: &'static str,
}

impl Confirmer for MutatingConfirmer {
    fn confirm(&self, _prompt: &str) -> Result<bool, std::io::Error> {
        fs::write(&self.path, self.replacement)?;
        Ok(true)
    }
}

#[test]
fn install_aborts_when_the_target_changes_during_confirmation() {
    let home = temporary_home("changed-during-confirmation");
    let zshrc = home.join(".zshrc");
    fs::write(&zshrc, "export ORIGINAL=1\n").expect("zshrc is written");
    let confirmer = MutatingConfirmer {
        path: zshrc.clone(),
        replacement: "# >>> ai-world >>>\naiw unexpected\n",
    };
    let store = MemorySecretStore::default();
    let manager = ShellIntegrationManager::new(&home, &confirmer, &store);

    let error = manager
        .install(ShellKind::Zsh)
        .expect_err("concurrent change must abort installation");

    assert!(
        error
            .to_string()
            .contains("changed while confirmation was pending")
    );
    assert_eq!(
        fs::read_to_string(&zshrc).unwrap(),
        "# >>> ai-world >>>\naiw unexpected\n"
    );
    fs::remove_dir_all(home).expect("temporary home is removed");
}

#[cfg(unix)]
struct PermissionMutatingConfirmer {
    path: std::path::PathBuf,
}

#[cfg(unix)]
impl Confirmer for PermissionMutatingConfirmer {
    fn confirm(&self, _prompt: &str) -> Result<bool, std::io::Error> {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))?;
        Ok(true)
    }
}

#[cfg(unix)]
#[test]
fn install_aborts_when_permissions_change_during_confirmation() {
    use std::os::unix::fs::PermissionsExt;

    let home = temporary_home("permissions-during-confirmation");
    let zshrc = home.join(".zshrc");
    fs::write(&zshrc, "export ORIGINAL=1\n").expect("zshrc is written");
    fs::set_permissions(&zshrc, fs::Permissions::from_mode(0o640))
        .expect("initial permissions are set");
    let confirmer = PermissionMutatingConfirmer {
        path: zshrc.clone(),
    };
    let store = MemorySecretStore::default();
    let manager = ShellIntegrationManager::new(&home, &confirmer, &store);

    manager
        .install(ShellKind::Zsh)
        .expect_err("permission change must abort installation");

    assert_eq!(
        fs::metadata(&zshrc).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::remove_dir_all(home).expect("temporary home is removed");
}

#[cfg(unix)]
#[test]
fn install_refuses_to_replace_a_symbolic_link() {
    use std::os::unix::fs::symlink;

    let home = temporary_home("zsh-symlink");
    let target = home.join("managed-zshrc");
    let zshrc = home.join(".zshrc");
    fs::write(&target, "export ORIGINAL=1\n").expect("target is written");
    symlink(&target, &zshrc).expect("symlink is created");
    let confirmer = TestConfirmer {
        answer: true,
        calls: Cell::new(0),
    };
    let store = MemorySecretStore::default();
    let manager = ShellIntegrationManager::new(&home, &confirmer, &store);

    manager
        .install(ShellKind::Zsh)
        .expect_err("symlink must not be replaced");

    assert!(
        fs::symlink_metadata(&zshrc)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(confirmer.calls.get(), 0);
    fs::remove_dir_all(home).expect("temporary home is removed");
}

struct RenameBlockingStore {
    target: std::path::PathBuf,
}

impl SecretStore for RenameBlockingStore {
    fn get(&self, _coordinates: &KeychainCoordinates) -> Result<Option<Vec<u8>>, SecretStoreError> {
        Ok(None)
    }

    fn set(
        &self,
        _coordinates: &KeychainCoordinates,
        _value: &[u8],
    ) -> Result<(), SecretStoreError> {
        fs::remove_file(&self.target).expect("target file is removed");
        fs::create_dir(&self.target).expect("blocking directory is created");
        Ok(())
    }

    fn delete(&self, _coordinates: &KeychainCoordinates) -> Result<(), SecretStoreError> {
        Ok(())
    }
}

#[test]
fn failed_atomic_replace_removes_its_temporary_file() {
    let home = temporary_home("temporary-cleanup");
    let zshrc = home.join(".zshrc");
    fs::write(&zshrc, "export ORIGINAL=1\n").expect("zshrc is written");
    let confirmer = TestConfirmer {
        answer: true,
        calls: Cell::new(0),
    };
    let store = RenameBlockingStore {
        target: zshrc.clone(),
    };
    let manager = ShellIntegrationManager::new(&home, &confirmer, &store);

    manager
        .install(ShellKind::Zsh)
        .expect_err("rename over a directory must fail");

    let temporary_files = fs::read_dir(&home)
        .expect("home is readable")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .contains(".ai-world.tmp.")
        })
        .count();
    assert_eq!(temporary_files, 0);
    fs::remove_dir_all(home).expect("temporary home is removed");
}
