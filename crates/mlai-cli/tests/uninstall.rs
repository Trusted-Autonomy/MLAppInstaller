use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
use tempfile::tempdir;

fn write_manifest(path: &std::path::Path) {
    fs::write(
        path,
        r#"
manifest_version = "1.0.0"

[[components]]
name = "hello-component"
source_url = "https://example.com/hello-component.zip"
ref = "main"
default = true
"#,
    )
    .unwrap();
}

#[test]
fn uninstall_with_yes_removes_the_component_directory() {
    let manifest_dir = tempdir().unwrap();
    let manifest_path = manifest_dir.path().join("manifest.toml");
    write_manifest(&manifest_path);

    let install_root = tempdir().unwrap();
    fs::create_dir_all(install_root.path().join("hello-component")).unwrap();

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("uninstall")
        .arg("--manifest")
        .arg(&manifest_path)
        .arg("--install-root")
        .arg(install_root.path())
        .arg("--yes");

    cmd.assert().success().stdout(contains("Removed 1"));
    assert!(!install_root.path().join("hello-component").exists());
}

#[test]
fn uninstall_dry_run_reports_without_deleting() {
    let manifest_dir = tempdir().unwrap();
    let manifest_path = manifest_dir.path().join("manifest.toml");
    write_manifest(&manifest_path);

    let install_root = tempdir().unwrap();
    fs::create_dir_all(install_root.path().join("hello-component")).unwrap();

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("uninstall")
        .arg("--manifest")
        .arg(&manifest_path)
        .arg("--install-root")
        .arg(install_root.path())
        .arg("--dry-run")
        .arg("--yes");

    cmd.assert().success().stdout(contains("Would remove 1"));
    assert!(install_root.path().join("hello-component").exists());
}

#[test]
fn uninstall_without_yes_or_a_tty_fails_clearly_rather_than_hanging() {
    let manifest_dir = tempdir().unwrap();
    let manifest_path = manifest_dir.path().join("manifest.toml");
    write_manifest(&manifest_path);

    let install_root = tempdir().unwrap();
    fs::create_dir_all(install_root.path().join("hello-component")).unwrap();

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("uninstall")
        .arg("--manifest")
        .arg(&manifest_path)
        .arg("--install-root")
        .arg(install_root.path())
        .write_stdin(""); // EOF immediately — simulates a non-interactive/no-tty run

    cmd.assert()
        .failure()
        .stderr(contains("confirmation required"));
    assert!(install_root.path().join("hello-component").exists());
}
