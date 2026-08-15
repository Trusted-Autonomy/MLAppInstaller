use assert_cmd::Command;
use predicates::str::contains;
use tempfile::tempdir;

#[test]
fn credential_set_stores_a_secret_read_from_stdin() {
    let vault_dir = tempdir().unwrap();

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("credential")
        .arg("set")
        .arg("test-api-key")
        .arg("--vault-dir")
        .arg(vault_dir.path())
        .write_stdin("sk-secret-value\n");

    cmd.assert()
        .success()
        .stdout(contains("Stored credential 'test-api-key'"));

    let raw = std::fs::read(vault_dir.path().join("credentials.age")).unwrap();
    let raw_str = String::from_utf8_lossy(&raw);
    assert!(!raw_str.contains("sk-secret-value"));
}
