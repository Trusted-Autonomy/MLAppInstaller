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

#[cfg(unix)]
#[test]
fn repair_fixes_a_component_broken_on_disk() {
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

    // Install once, then break it on disk without touching installed.json.
    let mut install_cmd = Command::cargo_bin("mlai").unwrap();
    install_cmd
        .arg("install")
        .arg("--manifest")
        .arg(&manifest_path)
        .arg("--install-root")
        .arg(install_root.path());
    install_cmd.assert().success();
    fs::remove_file(
        install_root
            .path()
            .join("hello-component")
            .join("marker.txt"),
    )
    .unwrap();

    let mut repair_cmd = Command::cargo_bin("mlai").unwrap();
    repair_cmd
        .arg("repair")
        .arg("--manifest")
        .arg(&manifest_path)
        .arg("--install-root")
        .arg(install_root.path());

    repair_cmd
        .assert()
        .success()
        .stdout(contains("hello-component -> repaired"));

    assert!(install_root
        .path()
        .join("hello-component")
        .join("marker.txt")
        .exists());
}

#[test]
fn repair_reports_already_healthy_without_reinstalling() {
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

    // No health check declared at all, and the component directory exists
    // -- check_health with no declared check is always Healthy, so repair
    // must report already-healthy without ever attempting a network fetch
    // (there's no mock server here at all; a real fetch attempt would fail
    // the test with a connection error).
    let install_root = tempdir().unwrap();
    fs::create_dir_all(install_root.path().join("hello-component")).unwrap();

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("repair")
        .arg("--manifest")
        .arg(&manifest_path)
        .arg("--install-root")
        .arg(install_root.path());

    cmd.assert()
        .success()
        .stdout(contains("hello-component -> already healthy"));
}
