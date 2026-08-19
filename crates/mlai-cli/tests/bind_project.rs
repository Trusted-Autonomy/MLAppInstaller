use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use std::fs;
#[cfg(unix)]
use std::io::Write;
use tempfile::tempdir;

#[test]
fn bind_project_fails_clearly_when_no_installed_component_matches_the_type() {
    let manifest_dir = tempdir().unwrap();
    let manifest_path = manifest_dir.path().join("manifest.toml");
    fs::write(
        &manifest_path,
        r#"
manifest_version = "1.0.0"

[[components]]
name = "ue5-plugin"
source_url = "https://example.com/ue5-plugin.zip"
ref = "main"
binds_to_project_type = "UE5"

[components.setup.posix]
command = "true"
args = []
"#,
    )
    .unwrap();
    let install_root = tempdir().unwrap();

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("bind-project")
        .arg("--manifest")
        .arg(&manifest_path)
        .arg("--install-root")
        .arg(install_root.path())
        .arg("--type")
        .arg("UE5")
        .arg("--path")
        .arg("/fake/MyGame.uproject");

    cmd.assert()
        .failure()
        .stderr(contains("UE5").and(contains("no installed component")));
}

#[cfg(unix)]
fn build_fixture_zip_with_project_placeholder(path: &std::path::Path) {
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    zip.add_directory("ue5-plugin-main/", options).unwrap();
    zip.start_file("ue5-plugin-main/setup.sh", options).unwrap();
    zip.write_all(b"#!/bin/sh\necho \"$@\" > argv.txt\ntouch marker.txt\n")
        .unwrap();
    zip.finish().unwrap();
}

#[cfg(unix)]
#[test]
fn bind_project_substitutes_the_real_path_for_an_installed_matching_component() {
    let mut server = mockito::Server::new();
    let zip_dir = tempdir().unwrap();
    let zip_path = zip_dir.path().join("bundle.zip");
    build_fixture_zip_with_project_placeholder(&zip_path);
    let zip_bytes = fs::read(&zip_path).unwrap();

    let _mock = server
        .mock("GET", "/ue5-plugin.zip")
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
name = "ue5-plugin"
source_url = "{}/ue5-plugin.zip"
ref = "main"
default = true
binds_to_project_type = "UE5"

[components.setup.posix]
command = "sh"
args = ["setup.sh", "-Project", "{{project}}"]

[components.health.posix]
type = "file_exists"
path = "marker.txt"
"#,
            server.url()
        ),
    )
    .unwrap();

    let install_root = tempdir().unwrap();

    let mut install_cmd = Command::cargo_bin("mlai").unwrap();
    install_cmd
        .arg("install")
        .arg("--manifest")
        .arg(&manifest_path)
        .arg("--install-root")
        .arg(install_root.path());
    install_cmd.assert().success();

    let mut bind_cmd = Command::cargo_bin("mlai").unwrap();
    bind_cmd
        .arg("bind-project")
        .arg("--manifest")
        .arg(&manifest_path)
        .arg("--install-root")
        .arg(install_root.path())
        .arg("--type")
        .arg("UE5")
        .arg("--path")
        .arg("/fake/MyGame.uproject");

    bind_cmd
        .assert()
        .success()
        .stdout(contains("ue5-plugin -> bound"));

    let argv = fs::read_to_string(install_root.path().join("ue5-plugin").join("argv.txt")).unwrap();
    assert!(argv.contains("/fake/MyGame.uproject"));
}
