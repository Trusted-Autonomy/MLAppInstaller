use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
use std::io::Write;
use tempfile::tempdir;

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

[components.setup]
command = "sh"
args = ["setup.sh"]

[components.health]
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
