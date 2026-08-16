# mlai init Guided Wizard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `mlai init` — a guided CLI wizard for the *adopter* (a TA or CinePipe engineer configuring a distribution, not an end customer) that prompts through the choices a `DistributionProfile` needs and writes the file, with no deep packaging/Rust/TOML knowledge required.

**Prerequisite:** `docs/superpowers/plans/2026-08-16-mlai-package-foundation.md` must already be merged — this plan writes `mlai_package::profile::DistributionProfile` values and needs those types to exist. Do not start this plan if `crates/mlai-package/src/profile.rs` doesn't exist yet. This plan has no dependency on `docs/superpowers/plans/2026-08-16-github-releases-deploy-adapter.md` and can be built in parallel with it — both depend only on `mlai-package-foundation`, not on each other.

**Architecture:** A plain sequential stdin/stdout prompt flow (no TUI library — matches this project's existing `mlai uninstall` confirmation-prompt precedent, which is also plain `read_line`), scoped to configuring **one target platform per run** (v1 — a profile with multiple targets is built by running `mlai init` once per platform and hand-merging, or editing the TOML directly; a true multi-target-in-one-run flow is a real UX improvement left for later rather than adding untested prompt-branching complexity now). Writes a complete, valid `DistributionProfile` TOML file.

**Tech Stack:** No new dependencies (`mlai-package`, `mlai-core`, `toml` already present).

## Global Constraints

- The wizard is for the *adopter*, not an end customer — this is a developer tool, consistent with `docs/superpowers/specs/2026-08-15-distribution-packaging-framework-design.md`'s scope note ("not an end-customer-facing GUI installer").
- Every prompt has a sensible default where one exists (format per platform, deploy adapter, output path) so pressing Enter through the whole flow produces a working, if minimal, profile.
- Signing prompts accept a blank answer (skip) — the wizard never requires a signing identity to produce a valid profile, matching `Target.signing_identity`/`certificate_thumbprint` being `Option`.
- Before every commit: `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` all pass on all three CI platforms.

## Out of scope for this plan

- Multiple targets in one wizard run (see Architecture above).
- Reading/validating that the referenced `manifest.toml` or its named `components` actually exist — the wizard writes what the user typed; `mlai package build` will surface a real error later if the profile references something invalid, matching this project's general "validate at the point of use" posture rather than duplicating validation in two places.
- Editing an existing profile in place — each run writes a fresh file (overwriting if the output path already exists, since that's what "init" conventionally means; no merge logic).

---

### Task 1: Prompt flow + profile construction

**Files:**
- Create: `crates/mlai-cli/src/commands/init.rs`
- Modify: `crates/mlai-cli/src/commands/mod.rs`
- Modify: `crates/mlai-cli/src/main.rs`
- Create: `crates/mlai-cli/tests/init.rs`

**Interfaces:**
- Consumes: `mlai_package::profile::{DistributionProfile, Distribution, Target, Platform, PackageFormat, DeployConfig}` (from `mlai-package-foundation`).
- Produces: `mlai init [--output <path>]` — reads prompts from stdin, writes a `DistributionProfile` TOML file. Internal: `run_wizard(reader: &mut impl BufRead, writer: &mut impl Write) -> Result<DistributionProfile, InitError>` (the testable core, decoupled from real stdin/stdout).

- [ ] **Step 1: Write the failing integration test**

`crates/mlai-cli/tests/init.rs`:
```rust
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

    cmd.assert().success().stdout(contains("Wrote distribution profile"));

    let written = fs::read_to_string(&output_path).unwrap();
    assert!(written.contains("name = \"my-app\""));
    assert!(written.contains("\"comp-a\""));
    assert!(written.contains("\"comp-b\""));
    assert!(written.contains("platform = \"macos\""));
    assert!(written.contains("format = \"dmg\""));
    assert!(written.contains("signing_identity = \"keychain:Developer ID Application: Example, Inc.\""));
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
    assert_eq!(profile.targets[0].format, mlai_package::profile::PackageFormat::Deb); // default for linux
    assert!(profile.targets[0].signing_identity.is_none());
    assert!(profile.deploy.is_none()); // declined with "n"
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --workspace`
Expected: FAIL — `mlai` has no `init` subcommand yet, and `crates/mlai-cli/tests/init.rs` won't compile against a nonexistent CLI surface (the binary itself still builds; `assert_cmd` will just fail the test at runtime with a "no such subcommand" error initially — but add the CLI wiring in Step 4 before expecting a real pass, per the plan's Step order).

- [ ] **Step 3: Write `commands/init.rs`**

`crates/mlai-cli/src/commands/init.rs`:
```rust
use anyhow::{Context, Result};
use mlai_package::profile::{DeployConfig, Distribution, DistributionProfile, PackageFormat, Platform, Target};
use std::io::{BufRead, Write};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("unrecognized platform '{0}' — expected one of: macos, windows, linux")]
    UnknownPlatform(String),
    #[error("unrecognized package format '{0}'")]
    UnknownFormat(String),
    #[error("failed to read input: {0}")]
    Io(#[from] std::io::Error),
}

fn prompt(writer: &mut impl Write, text: &str) -> std::io::Result<()> {
    write!(writer, "{text}")?;
    writer.flush()
}

fn read_answer(reader: &mut impl BufRead) -> Result<String, InitError> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn parse_platform(s: &str) -> Result<Platform, InitError> {
    match s.to_lowercase().as_str() {
        "macos" => Ok(Platform::Macos),
        "windows" => Ok(Platform::Windows),
        "linux" => Ok(Platform::Linux),
        other => Err(InitError::UnknownPlatform(other.to_string())),
    }
}

fn default_format_for(platform: Platform) -> PackageFormat {
    match platform {
        Platform::Macos => PackageFormat::Dmg,
        Platform::Windows => PackageFormat::Msi,
        Platform::Linux => PackageFormat::Deb,
    }
}

fn parse_format(s: &str) -> Result<PackageFormat, InitError> {
    match s.to_lowercase().as_str() {
        "dmg" => Ok(PackageFormat::Dmg),
        "app" => Ok(PackageFormat::App),
        "msi" => Ok(PackageFormat::Msi),
        "nsis" => Ok(PackageFormat::Nsis),
        "deb" => Ok(PackageFormat::Deb),
        "appimage" => Ok(PackageFormat::Appimage),
        other => Err(InitError::UnknownFormat(other.to_string())),
    }
}

/// The wizard's testable core: reads answers from `reader`, writes prompts
/// to `writer`, returns the constructed profile. Decoupled from real
/// stdin/stdout so tests can drive it with an in-memory buffer.
pub fn run_wizard(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> Result<DistributionProfile, InitError> {
    prompt(writer, "Distribution name: ")?;
    let name = read_answer(reader)?;

    prompt(writer, "Manifest path [manifest.toml]: ")?;
    let manifest_answer = read_answer(reader)?;
    let manifest = if manifest_answer.is_empty() { "manifest.toml".to_string() } else { manifest_answer };

    prompt(writer, "Components (comma-separated, blank = all from manifest): ")?;
    let components_answer = read_answer(reader)?;
    let components: Vec<String> = if components_answer.is_empty() {
        vec![]
    } else {
        components_answer.split(',').map(|s| s.trim().to_string()).collect()
    };

    prompt(writer, "Target platform (macos/windows/linux): ")?;
    let platform = parse_platform(&read_answer(reader)?)?;
    let default_format = default_format_for(platform);

    prompt(writer, &format!("Package format [{default_format:?}]: "))?;
    let format_answer = read_answer(reader)?;
    let format = if format_answer.is_empty() { default_format } else { parse_format(&format_answer)? };

    prompt(writer, "Signing identity (macOS keychain name, blank = none): ")?;
    let signing_identity_answer = read_answer(reader)?;
    let signing_identity = if signing_identity_answer.is_empty() { None } else { Some(signing_identity_answer) };

    prompt(writer, "Certificate thumbprint (Windows, blank = none): ")?;
    let thumbprint_answer = read_answer(reader)?;
    let certificate_thumbprint = if thumbprint_answer.is_empty() { None } else { Some(thumbprint_answer) };

    prompt(writer, "Configure a deploy target? [y/N]: ")?;
    let wants_deploy = read_answer(reader)?.to_lowercase().starts_with('y');
    let deploy = if wants_deploy {
        prompt(writer, "GitHub repo (owner/name): ")?;
        let repo = read_answer(reader)?;
        Some(DeployConfig { adapter: "github-releases".to_string(), repo: Some(repo) })
    } else {
        None
    };

    Ok(DistributionProfile {
        distribution: Distribution { name, manifest, components },
        targets: vec![Target { platform, format, signing_identity, certificate_thumbprint, notarize: false }],
        deploy,
    })
}

pub fn run(output: &Path) -> Result<()> {
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    let profile = run_wizard(&mut reader, &mut writer).context("running the init wizard")?;
    let toml_str = toml::to_string_pretty(&profile).context("serializing the distribution profile")?;
    std::fs::write(output, toml_str)
        .with_context(|| format!("writing distribution profile to {}", output.display()))?;
    println!("Wrote distribution profile to {}", output.display());
    Ok(())
}
```

Modify `crates/mlai-cli/src/commands/mod.rs`:
```rust
pub mod catalog;
pub mod init;
pub mod install;
pub mod package;
pub mod repair;
pub mod uninstall;
```

- [ ] **Step 4: Wire the CLI subcommand**

In `crates/mlai-cli/src/main.rs`, add to the `Commands` enum:
```rust
    /// Guided wizard that writes a distribution profile for mlai package build
    Init {
        #[arg(long, default_value = "distribution-profile.toml")]
        output: PathBuf,
    },
```
Update the `match cli.command` block:
```rust
        Commands::Init { output } => commands::init::run(&output),
```

Modify `crates/mlai-cli/Cargo.toml` — add to `[dependencies]` (if not already present from the `mlai-package-foundation` plan's Task 4, which this plan assumes is already merged):
```toml
mlai-package = { path = "../mlai-package" }
```
Add to `[dependencies]` (this plan's own addition, not present before):
```toml
toml = "0.8"
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS — both `mlai-cli/tests/init.rs` tests.

- [ ] **Step 6: Commit**

```bash
git add crates/mlai-cli
git commit -m "feat(mlai-cli): add init guided wizard"
```

---

### Task 2: Docs + final verification

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

Add after "Publishing a distribution":
```markdown
## Guided setup (`mlai init`)

For an adopter who doesn't want to hand-write a distribution profile:

```bash
mlai init --output distribution-profile.toml
```

Prompts through distribution name, manifest path, which components to
include, one target platform (format defaults per platform: `dmg` on
macOS, `msi` on Windows, `deb` on Linux), optional signing identity
references, and an optional GitHub Releases deploy target — then writes
the file `mlai package build`/`mlai package deploy` read. Every prompt has
a sensible default; pressing Enter through the whole flow produces a
minimal but valid profile. Configures one platform per run — for more than
one, run it again with a different platform answer and merge the
`[[targets]]` entries by hand, or edit the TOML directly.
```

- [ ] **Step 3: Commit**

```bash
git add docs/USAGE.md
git commit -m "docs: document mlai init"
```

- [ ] **Step 4: Final full-workspace verification**

Run: `cargo test --workspace`
Expected: PASS on the local platform; CI verifies all three once pushed.

---

## Self-Review Notes

- **Spec coverage**: the guided adopter-facing wizard is covered per `docs/superpowers/specs/2026-08-15-distribution-packaging-framework-design.md`'s architecture piece 3. Multi-target-per-run and profile-editing are explicitly out of scope (real UX improvements, not gaps — see "Out of scope" above).
- **Placeholder scan**: no TBD/TODO markers; every step has complete, runnable code, including two full integration tests exercising the entire prompt sequence (full-answers and all-blank-defaults paths).
- **Type consistency**: `run_wizard`'s return type (`DistributionProfile`) and every field it populates match `mlai-package-foundation`'s exact types (`Distribution`, `Target`, `Platform`, `PackageFormat`, `DeployConfig`) with no local redefinition.
