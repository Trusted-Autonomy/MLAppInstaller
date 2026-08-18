use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
use tempfile::tempdir;

/// The wizard's full prompt sequence, in order, for a macOS target with
/// signing configured and a GitHub Releases deploy target.
const FULL_ANSWERS: &str = "\
my-app
manifest.toml
comp-a, comp-b
macos
dmg
keychain:Developer ID Application: Example, Inc.

y
example/my-app
";

#[test]
fn writes_a_complete_profile_from_full_answers() {
    let dir = tempdir().unwrap();
    let output_path = dir.path().join("distribution-profile.toml");

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("init")
        .arg("--output")
        .arg(&output_path)
        .write_stdin(FULL_ANSWERS);

    cmd.assert()
        .success()
        .stdout(contains("Wrote distribution profile"));

    let written = fs::read_to_string(&output_path).unwrap();
    assert!(written.contains("name = \"my-app\""));
    assert!(written.contains("\"comp-a\""));
    assert!(written.contains("\"comp-b\""));
    assert!(written.contains("platform = \"macos\""));
    assert!(written.contains("format = \"dmg\""));
    assert!(
        written.contains("signing_identity = \"keychain:Developer ID Application: Example, Inc.\"")
    );
    assert!(written.contains("adapter = \"github-releases\""));
    assert!(written.contains("repo = \"example/my-app\""));
}

/// Blank lines for every optional prompt (components, signing identity,
/// certificate thumbprint, repo) plus accepting every default (format,
/// deploy adapter) via blank lines too. Confirms the wizard produces a
/// valid, minimal profile with zero typing beyond the two truly required
/// answers (name and platform).
const MINIMAL_ANSWERS: &str = "\
minimal-app


linux



n

";

#[test]
fn writes_a_minimal_valid_profile_from_all_blank_optional_answers() {
    let dir = tempdir().unwrap();
    let output_path = dir.path().join("distribution-profile.toml");

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("init")
        .arg("--output")
        .arg(&output_path)
        .write_stdin(MINIMAL_ANSWERS);

    cmd.assert().success();

    let written = fs::read_to_string(&output_path).unwrap();
    let profile = mlai_package::profile::DistributionProfile::parse(&written)
        .expect("wizard must always write a parseable profile");
    assert_eq!(profile.distribution.name, "minimal-app");
    assert_eq!(profile.distribution.manifest, "manifest.toml"); // default applied
    assert!(profile.distribution.components.is_empty());
    assert_eq!(
        profile.targets[0].format,
        mlai_package::profile::PackageFormat::Deb
    ); // default for linux
    assert!(profile.targets[0].signing_identity.is_none());
    assert!(profile.deploy.is_none()); // declined with "n"
}
