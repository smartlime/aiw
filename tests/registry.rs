use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use ai_world::registry::{KEYCHAIN_SERVICE, SecretRegistry};

#[test]
fn registry_accepts_env_and_envs() {
    let registry = SecretRegistry::from_toml(
        r#"
        [secrets.primary]
        env = "PRIMARY_TOKEN"
        url = "https://oauth.example/primary"

        [secrets.shared]
        envs = ["FIRST_TOKEN", "SECOND_TOKEN"]
        url = "https://oauth.example/shared"
        "#,
    )
    .expect("registry is valid");

    assert_eq!(registry.secrets().len(), 2);
    let shared = registry.find("SECOND_TOKEN").expect("alias resolves");
    assert_eq!(shared.account, "shared");
    assert_eq!(shared.envs, ["FIRST_TOKEN", "SECOND_TOKEN"]);
    assert_eq!(shared.keychain_coordinates().service, KEYCHAIN_SERVICE);
}

#[test]
fn registry_rejects_env_and_envs_together() {
    let error = SecretRegistry::from_toml(
        r#"
        [secrets.primary]
        env = "PRIMARY_TOKEN"
        envs = ["PRIMARY_TOKEN"]
        url = "https://oauth.example/primary"
        "#,
    )
    .expect_err("ambiguous environment fields must fail");

    assert_eq!(
        error.to_string(),
        "secret 'primary' must use either 'env' or 'envs'"
    );
}

#[test]
fn registry_rejects_duplicate_environment_names() {
    let error = SecretRegistry::from_toml(
        r#"
        [secrets.first]
        env = "API_TOKEN"
        url = "https://oauth.example/first"

        [secrets.second]
        envs = ["OTHER_TOKEN", "API_TOKEN"]
        url = "https://oauth.example/second"
        "#,
    )
    .expect_err("duplicate environments must fail");

    assert_eq!(error.to_string(), "duplicate environment name 'API_TOKEN'");
}

#[test]
fn registry_rejects_value_fields_without_echoing_the_value() {
    let error = SecretRegistry::from_toml(
        r#"
        [secrets.primary]
        env = "PRIMARY_TOKEN"
        url = "https://oauth.example/primary"
        value = "must-not-be-accepted"
        "#,
    )
    .expect_err("secret values must fail");

    assert!(error.to_string().contains("unknown field `value`"));
    assert!(!error.to_string().contains("must-not-be-accepted"));
}

#[test]
fn registry_merges_toml_files_in_one_directory() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is valid")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("ai-world-registry-{unique}"));
    fs::create_dir(&directory).expect("temporary directory is created");
    fs::write(
        directory.join("one.toml"),
        "[secrets.one]\nenv = \"ONE_TOKEN\"\nurl = \"https://oauth.example/one\"\n",
    )
    .expect("first file is written");
    fs::write(
        directory.join("two.toml"),
        "[secrets.two]\nenv = \"TWO_TOKEN\"\nurl = \"https://oauth.example/two\"\n",
    )
    .expect("second file is written");

    let registry = SecretRegistry::from_directory(&directory).expect("directory loads");

    assert!(registry.find("ONE_TOKEN").is_some());
    assert!(registry.find("TWO_TOKEN").is_some());
    fs::remove_dir_all(directory).expect("temporary directory is removed");
}

#[test]
fn registry_rejects_duplicate_accounts_across_files() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is valid")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("ai-world-duplicate-{unique}"));
    fs::create_dir(&directory).expect("temporary directory is created");
    for file in ["one.toml", "two.toml"] {
        fs::write(
            directory.join(file),
            "[secrets.shared]\nenv = \"TOKEN\"\nurl = \"https://oauth.example/token\"\n",
        )
        .expect("configuration file is written");
    }

    let error = SecretRegistry::from_directory(&directory)
        .expect_err("duplicate account across files must fail");

    assert_eq!(error.to_string(), "duplicate Keychain account 'shared'");
    fs::remove_dir_all(directory).expect("temporary directory is removed");
}

#[test]
fn tracked_example_uses_only_reserved_example_urls() {
    let example = include_str!("../config/secrets.example.toml");

    SecretRegistry::from_toml(example).expect("tracked example is valid");
    for line in example
        .lines()
        .filter(|line| line.trim().starts_with("url"))
    {
        assert!(line.contains("example.invalid"));
    }
}
