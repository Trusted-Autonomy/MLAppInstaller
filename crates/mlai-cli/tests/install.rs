use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
#[cfg(unix)]
use std::io::Write;
use tempfile::tempdir;

// This fixture's component declares only a posix setup script — on Windows,
// setup_for_current_os() returns None (no [components.setup.windows] entry),
// so setup never runs and the health check would correctly fail. Gated to
// unix until a Windows-native fixture is worth building.
#[cfg(unix)]
fn build_fixture_zip(path: &std::path::Path) {
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    zip.add_directory("hello-component-main/", options).unwrap();
    zip.start_file("hello-component-main/setup.sh", options)
        .unwrap();
    zip.write_all(b"#!/bin/sh\ntouch marker.txt\n").unwrap();
    zip.finish().unwrap();
}

#[test]
#[cfg(unix)]
fn install_command_installs_default_components_and_reports_healthy() {
    let mut server = mockito::Server::new();
    let zip_dir = tempdir().unwrap();
    let zip_path = zip_dir.path().join("bundle.zip");
    build_fixture_zip(&zip_path);
    let zip_bytes = fs::read(&zip_path).unwrap();

    let _mock = server
        .mock("GET", "/hello-component.zip")
        .with_status(200)
        .with_body(zip_bytes)
        .create();

    let manifest_dir = tempdir().unwrap();
    let manifest_path = manifest_dir.path().join("manifest.toml");
    fs::write(
        &manifest_path,
        format!(
            r#"
manifest_version = "1.0.0"

[[components]]
name = "hello-component"
source_url = "{}/hello-component.zip"
ref = "main"
default = true

[components.setup.posix]
command = "sh"
args = ["setup.sh"]

[components.health.posix]
type = "file_exists"
path = "marker.txt"
"#,
            server.url()
        ),
    )
    .unwrap();

    let install_root = tempdir().unwrap();

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("install")
        .arg("--manifest")
        .arg(&manifest_path)
        .arg("--install-root")
        .arg(install_root.path());

    cmd.assert()
        .success()
        .stdout(contains("hello-component -> healthy"));

    assert!(install_root
        .path()
        .join("hello-component")
        .join("marker.txt")
        .exists());
}

#[test]
fn install_command_fails_clearly_for_unknown_named_component() {
    let manifest_dir = tempdir().unwrap();
    let manifest_path = manifest_dir.path().join("manifest.toml");
    fs::write(
        &manifest_path,
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
    let install_root = tempdir().unwrap();

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("install")
        .arg("--manifest")
        .arg(&manifest_path)
        .arg("--install-root")
        .arg(install_root.path())
        .arg("--component")
        .arg("nonexistent");

    cmd.assert()
        .failure()
        .stderr(contains("no component named 'nonexistent'"));
}

#[test]
fn install_command_rejects_set_for_a_component_without_protocol_support() {
    let manifest_dir = tempdir().unwrap();
    let manifest_path = manifest_dir.path().join("manifest.toml");
    fs::write(
        &manifest_path,
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
    let install_root = tempdir().unwrap();

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("install")
        .arg("--manifest")
        .arg(&manifest_path)
        .arg("--install-root")
        .arg(install_root.path())
        .arg("--set")
        .arg("model=qwen3:14b");

    cmd.assert()
        .failure()
        .stderr(contains("does not declare supports_options_protocol"));
}

#[cfg(unix)]
#[test]
fn install_command_force_reinstalls_even_when_already_healthy() {
    let mut server = mockito::Server::new();
    let zip_dir = tempdir().unwrap();
    let zip_path = zip_dir.path().join("bundle.zip");
    build_fixture_zip(&zip_path);
    let zip_bytes = fs::read(&zip_path).unwrap();

    let mock = server
        .mock("GET", "/hello-component.zip")
        .with_status(200)
        .with_body(zip_bytes)
        .expect(2) // one real install, one forced reinstall
        .create();

    let manifest_dir = tempdir().unwrap();
    let manifest_path = manifest_dir.path().join("manifest.toml");
    fs::write(
        &manifest_path,
        format!(
            r#"
manifest_version = "1.0.0"

[[components]]
name = "hello-component"
source_url = "{}/hello-component.zip"
ref = "main"
default = true

[components.setup.posix]
command = "sh"
args = ["setup.sh"]

[components.health.posix]
type = "file_exists"
path = "marker.txt"
"#,
            server.url()
        ),
    )
    .unwrap();

    let install_root = tempdir().unwrap();

    let mut first = Command::cargo_bin("mlai").unwrap();
    first
        .arg("install")
        .arg("--manifest")
        .arg(&manifest_path)
        .arg("--install-root")
        .arg(install_root.path());
    first.assert().success();

    let mut forced = Command::cargo_bin("mlai").unwrap();
    forced
        .arg("install")
        .arg("--manifest")
        .arg(&manifest_path)
        .arg("--install-root")
        .arg(install_root.path())
        .arg("--force");
    forced
        .assert()
        .success()
        .stdout(contains("hello-component -> healthy"));

    mock.assert(); // fails the test if the second GET never happened
}
