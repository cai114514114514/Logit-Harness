use std::process::Command;

#[test]
fn doctor_explains_missing_credentials_without_printing_a_secret() {
    let home = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_weft"))
        .args(["--home", home.path().to_str().unwrap(), "doctor", "--json"])
        .env_remove("DEEPSEEK_API_KEY")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["model"], "deepseek-flash");
    assert_eq!(result["api_key_present"], false);
    assert_eq!(result["paid_probe_performed"], false);
}

#[test]
fn session_listing_works_before_any_model_configuration() {
    let home = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_weft"))
        .args([
            "--home",
            home.path().to_str().unwrap(),
            "sessions",
            "--json",
        ])
        .env_remove("DEEPSEEK_API_KEY")
        .output()
        .unwrap();
    assert!(output.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows, serde_json::json!([]));
}
