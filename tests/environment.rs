use std::collections::BTreeMap;

use ai_world::environment::{ShellKind, render};
use ai_world::registry::SecretRegistry;
use ai_world::secret_store::{KeychainCoordinates, MemorySecretStore};

const REGISTRY: &str = r#"
[secrets.present]
envs = ["PRIMARY_TOKEN", "PRIMARY_TOKEN_ALIAS"]
url = "https://oauth.example/present"

[secrets.missing]
env = "MISSING_TOKEN"
url = "https://oauth.example/missing"
"#;

#[test]
fn zsh_environment_quotes_values_and_skips_missing_accounts() {
    let registry = SecretRegistry::from_toml(REGISTRY).expect("registry is valid");
    let store = MemorySecretStore::from_values(BTreeMap::from([(
        KeychainCoordinates::new("ai-world", "present"),
        "a'b".into(),
    )]));

    let output = render(ShellKind::Zsh, &registry, &store).expect("environment renders");

    assert_eq!(
        output,
        "export PRIMARY_TOKEN='a'\\''b'\nexport PRIMARY_TOKEN_ALIAS='a'\\''b'\n"
    );
}

#[test]
fn fish_environment_quotes_values_and_skips_missing_accounts() {
    let registry = SecretRegistry::from_toml(REGISTRY).expect("registry is valid");
    let store = MemorySecretStore::from_values(BTreeMap::from([(
        KeychainCoordinates::new("ai-world", "present"),
        "a\\b'c".into(),
    )]));

    let output = render(ShellKind::Fish, &registry, &store).expect("environment renders");

    assert_eq!(
        output,
        "set -gx PRIMARY_TOKEN 'a\\\\b\\'c';\nset -gx PRIMARY_TOKEN_ALIAS 'a\\\\b\\'c';\n"
    );
}
