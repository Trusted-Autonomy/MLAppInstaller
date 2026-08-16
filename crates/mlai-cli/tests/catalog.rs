use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use std::fs;
use tempfile::tempdir;

fn write_catalog(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    path
}

#[test]
fn resolve_prints_the_matching_model_to_stdout() {
    let dir = tempdir().unwrap();
    let catalog_path = write_catalog(
        dir.path(),
        "catalog.toml",
        r#"
[purposes.text-structured-json]
owner = "cinepipe-stories"

[[purposes.text-structured-json.tiers]]
min_vram_gb = 8
model = "qwen3:8b"
"#,
    );

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("catalog")
        .arg("resolve")
        .arg("--purpose")
        .arg("text-structured-json")
        .arg("--catalog")
        .arg(&catalog_path)
        .arg("--os")
        .arg("linux")
        .arg("--gpu-vendor")
        .arg("nvidia")
        .arg("--vram-gb")
        .arg("10")
        .arg("--effective-vram-gb")
        .arg("10")
        .arg("--disk-free-gb")
        .arg("100");

    cmd.assert().success().stdout(contains("qwen3:8b"));
}

#[test]
fn resolve_fails_clearly_when_two_catalogs_conflict() {
    let dir = tempdir().unwrap();
    let a = write_catalog(
        dir.path(),
        "a.toml",
        r#"
[purposes.text-structured-json]
owner = "cinepipe-stories"

[[purposes.text-structured-json.tiers]]
min_vram_gb = 8
model = "qwen3:8b"
"#,
    );
    let b = write_catalog(
        dir.path(),
        "b.toml",
        r#"
[purposes.text-structured-json]
owner = "cinepipe-director"

[[purposes.text-structured-json.tiers]]
min_vram_gb = 8
model = "llama3:8b"
"#,
    );

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("catalog")
        .arg("resolve")
        .arg("--purpose")
        .arg("text-structured-json")
        .arg("--catalog")
        .arg(&a)
        .arg("--catalog")
        .arg(&b)
        .arg("--os")
        .arg("linux")
        .arg("--gpu-vendor")
        .arg("nvidia")
        .arg("--vram-gb")
        .arg("10")
        .arg("--effective-vram-gb")
        .arg("10")
        .arg("--disk-free-gb")
        .arg("100");

    cmd.assert()
        .failure()
        .stderr(contains("cinepipe-stories").and(contains("cinepipe-director")));
}

#[test]
fn resolve_fails_clearly_when_nothing_matches() {
    let dir = tempdir().unwrap();
    let catalog_path = write_catalog(
        dir.path(),
        "catalog.toml",
        r#"
[purposes.text-structured-json]
owner = "cinepipe-stories"

[[purposes.text-structured-json.tiers]]
min_vram_gb = 24
model = "qwen3:32b"
"#,
    );

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("catalog")
        .arg("resolve")
        .arg("--purpose")
        .arg("text-structured-json")
        .arg("--catalog")
        .arg(&catalog_path)
        .arg("--os")
        .arg("linux")
        .arg("--gpu-vendor")
        .arg("nvidia")
        .arg("--vram-gb")
        .arg("4")
        .arg("--effective-vram-gb")
        .arg("4")
        .arg("--disk-free-gb")
        .arg("100");

    cmd.assert().failure().stderr(contains(
        "no model in 'text-structured-json' fits this hardware profile",
    ));
}
