use loh::{
    cli::terminal::{Intent, parse},
    config::{Config, Paths, api},
};
use std::os::unix::fs::{PermissionsExt, symlink};

// Deliberately synthetic, never a real credential.
const KEY: &str = "fixture-setup-secret-47829";

#[test]
fn api_settings_keep_keys_separate_and_preserve_budget_values() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("state"));
    paths.prepare().unwrap();
    let config = Config {
        tokens: 123_456,
        seconds: 70,
        workers: 1,
        ..Config::default()
    };
    std::fs::write(
        paths.home.join("config.toml"),
        toml::to_string(&config).unwrap(),
    )
    .unwrap();
    api::save(&paths, "https://example.test/v1/", Some(KEY)).unwrap();
    let text = std::fs::read_to_string(paths.home.join("config.toml")).unwrap();
    assert!(!text.contains(KEY));
    let loaded = Config::load(&paths.home).unwrap();
    assert_eq!(loaded.base_url, "https://example.test/v1");
    assert_eq!(loaded.tokens, 123_456);
    assert_eq!(loaded.seconds, 70);
    assert_eq!(loaded.workers, 1);
    assert_eq!(
        api::saved_key(&paths, "https://example.test/v1/")
            .unwrap()
            .as_deref(),
        Some(KEY)
    );
    assert!(
        api::saved_key(&paths, "https://elsewhere.test")
            .unwrap()
            .is_none()
    );
    assert!(
        api::saved_key(&paths, "https://example.test/v2")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        std::fs::metadata(paths.home.join("credentials.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(&paths.home).unwrap().permissions().mode() & 0o777,
        0o700
    );
    api::save(&paths, "https://second.test", Some("second-fixture-key")).unwrap();
    assert_eq!(
        api::saved_key(&paths, "https://example.test/v1")
            .unwrap()
            .as_deref(),
        Some(KEY)
    );
}

#[test]
fn unsafe_endpoints_and_bad_keys_fail_without_writing_or_echoing_a_secret() {
    for url in [
        "http://remote.test",
        "https://secret@remote.test",
        "https://remote.test?api_key=secret",
        "https://remote.test/#secret",
        "https://remote.test/responses",
    ] {
        let error = api::endpoint(url).unwrap_err().to_string();
        assert!(!error.contains("secret"));
    }
    assert_eq!(
        api::endpoint("http://127.0.0.1:8080/").unwrap(),
        "http://127.0.0.1:8080"
    );
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("state"));
    let error = api::save(&paths, "https://api.deepseek.com", Some("secret\ninvalid"))
        .unwrap_err()
        .to_string();
    assert!(!error.contains("secret"));
    assert!(!paths.home.exists());
}

#[test]
fn shared_or_linked_credential_files_are_rejected_without_exposing_contents() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("state"));
    api::save(&paths, "https://api.deepseek.com", Some(KEY)).unwrap();
    let file = paths.home.join("credentials.json");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let error = api::saved_key(&paths, "https://api.deepseek.com")
        .unwrap_err()
        .to_string();
    assert!(!error.contains(KEY));
    assert!(error.contains("600"));
    let target = paths.home.join("original.json");
    std::fs::rename(&file, &target).unwrap();
    symlink(&target, &file).unwrap();
    assert!(api::saved_key(&paths, "https://api.deepseek.com").is_err());
}

#[test]
fn doctor_uses_saved_credentials_without_printing_them() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("state"));
    api::save(&paths, "https://api.deepseek.com", Some(KEY)).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_loh"))
        .args(["--home", paths.home.to_str().unwrap(), "doctor", "--json"])
        .env_remove("DEEPSEEK_API_KEY")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains(KEY));
    let result: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(result["api_key_present"], true);
    assert_eq!(result["api_key_source"], "credentials_file");
    assert_eq!(result["paid_probe_performed"], false);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_loh"))
        .args(["--home", paths.home.to_str().unwrap(), "doctor", "--json"])
        .env("DEEPSEEK_API_KEY", "fixture-environment-override")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("fixture-environment-override"));
    let result: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(result["api_key_source"], "environment");
    assert!(matches!(parse("/setup").unwrap(), Intent::Setup));
    assert!(parse("/setup secret").is_err());
}
