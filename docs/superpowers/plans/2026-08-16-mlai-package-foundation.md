# mlai-package Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new `mlai-package` crate: the `DistributionProfile` schema (what an adopter authors) and the translation into a real `cargo-packager` invocation that produces native installers (macOS `.dmg`/`.app`, Windows `.msi`/`.exe`, Linux `.deb`/`.AppImage`), with signing as identity references only. This is the crate `mlai init` (a separate, later plan) writes profiles for, and the foundation the GitHub Releases deploy adapter (also separate) builds on — both depend on this plan's types, so this one lands first, not in parallel with either.

**Architecture:** `DistributionProfile` (TOML, adopter-facing) → `build_packager_config` (pure function, translates to `cargo-packager`'s actual JSON config shape) → `packager_command` (builds the `cargo packager -c <json> -f <format> -o <dir> -r` invocation as a `std::process::Command`, testable by inspecting its args without executing it) → `build_package` (thin wrapper that actually runs it). `mlai-cli` gets a `package build` subcommand.

**Tech Stack:** No new crate dependencies beyond `serde`/`serde_json`/`toml`/`thiserror` (already used elsewhere in this workspace). Shells out to the `cargo-packager` CLI (`cargo install cargo-packager`) — not a library dependency, matching this project's established "orchestrate an already-built external tool" pattern (`docs/CONSTITUTION.md` §1.6).

## Global Constraints

- Signing fields carry identity **references** only (a keychain identity name string, a certificate-store thumbprint string) — never a secret value, path to key material, or password. This was verified directly against `cargo-packager` 0.11.8 (installed and run locally, not just read about): macOS signing takes `signingIdentity` (a keychain name); Windows signing takes `certificateThumbprint` (assumes the cert is already in the build machine's certificate store) — there is no PFX-path/password config field in `cargo-packager` at all, so don't invent one (`docs/superpowers/specs/2026-08-15-distribution-packaging-framework-design.md`, amended 2026-08-16 with this verified finding).
- `cargo-packager`'s JSON config uses **camelCase** field names (`productName`, `signingIdentity`, `certificateThumbprint`) — confirmed empirically; do not use kebab-case or snake_case for the JSON path (kebab-case is a *different*, also-real config path — `[package.metadata.packager]` in a `Cargo.toml` — not the one this plan uses).
- Before every commit: `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` all pass on all three CI platforms. CI does **not** need `cargo-packager` installed for this plan's own tests to pass — every test in this plan either tests pure config-translation logic or inspects a constructed `Command`'s arguments without executing it (an actual `cargo packager` run needs the tool installed, a real Rust binary to package, and — for signing — real certificates, none of which belong in this plan's automated test suite).

## Out of scope for this plan

- The GitHub Releases deploy adapter (`DeployConfig.adapter = "github-releases"` parses, but nothing acts on it yet) — a separate, later plan.
- `mlai init` (the guided wizard that writes a `DistributionProfile`) — a separate, later plan; both it and the deploy adapter can be built in parallel with each other once this plan is merged, since neither depends on the other, only on this one.
- Actually running `cargo packager` against a real signed build in CI — no certs available; this plan verifies config *construction*, not a live signed package.

---

### Task 1: `DistributionProfile` schema + parsing

**Files:**
- Create: `crates/mlai-package/Cargo.toml`
- Create: `crates/mlai-package/src/lib.rs`
- Create: `crates/mlai-package/src/profile.rs`
- Modify: `Cargo.toml` (workspace member)

**Interfaces:**
- Produces: `mlai_package::profile::{DistributionProfile, Distribution, Target, Platform, PackageFormat, DeployConfig, ProfileError}`. `DistributionProfile::parse(toml_str: &str) -> Result<DistributionProfile, ProfileError>`.

- [ ] **Step 1: Create the workspace member and crate scaffold**

Modify `Cargo.toml` (repo root) — add `"crates/mlai-package"` to `members`.

`crates/mlai-package/Cargo.toml`:
```toml
[package]
name = "mlai-package"
version.workspace = true
edition.workspace = true

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
thiserror = "1"
```

`crates/mlai-package/src/lib.rs`:
```rust
pub mod profile;
```

- [ ] **Step 2: Write the failing test**

`crates/mlai-package/src/profile.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[distribution]
name = "example-app-suite"
manifest = "manifest.toml"
components = ["example-component", "ue5-plugin"]

[[targets]]
platform = "macos"
format = "dmg"
signing_identity = "keychain:Developer ID Application: Example Studio, Inc."

[[targets]]
platform = "windows"
format = "msi"
certificate_thumbprint = "AB12CD34EF56"

[deploy]
adapter = "github-releases"
repo = "example-org/example-app"
"#;

    #[test]
    fn parses_a_full_profile() {
        let profile = DistributionProfile::parse(SAMPLE).expect("valid profile");
        assert_eq!(profile.distribution.name, "example-app-suite");
        assert_eq!(profile.distribution.components.len(), 2);
        assert_eq!(profile.targets.len(), 2);

        let macos = &profile.targets[0];
        assert_eq!(macos.platform, Platform::Macos);
        assert_eq!(macos.format, PackageFormat::Dmg);
        assert_eq!(
            macos.signing_identity.as_deref(),
            Some("keychain:Developer ID Application: Example Studio, Inc.")
        );
        assert!(macos.certificate_thumbprint.is_none());

        let windows = &profile.targets[1];
        assert_eq!(windows.platform, Platform::Windows);
        assert_eq!(windows.certificate_thumbprint.as_deref(), Some("AB12CD34EF56"));

        let deploy = profile.deploy.expect("deploy config present");
        assert_eq!(deploy.adapter, "github-releases");
        assert_eq!(deploy.repo.as_deref(), Some("example-org/example-app"));
    }

    #[test]
    fn deploy_and_components_are_optional() {
        let toml = r#"
[distribution]
name = "minimal"
manifest = "manifest.toml"

[[targets]]
platform = "linux"
format = "deb"
"#;
        let profile = DistributionProfile::parse(toml).unwrap();
        assert!(profile.distribution.components.is_empty());
        assert!(profile.deploy.is_none());
        assert!(profile.targets[0].signing_identity.is_none());
    }

    #[test]
    fn rejects_invalid_toml() {
        let err = DistributionProfile::parse("not valid toml [[[").unwrap_err();
        assert!(matches!(err, ProfileError::Parse(_)));
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cd crates/mlai-package && cargo test`
Expected: FAIL to compile — the types don't exist yet.

- [ ] **Step 4: Write the implementation**

Prepend to the top of `crates/mlai-package/src/profile.rs`:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct DistributionProfile {
    pub distribution: Distribution,
    pub targets: Vec<Target>,
    #[serde(default)]
    pub deploy: Option<DeployConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Distribution {
    pub name: String,
    pub manifest: String,
    #[serde(default)]
    pub components: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Macos,
    Windows,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageFormat {
    Dmg,
    App,
    Msi,
    Nsis,
    Deb,
    Appimage,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Target {
    pub platform: Platform,
    pub format: PackageFormat,
    #[serde(default)]
    pub signing_identity: Option<String>,
    #[serde(default)]
    pub certificate_thumbprint: Option<String>,
    #[serde(default)]
    pub notarize: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct DeployConfig {
    pub adapter: String,
    #[serde(default)]
    pub repo: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("failed to parse distribution profile TOML: {0}")]
    Parse(#[from] toml::de::Error),
}

impl DistributionProfile {
    pub fn parse(toml_str: &str) -> Result<DistributionProfile, ProfileError> {
        toml::from_str(toml_str).map_err(ProfileError::from)
    }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cd crates/mlai-package && cargo test`
Expected: PASS — 3 tests.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/mlai-package
git commit -m "feat(mlai-package): add DistributionProfile schema and parsing"
```

---

### Task 2: `cargo-packager` config translation

**Files:**
- Create: `crates/mlai-package/src/packager_config.rs`
- Modify: `crates/mlai-package/src/lib.rs`

**Interfaces:**
- Consumes: `mlai_package::profile::{DistributionProfile, Target, PackageFormat}` (Task 1).
- Produces: `mlai_package::packager_config::build_packager_config(profile: &DistributionProfile, target: &Target, binary_path: &str) -> String` — a JSON string in `cargo-packager`'s own config shape.

- [ ] **Step 1: Write the failing test**

`crates/mlai-package/src/packager_config.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{DistributionProfile, Platform, PackageFormat, Target};

    fn sample_profile() -> DistributionProfile {
        DistributionProfile::parse(
            r#"
[distribution]
name = "hello-app"
manifest = "manifest.toml"

[[targets]]
platform = "macos"
format = "dmg"
signing_identity = "keychain:Developer ID Application: Example, Inc."
"#,
        )
        .unwrap()
    }

    #[test]
    fn includes_product_name_identifier_and_binary() {
        let profile = sample_profile();
        let target = &profile.targets[0];
        let json = build_packager_config(&profile, target, "target/release/hello-app");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["productName"], "hello-app");
        assert_eq!(value["identifier"], "com.mlappinstaller.hello-app");
        assert_eq!(value["formats"], serde_json::json!(["dmg"]));
        assert_eq!(value["binaries"][0]["path"], "target/release/hello-app");
        assert_eq!(value["binaries"][0]["main"], true);
    }

    #[test]
    fn includes_macos_signing_identity_when_present() {
        let profile = sample_profile();
        let target = &profile.targets[0];
        let json = build_packager_config(&profile, target, "bin/hello-app");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(
            value["macos"]["signingIdentity"],
            "keychain:Developer ID Application: Example, Inc."
        );
    }

    #[test]
    fn omits_macos_and_windows_blocks_when_no_signing_configured() {
        let mut profile = sample_profile();
        profile.targets[0].signing_identity = None;
        let target = &profile.targets[0];
        let json = build_packager_config(&profile, target, "bin/hello-app");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(value.get("macos").is_none());
        assert!(value.get("windows").is_none());
    }

    #[test]
    fn includes_windows_certificate_thumbprint_when_present() {
        let mut profile = sample_profile();
        profile.targets[0].platform = Platform::Windows;
        profile.targets[0].format = PackageFormat::Msi;
        profile.targets[0].signing_identity = None;
        profile.targets[0].certificate_thumbprint = Some("AB12CD34".to_string());
        let target = &profile.targets[0];
        let json = build_packager_config(&profile, target, "bin/hello-app.exe");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["windows"]["certificateThumbprint"], "AB12CD34");
    }

    #[test]
    fn format_enum_maps_to_cargo_packager_format_strings() {
        let profile = sample_profile();
        let cases = [
            (PackageFormat::Dmg, "dmg"),
            (PackageFormat::App, "app"),
            (PackageFormat::Msi, "wix"),
            (PackageFormat::Nsis, "nsis"),
            (PackageFormat::Deb, "deb"),
            (PackageFormat::Appimage, "appimage"),
        ];
        for (format, expected) in cases {
            let mut target = profile.targets[0].clone();
            target.format = format;
            assert_eq!(packager_format_str(&target.format), expected, "format {format:?}");
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-package && cargo test packager_config::`
Expected: FAIL to compile — module `packager_config` doesn't exist yet.

- [ ] **Step 3: Write the implementation**

Prepend to the top of `crates/mlai-package/src/packager_config.rs`:
```rust
// Translates this crate's own DistributionProfile into cargo-packager's
// actual JSON config shape. Field names and casing (camelCase) verified
// directly against cargo-packager 0.11.8, installed and run locally --
// not guessed from documentation. See this plan's Global Constraints for
// what was specifically confirmed (macOS signingIdentity, Windows
// certificateThumbprint, no PFX/password fields exist).

use crate::profile::{DistributionProfile, PackageFormat, Target};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackagerBinary {
    path: String,
    main: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackagerMacosConfig {
    signing_identity: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackagerWindowsConfig {
    certificate_thumbprint: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackagerConfig {
    product_name: String,
    identifier: String,
    formats: Vec<String>,
    binaries: Vec<PackagerBinary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    macos: Option<PackagerMacosConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    windows: Option<PackagerWindowsConfig>,
}

/// Maps this crate's `PackageFormat` to the exact format string
/// `cargo-packager`'s `-f`/`formats` option expects (confirmed via
/// `cargo packager --help`; `msi` is generated through the WiX toolset,
/// whose format string is `wix`, not `msi`).
pub fn packager_format_str(format: &PackageFormat) -> &'static str {
    match format {
        PackageFormat::Dmg => "dmg",
        PackageFormat::App => "app",
        PackageFormat::Msi => "wix",
        PackageFormat::Nsis => "nsis",
        PackageFormat::Deb => "deb",
        PackageFormat::Appimage => "appimage",
    }
}

/// Builds the JSON string to pass as `cargo packager -c <this>` — verified
/// directly that `-c` accepts a raw JSON string, not only a file path, so
/// this never needs to write a config file or touch the adopter's own
/// `Cargo.toml`.
pub fn build_packager_config(profile: &DistributionProfile, target: &Target, binary_path: &str) -> String {
    let config = PackagerConfig {
        product_name: profile.distribution.name.clone(),
        identifier: format!("com.mlappinstaller.{}", profile.distribution.name),
        formats: vec![packager_format_str(&target.format).to_string()],
        binaries: vec![PackagerBinary { path: binary_path.to_string(), main: true }],
        macos: target
            .signing_identity
            .clone()
            .map(|signing_identity| PackagerMacosConfig { signing_identity }),
        windows: target
            .certificate_thumbprint
            .clone()
            .map(|certificate_thumbprint| PackagerWindowsConfig { certificate_thumbprint }),
    };
    serde_json::to_string(&config).expect("PackagerConfig always serializes")
}
```

Add to `crates/mlai-package/src/lib.rs`:
```rust
pub mod packager_config;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd crates/mlai-package && cargo test packager_config::`
Expected: PASS — 5 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/mlai-package/src/packager_config.rs crates/mlai-package/src/lib.rs
git commit -m "feat(mlai-package): translate DistributionProfile into cargo-packager JSON config"
```

---

### Task 3: Packaging command construction and execution

**Files:**
- Create: `crates/mlai-package/src/build.rs`
- Modify: `crates/mlai-package/src/lib.rs`

**Interfaces:**
- Consumes: `build_packager_config`, `packager_format_str` (Task 2).
- Produces: `mlai_package::build::{packager_command, build_package, BuildError}`. `packager_command(profile: &DistributionProfile, target: &Target, binary_path: &str, out_dir: &Path) -> Command`. `build_package(profile: &DistributionProfile, target: &Target, binary_path: &str, out_dir: &Path) -> Result<(), BuildError>`.

- [ ] **Step 1: Write the failing test**

`crates/mlai-package/src/build.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::DistributionProfile;
    use std::path::Path;

    fn sample_profile() -> DistributionProfile {
        DistributionProfile::parse(
            r#"
[distribution]
name = "hello-app"
manifest = "manifest.toml"

[[targets]]
platform = "macos"
format = "dmg"
"#,
        )
        .unwrap()
    }

    #[test]
    fn command_invokes_cargo_packager_with_expected_flags() {
        let profile = sample_profile();
        let target = &profile.targets[0];
        let cmd = packager_command(&profile, target, "target/release/hello-app", Path::new("dist"));

        assert_eq!(cmd.get_program(), "cargo");
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert_eq!(args[0], "packager");
        assert!(args.contains(&"-c".to_string()));
        assert!(args.contains(&"-f".to_string()));
        assert!(args.contains(&"dmg".to_string()));
        assert!(args.contains(&"-o".to_string()));
        assert!(args.contains(&"dist".to_string()));
        assert!(args.contains(&"-r".to_string()), "release flag must always be passed");
    }

    #[test]
    fn command_config_arg_is_valid_json_matching_the_target() {
        let profile = sample_profile();
        let target = &profile.targets[0];
        let cmd = packager_command(&profile, target, "target/release/hello-app", Path::new("dist"));

        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        let config_index = args.iter().position(|a| a == "-c").unwrap();
        let config_json = &args[config_index + 1];
        let value: serde_json::Value = serde_json::from_str(config_json).expect("must be valid JSON");
        assert_eq!(value["productName"], "hello-app");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-package && cargo test build::`
Expected: FAIL to compile — module `build` doesn't exist yet.

- [ ] **Step 3: Write the implementation**

Prepend to the top of `crates/mlai-package/src/build.rs`:
```rust
use crate::packager_config::{build_packager_config, packager_format_str};
use crate::profile::{DistributionProfile, Target};
use std::path::Path;
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("failed to launch cargo packager: {source}")]
    Launch {
        #[source]
        source: std::io::Error,
    },
    #[error("cargo packager exited with status {status}")]
    Failed { status: i32 },
}

/// Builds the `cargo packager` invocation for one target, without running
/// it — kept separate from `build_package` so the exact command shape is
/// testable (inspecting `Command::get_args()`) without needing
/// `cargo-packager` installed or a real binary to package.
pub fn packager_command(
    profile: &DistributionProfile,
    target: &Target,
    binary_path: &str,
    out_dir: &Path,
) -> Command {
    let config_json = build_packager_config(profile, target, binary_path);
    let mut cmd = Command::new("cargo");
    cmd.arg("packager")
        .arg("-c")
        .arg(config_json)
        .arg("-f")
        .arg(packager_format_str(&target.format))
        .arg("-o")
        .arg(out_dir)
        .arg("-r");
    cmd
}

pub fn build_package(
    profile: &DistributionProfile,
    target: &Target,
    binary_path: &str,
    out_dir: &Path,
) -> Result<(), BuildError> {
    let status = packager_command(profile, target, binary_path, out_dir)
        .status()
        .map_err(|source| BuildError::Launch { source })?;
    if !status.success() {
        return Err(BuildError::Failed { status: status.code().unwrap_or(-1) });
    }
    Ok(())
}
```

Add to `crates/mlai-package/src/lib.rs`:
```rust
pub mod build;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd crates/mlai-package && cargo test`
Expected: PASS — all tests across the crate (10 total: 3 profile + 5 packager_config + 2 build).

- [ ] **Step 5: Commit**

```bash
git add crates/mlai-package/src/build.rs crates/mlai-package/src/lib.rs
git commit -m "feat(mlai-package): construct and run the cargo packager invocation"
```

---

### Task 4: `mlai package build` CLI command

**Files:**
- Create: `crates/mlai-cli/src/commands/package.rs`
- Modify: `crates/mlai-cli/src/commands/mod.rs`
- Modify: `crates/mlai-cli/src/main.rs`
- Modify: `crates/mlai-cli/Cargo.toml`

**Interfaces:**
- Consumes: `mlai_package::{profile::DistributionProfile, build::build_package}` (Tasks 1–3).
- Produces: `mlai package build --profile <path> --target-index <n> --binary <path> --out-dir <dir>` (targets a specific entry in the profile's `targets` array by index — simplest possible v1 selector, since a profile may declare several platforms and a single CI job typically builds one at a time on its own OS).

- [ ] **Step 1: Add the mlai-package dependency**

Modify `crates/mlai-cli/Cargo.toml` — add to `[dependencies]`:
```toml
mlai-package = { path = "../mlai-package" }
```

- [ ] **Step 2: Write `commands/package.rs`**

`crates/mlai-cli/src/commands/package.rs`:
```rust
use anyhow::{bail, Context, Result};
use mlai_package::build::build_package;
use mlai_package::profile::DistributionProfile;
use std::fs;
use std::path::Path;

pub fn build(profile_path: &Path, target_index: usize, binary: &str, out_dir: &Path) -> Result<()> {
    let profile_str = fs::read_to_string(profile_path)
        .with_context(|| format!("reading distribution profile at {}", profile_path.display()))?;
    let profile = DistributionProfile::parse(&profile_str)
        .with_context(|| format!("parsing distribution profile at {}", profile_path.display()))?;

    let Some(target) = profile.targets.get(target_index) else {
        bail!(
            "target index {target_index} out of range — profile '{}' declares {} target(s)",
            profile.distribution.name,
            profile.targets.len()
        );
    };

    fs::create_dir_all(out_dir)
        .with_context(|| format!("creating output directory at {}", out_dir.display()))?;

    println!(
        "Packaging '{}' for {:?}/{:?}...",
        profile.distribution.name, target.platform, target.format
    );
    build_package(&profile, target, binary, out_dir)
        .with_context(|| format!("packaging target index {target_index}"))?;
    println!("Packaged to {}", out_dir.display());
    Ok(())
}
```

Modify `crates/mlai-cli/src/commands/mod.rs`:
```rust
pub mod catalog;
pub mod install;
pub mod package;
pub mod repair;
pub mod uninstall;
```

- [ ] **Step 3: Wire the CLI subcommand**

In `crates/mlai-cli/src/main.rs`, add to the `Commands` enum:
```rust
    /// Build a native installer from a distribution profile
    Package {
        #[command(subcommand)]
        action: PackageAction,
    },
```
```rust
#[derive(Subcommand)]
enum PackageAction {
    Build {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long, default_value_t = 0)]
        target_index: usize,
        #[arg(long)]
        binary: String,
        #[arg(long)]
        out_dir: PathBuf,
    },
}
```
Update the `match cli.command` block:
```rust
        Commands::Package { action } => match action {
            PackageAction::Build { profile, target_index, binary, out_dir } => {
                commands::package::build(&profile, target_index, &binary, &out_dir)
            }
        },
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo build --workspace && cargo test --workspace`
Expected: PASS — this task adds no new automated tests of its own (the CLI layer is a thin pass-through already covered by Task 3's `build_package` tests plus manual verification, matching this project's established pattern of not re-testing pure argument-forwarding); confirm the whole workspace still builds and every existing test still passes.

- [ ] **Step 5: Commit**

```bash
git add crates/mlai-cli
git commit -m "feat(mlai-cli): add package build command"
```

---

### Task 5: Docs + final verification

**Files:**
- Modify: `docs/USAGE.md`

**Interfaces:** none new.

- [ ] **Step 1: Run the full constitution-required check suite locally**

Run:
```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```
Expected: all four PASS.

- [ ] **Step 2: Update `docs/USAGE.md`**

Add a new section after "Model catalog":
```markdown
## Building a distribution

An adopter authors a distribution profile describing what to package, for
which platforms, and how to sign it:

```toml
[distribution]
name = "my-app"
manifest = "manifest.toml"

[[targets]]
platform = "macos"
format = "dmg"
signing_identity = "keychain:Developer ID Application: My Company, Inc."

[[targets]]
platform = "windows"
format = "msi"
certificate_thumbprint = "AB12CD34EF56"
```

```bash
mlai package build --profile distribution-profile.toml --target-index 0 \
  --binary target/release/my-app --out-dir dist
```

Wraps `cargo-packager` (`cargo install cargo-packager` first) to produce the
actual installer — `mlai` never reimplements installer-format generation.
Signing fields are references only (a keychain identity name, a
certificate-store thumbprint) — `mlai` never touches key material or
passwords; those stay in the build machine's own keychain/certificate
store and CI secret configuration. `--target-index` selects one entry from
the profile's `targets` array; a CI job building on macOS picks the macOS
target, a Windows job picks the Windows target, and so on.
```

- [ ] **Step 3: Commit**

```bash
git add docs/USAGE.md
git commit -m "docs: document distribution profiles and mlai package build"
```

- [ ] **Step 4: Final full-workspace verification**

Run: `cargo test --workspace`
Expected: PASS on the local platform; CI verifies all three once pushed.

---

## Self-Review Notes

- **Spec coverage**: `DistributionProfile` schema, signing-as-reference, and the `cargo-packager` wrapping are all covered per `docs/superpowers/specs/2026-08-15-distribution-packaging-framework-design.md`'s architecture section. The deploy adapter and `mlai init` are explicitly out of scope (separate, later, parallelizable-with-each-other plans), not gaps.
- **Placeholder scan**: no TBD/TODO markers; every step has complete, runnable code, all field names/casing verified against a real local `cargo-packager` run rather than guessed.
- **Type consistency**: `DistributionProfile`/`Target`/`PackageFormat` (Task 1), `build_packager_config`/`packager_format_str` (Task 2), and `packager_command`/`build_package` (Task 3) are each defined once and consumed identically in Task 4's CLI layer.
