# Project Binding (`bind-project`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port a prior-art installer's `BindsToProjectType`/`add_project` mechanism into MLAppInstaller — a manifest field, a pipeline function, a CLI subcommand, and a GUI panel — so an installed component can be bound to a real target-project file (e.g. a UE5 `.uproject`) after install.

**Architecture:** Add `binds_to_project_type: Option<String>` to `Component` in `mlai-core::manifest`. Add `bind_project()` to `mlai-core::pipeline`, matching the exact per-component `PipelineOptions` construction pattern already used by `commands::install::run`/`commands::repair::run` (not a shared pre-built `PipelineOptions`, since `version` must be each component's own `component_ref`). `bind_project` filters `manifest.components` by `binds_to_project_type` and by already-installed state (checked via `InstalledState::load`), substitutes a `{project}` placeholder into the matched components' setup args, and force-reinstalls each via the existing `install_component`. Wire a `mlai bind-project` CLI subcommand, then a Tauri command + GUI panel in `mlai-gui` that calls the same pipeline function.

**Tech Stack:** Rust (existing `mlai-core`/`mlai-cli`/`mlai-gui` crates), `tauri-plugin-dialog` (new dependency for the native file picker), existing `clap`/`serde`/`toml`/`anyhow`/`assert_cmd`/`predicates`/`mockito`/`zip` stack (all already present in `mlai-cli`'s `Cargo.toml`).

## Global Constraints

- Every step's exit criteria: `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` must all pass before commit (per `CLAUDE.md`).
- A component with no `binds_to_project_type` must be completely unaffected — zero behavior change for every existing manifest.
- Error messages must state what happened, what was attempted, and what to do next (Observability Mandate) — never a bare failure.
- Commit in logical working units; update `docs/USAGE.md` for the new CLI command and GUI panel per the Documentation Updates policy.
- Design reference: `docs/superpowers/specs/2026-08-18-bind-project-design.md`.
- Grounding note: every function/struct/field name below was verified directly against the current contents of `crates/mlai-core/src/{manifest,pipeline,state}.rs`, `crates/mlai-cli/src/commands/{install,repair}.rs`, `crates/mlai-cli/Cargo.toml`, `crates/mlai-gui/src-tauri/src/lib.rs`, `crates/mlai-gui/src/main.ts`, and `crates/mlai-gui/src-tauri/Cargo.toml` on 2026-08-18 — not assumed from memory.

---

### Task 1: Manifest field + pipeline `bind_project`

**Files:**
- Modify: `crates/mlai-core/src/manifest.rs`
- Modify: `crates/mlai-core/src/pipeline.rs`
- Test: inline `#[cfg(test)]` modules in both files above

**Interfaces:**
- Consumes: `mlai_core::manifest::{Manifest, Component, PlatformSetup, SetupCommand}` (existing — `Component` has public fields `name: String`, `source_url: String`, `component_ref: String` (serde-renamed from `ref`), `default: bool`, `setup: PlatformSetup`, `health: PlatformHealth`, `supports_options_protocol: PlatformFlag`; `PlatformSetup` has public fields `windows: Option<SetupCommand>`, `posix: Option<SetupCommand>`; `Manifest.components: Vec<Component>`), `mlai_core::pipeline::{PipelineOptions, install_component, PipelineError}` (existing — `install_component(component: &Component, manifest: &Manifest, opts: &PipelineOptions) -> Result<ComponentState, PipelineError>`; `PipelineOptions<'a> { install_root: PathBuf, fetcher: &'a dyn Fetcher, version: String, backup_keep: usize, set_options: Vec<(String, String)>, force: bool }`, no `Clone` derive), `mlai_core::state::InstalledState` (existing — `InstalledState::load(install_root: &Path) -> Result<InstalledState, StateError>`, `.components: BTreeMap<String, ComponentRecord>`), `mlai_core::fetch::Fetcher` trait (existing).
- Produces: `Component.binds_to_project_type: Option<String>` (new field, consumed by Task 2's CLI and Task 3's GUI); `mlai_core::pipeline::bind_project(manifest: &Manifest, install_root: &Path, fetcher: &dyn Fetcher, project_type: &str, project_path: &Path) -> Vec<(String, Result<ComponentState, PipelineError>)>` (new function, consumed by Task 2's CLI and Task 3's GUI — the `String` in each tuple is the component name; `install_root`/`fetcher` are separate params, not a caller-built `PipelineOptions`, because each matched component needs its own `version: component.component_ref.clone()`, matching `install.rs`/`repair.rs`'s existing per-component construction).

- [ ] **Step 1: Write the failing test for the manifest field**

Add to `crates/mlai-core/src/manifest.rs`, inside the existing `#[cfg(test)] mod tests` block:

```rust
#[test]
fn component_parses_binds_to_project_type_when_present() {
    let toml = r#"
        manifest_version = "1.0.0"

        [[components]]
        name = "ue5-cine-pipeline"
        source_url = "https://example.com/ue5-cine-pipeline.zip"
        ref = "main"
        binds_to_project_type = "UE5"

        [components.setup.posix]
        command = "./install.sh"
        args = ["-Project", "{project}"]
    "#;
    let manifest = Manifest::parse(toml).unwrap();
    assert_eq!(
        manifest.components[0].binds_to_project_type.as_deref(),
        Some("UE5")
    );
}

#[test]
fn component_binds_to_project_type_defaults_to_none_when_absent() {
    let manifest = Manifest::parse(SAMPLE).unwrap();
    assert_eq!(manifest.components[0].binds_to_project_type, None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mlai-core component_parses_binds_to_project_type_when_present`
Expected: FAIL with "no field `binds_to_project_type` on type `Component`" (compile error).

- [ ] **Step 3: Add the field to `Component`**

In `crates/mlai-core/src/manifest.rs`, add to the `Component` struct (after `supports_options_protocol`):

```rust
    #[serde(default)]
    pub binds_to_project_type: Option<String>,
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p mlai-core component_parses_binds_to_project_type_when_present component_binds_to_project_type_defaults_to_none_when_absent`
Expected: PASS (2 passed).

- [ ] **Step 5: Commit**

```bash
git add crates/mlai-core/src/manifest.rs
git commit -m "feat(mlai-core): add binds_to_project_type field to Component"
```

- [ ] **Step 6: Run full mlai-core suite to confirm no other test broke**

Run: `cargo test -p mlai-core`
Expected: all PASS — every other test that constructs a `Component` literal (e.g. `crates/mlai-gui/src-tauri/src/lib.rs`'s `options_for_a_component_are_none_when_the_component_declares_no_support`, and `crates/mlai-core/src/pipeline.rs`'s `sample_component()`) must be updated to add `binds_to_project_type: None,` to their struct literals, since `Component` has no `#[derive(Default)]` and every field must be named. Find every such literal with `grep -rn "supports_options_protocol: PlatformFlag::default()," crates/ --include=*.rs` and `grep -rn "supports_options_protocol:" crates/mlai-core/src/pipeline.rs` (the two known call sites: `crates/mlai-gui/src-tauri/src/lib.rs`'s test module and `crates/mlai-core/src/pipeline.rs`'s `sample_component()`) and add `binds_to_project_type: None,` alongside each. Re-run `cargo test --workspace` after fixing, expect all PASS.

- [ ] **Step 7: Commit the fixture fix**

```bash
git add crates/mlai-core/src/pipeline.rs crates/mlai-gui/src-tauri/src/lib.rs
git commit -m "fix: add binds_to_project_type to existing Component test fixtures"
```

- [ ] **Step 8: Write the failing test for `bind_project` — no matches**

Add to `crates/mlai-core/src/pipeline.rs`'s existing `#[cfg(all(test, unix))] mod tests` block, reusing its existing `sample_component()` helper and `FixtureFetcher`:

```rust
#[test]
fn bind_project_ignores_untagged_and_uninstalled_components() {
    let root = tempdir().unwrap();
    let fixture_dir = tempdir().unwrap();
    let zip_path = fixture_dir.path().join("bundle.zip");
    build_fixture_zip(&zip_path);
    let fetcher = FixtureFetcher { zip_path };

    let mut untagged = sample_component();
    untagged.name = "untagged".into();

    let mut tagged_not_installed = sample_component();
    tagged_not_installed.name = "tagged-not-installed".into();
    tagged_not_installed.binds_to_project_type = Some("UE5".into());

    let manifest = Manifest {
        manifest_version: "1.0.0".into(),
        components: vec![untagged.clone(), tagged_not_installed],
        removals: vec![],
    };

    // Install only the untagged component, so it's recorded in installed.json
    // -- tagged_not_installed is declared in the manifest but never installed.
    let opts = PipelineOptions {
        install_root: root.path().to_path_buf(),
        fetcher: &fetcher,
        version: "abc123".into(),
        backup_keep: 3,
        set_options: vec![],
        force: false,
    };
    install_component(&untagged, &manifest, &opts).unwrap();

    let results = bind_project(
        &manifest,
        root.path(),
        &fetcher,
        "UE5",
        Path::new("/fake/MyGame.uproject"),
    );

    assert!(
        results.is_empty(),
        "untagged component and a tagged-but-uninstalled component must both be skipped"
    );
}
```

- [ ] **Step 9: Run test to verify it fails**

Run: `cargo test -p mlai-core bind_project_ignores_untagged_and_uninstalled_components`
Expected: FAIL with "cannot find function `bind_project`".

- [ ] **Step 10: Implement `bind_project`**

Add to `crates/mlai-core/src/pipeline.rs`, after `install_component`:

```rust
/// Finds every already-installed component matching `project_type`,
/// substitutes `project_path` for a `{project}` placeholder in its setup
/// command args, and force-reinstalls it -- the same semantics as
/// the same semantics as the source installer's original `add_project`: untagged components and
/// tagged-but-not-yet-installed components are left completely untouched.
pub fn bind_project(
    manifest: &crate::manifest::Manifest,
    install_root: &Path,
    fetcher: &dyn crate::fetch::Fetcher,
    project_type: &str,
    project_path: &Path,
) -> Vec<(String, Result<ComponentState, PipelineError>)> {
    let installed = InstalledState::load(install_root).unwrap_or_default();
    let project_str = project_path.to_string_lossy().to_string();

    manifest
        .components
        .iter()
        .filter(|c| c.binds_to_project_type.as_deref() == Some(project_type))
        .filter(|c| installed.components.contains_key(&c.name))
        .map(|component| {
            let mut bound = component.clone();
            let setup = if cfg!(target_os = "windows") {
                bound.setup.windows.as_mut()
            } else {
                bound.setup.posix.as_mut()
            };
            if let Some(setup) = setup {
                for arg in &mut setup.args {
                    if arg == "{project}" {
                        *arg = project_str.clone();
                    }
                }
            }
            let opts = PipelineOptions {
                install_root: install_root.to_path_buf(),
                fetcher,
                version: component.component_ref.clone(),
                backup_keep: 3,
                set_options: Vec::new(),
                force: true,
            };
            let result = install_component(&bound, manifest, &opts);
            (component.name.clone(), result)
        })
        .collect()
}
```

- [ ] **Step 11: Run tests to verify they pass**

Run: `cargo test -p mlai-core bind_project`
Expected: PASS (Step 8's test).

- [ ] **Step 12: Commit**

```bash
git add crates/mlai-core/src/pipeline.rs
git commit -m "feat(mlai-core): add bind_project pipeline function (no-match case)"
```

- [ ] **Step 13: Write the failing test for `bind_project` — real substitution and force-reinstall**

Add to the same test module, using a new fixture-zip builder that records its argv (mirroring the existing `build_fixture_zip_recording_args`-style pattern used elsewhere in this crate for observing "what args did setup actually receive"):

```rust
fn build_fixture_zip_with_project_placeholder(path: &Path) {
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    zip.add_directory("ue5-cine-pipeline-main/", options).unwrap();
    zip.start_file("ue5-cine-pipeline-main/setup.sh", options)
        .unwrap();
    zip.write_all(b"#!/bin/sh\necho \"$@\" > argv.txt\ntouch marker.txt\n")
        .unwrap();
    zip.finish().unwrap();
}

#[test]
fn bind_project_substitutes_the_real_path_and_force_reinstalls_a_matching_installed_component() {
    let root = tempdir().unwrap();
    let fixture_dir = tempdir().unwrap();
    let zip_path = fixture_dir.path().join("bundle.zip");
    build_fixture_zip_with_project_placeholder(&zip_path);
    let fetcher = FixtureFetcher { zip_path };

    let ue5 = Component {
        name: "ue5-cine-pipeline".into(),
        source_url: "https://example.com/ue5-cine-pipeline.zip".into(),
        component_ref: "main".into(),
        default: true,
        setup: PlatformSetup {
            windows: None,
            posix: Some(SetupCommand {
                command: "sh".into(),
                args: vec!["setup.sh".into(), "-Project".into(), "{project}".into()],
            }),
        },
        health: PlatformHealth {
            windows: None,
            posix: Some(HealthCheck::FileExists {
                path: "marker.txt".into(),
            }),
        },
        supports_options_protocol: PlatformFlag::default(),
        binds_to_project_type: Some("UE5".into()),
    };
    let manifest = Manifest {
        manifest_version: "1.0.0".into(),
        components: vec![ue5.clone()],
        removals: vec![],
    };

    let opts = PipelineOptions {
        install_root: root.path().to_path_buf(),
        fetcher: &fetcher,
        version: "main".into(),
        backup_keep: 3,
        set_options: vec![],
        force: false,
    };
    install_component(&ue5, &manifest, &opts).unwrap();

    let results = bind_project(
        &manifest,
        root.path(),
        &fetcher,
        "UE5",
        Path::new("/fake/MyGame.uproject"),
    );

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "ue5-cine-pipeline");
    assert_eq!(results[0].1.as_ref().unwrap(), &ComponentState::Healthy);

    let argv = fs::read_to_string(root.path().join("ue5-cine-pipeline").join("argv.txt")).unwrap();
    assert!(
        argv.contains("/fake/MyGame.uproject"),
        "the {{project}} placeholder must be substituted with the real path: {argv}"
    );
    assert!(
        !argv.contains("{project}"),
        "the literal placeholder must not survive into the real setup invocation: {argv}"
    );
}
```

- [ ] **Step 14: Run test to verify it fails, then fix implementation until it passes**

Run: `cargo test -p mlai-core bind_project_substitutes_the_real_path_and_force_reinstalls_a_matching_installed_component`
Expected: initial run should PASS given Step 10's implementation already handles substitution and forcing — if it fails, fix `bind_project` (not the test) to match this assertion; the test is the contract for exact substitution/force-reinstall behavior.

- [ ] **Step 15: Run full mlai-core suite, lints, and commit**

Run: `cargo test -p mlai-core && cargo fmt --all -- --check && cargo clippy -p mlai-core --all-targets -- -D warnings`
Expected: all clean.

```bash
git add crates/mlai-core/src/pipeline.rs
git commit -m "feat(mlai-core): bind_project substitutes {project} and force-reinstalls matches"
```

---

### Task 2: CLI subcommand `mlai bind-project`

**Files:**
- Modify: `crates/mlai-cli/src/main.rs`
- Create: `crates/mlai-cli/src/commands/bind_project.rs`
- Modify: `crates/mlai-cli/src/commands/mod.rs`
- Create: `crates/mlai-cli/tests/bind_project.rs`

**Interfaces:**
- Consumes: `mlai_core::pipeline::bind_project` (Task 1), `mlai_core::fetch::HttpFetcher` (existing — `HttpFetcher { token: Option<String> }`, already used identically by `commands::install::run`/`commands::repair::run`), `mlai_core::manifest::Manifest::parse` (existing).
- Produces: `commands::bind_project::run(manifest_path: &Path, install_root: &Path, project_type: &str, project_path: &Path) -> anyhow::Result<()>` (production entry point, consumed by `main.rs`'s match arm).

- [ ] **Step 1: Write the failing integration test — no match**

Create `crates/mlai-cli/tests/bind_project.rs`, following the exact structure of `crates/mlai-cli/tests/repair.rs`:

```rust
use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
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
name = "ue5-cine-pipeline"
source_url = "https://example.com/ue5-cine-pipeline.zip"
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mlai-cli bind_project_fails_clearly_when_no_installed_component_matches_the_type`
Expected: FAIL (no `bind-project` subcommand recognized — clap reports an unrecognized subcommand, non-zero exit, so `.failure()` passes but `.stderr(contains(...))` fails on the wrong message; either way the test fails as expected before implementation exists).

- [ ] **Step 3: Implement `commands::bind_project::run`**

Create `crates/mlai-cli/src/commands/bind_project.rs`:

```rust
use anyhow::{bail, Context, Result};
use mlai_core::fetch::HttpFetcher;
use mlai_core::manifest::Manifest;
use mlai_core::pipeline::bind_project as core_bind_project;
use mlai_core::state::ComponentState;
use std::fs;
use std::path::Path;

pub fn run(
    manifest_path: &Path,
    install_root: &Path,
    project_type: &str,
    project_path: &Path,
) -> Result<()> {
    let manifest_str = fs::read_to_string(manifest_path)
        .with_context(|| format!("reading manifest at {}", manifest_path.display()))?;
    let manifest = Manifest::parse(&manifest_str)
        .with_context(|| format!("parsing manifest at {}", manifest_path.display()))?;

    let fetcher = HttpFetcher {
        token: std::env::var("MLAI_TOKEN").ok(),
    };

    let results = core_bind_project(&manifest, install_root, &fetcher, project_type, project_path);

    if results.is_empty() {
        bail!(
            "no installed component declares binds_to_project_type = \"{project_type}\" in {} \
             -- check the manifest's [[components]] entries and confirm the matching component \
             is already installed before binding a project to it",
            manifest_path.display()
        );
    }

    let mut any_failed = false;
    for (name, result) in results {
        match result {
            Ok(ComponentState::Healthy) => println!("  {name} -> bound"),
            Ok(other) => {
                println!("  {name} -> {other:?} (NEEDS ATTENTION)");
                any_failed = true;
            }
            Err(e) => {
                println!("  {name} -> failed: {e}");
                any_failed = true;
            }
        }
    }

    if any_failed {
        bail!("one or more components failed to bind to the project -- see output above");
    }

    Ok(())
}
```

- [ ] **Step 4: Register the module**

In `crates/mlai-cli/src/commands/mod.rs`, add in alphabetical order:

```rust
pub mod bind_project;
```

- [ ] **Step 5: Wire the CLI subcommand**

In `crates/mlai-cli/src/main.rs`, add to the `Commands` enum, after the `Package` variant and before `Init`:

```rust
    /// Bind a real project file to every installed component matching a project type
    BindProject {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        install_root: PathBuf,
        #[arg(long = "type")]
        project_type: String,
        #[arg(long)]
        path: PathBuf,
    },
```

And in `main()`'s match, after the `Commands::Package { action } => match action { ... }` arm and before `Commands::Init`:

```rust
        Commands::BindProject {
            manifest,
            install_root,
            project_type,
            path,
        } => commands::bind_project::run(&manifest, &install_root, &project_type, &path),
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test -p mlai-cli bind_project_fails_clearly_when_no_installed_component_matches_the_type`
Expected: PASS.

- [ ] **Step 7: Write and pass the success-path integration test**

Add to `crates/mlai-cli/tests/bind_project.rs`, following `tests/repair.rs`'s exact mockito + zip-fixture pattern:

```rust
#[cfg(unix)]
fn build_fixture_zip_with_project_placeholder(path: &std::path::Path) {
    use std::io::Write;
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    zip.add_directory("ue5-cine-pipeline-main/", options).unwrap();
    zip.start_file("ue5-cine-pipeline-main/setup.sh", options)
        .unwrap();
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
        .mock("GET", "/ue5-cine-pipeline.zip")
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
name = "ue5-cine-pipeline"
source_url = "{}/ue5-cine-pipeline.zip"
ref = "main"
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
        .stdout(contains("ue5-cine-pipeline -> bound"));

    let argv = fs::read_to_string(
        install_root
            .path()
            .join("ue5-cine-pipeline")
            .join("argv.txt"),
    )
    .unwrap();
    assert!(argv.contains("/fake/MyGame.uproject"));
}
```

Note the `{{project}}` (doubled braces) in the TOML `format!` string above — that's the literal `{project}` placeholder surviving Rust's `format!` brace-escaping, not a mistake to "fix."

- [ ] **Step 8: Run test to verify it fails, then fix until it passes**

Run: `cargo test -p mlai-cli bind_project_substitutes_the_real_path_for_an_installed_matching_component`
Expected: passes against Task 1's already-implemented `bind_project`; if it fails, the bug is in this test's fixture wiring (compare line-by-line against `tests/repair.rs`'s working equivalent) rather than in Task 1's core logic, which Task 1 Step 14 already proved correct in isolation.

- [ ] **Step 9: Run full mlai-cli suite, lints, and commit**

Run: `cargo test -p mlai-cli && cargo fmt --all -- --check && cargo clippy -p mlai-cli --all-targets -- -D warnings`
Expected: all clean.

```bash
git add crates/mlai-cli
git commit -m "feat(mlai-cli): add bind-project subcommand"
```

- [ ] **Step 10: Update `docs/USAGE.md`**

Add a "Binding a project (`mlai bind-project`)" section documenting the command (flags: `--manifest`, `--install-root`, `--type`, `--path`), an example invocation, and the manifest's `binds_to_project_type` field with a short TOML example. Match this doc's existing section heading/prose style (check the "Guided setup (`mlai init`)" section for the pattern to mirror).

- [ ] **Step 11: Commit docs**

```bash
git add docs/USAGE.md
git commit -m "docs: document mlai bind-project"
```

---

### Task 3: GUI panel in `mlai-gui`

**Files:**
- Modify: `crates/mlai-gui/src-tauri/Cargo.toml`
- Modify: `crates/mlai-gui/src-tauri/src/lib.rs`
- Modify: `crates/mlai-gui/src/main.ts`
- Modify: `crates/mlai-gui/index.html`
- Modify: `crates/mlai-gui/package.json` (via `npm install`, Step 9)

**Interfaces:**
- Consumes: `mlai_core::pipeline::bind_project` (Task 1); `mlai-gui`'s existing `read_manifest_at(path: &Path) -> Result<Manifest, String>`, `find_resource(app: &AppHandle, relative: &str) -> Option<PathBuf>`, `ComponentResult { name: String, outcome: String, message: Option<String> }` (all already defined in `crates/mlai-gui/src-tauri/src/lib.rs` — reuse exactly, do not redefine); `mlai_core::fetch::HttpFetcher`, `mlai_core::paths::default_install_root()` (existing, already used by `read_install_status`).
- Produces: a new Tauri command `bind_project(app: AppHandle, install_root: Option<String>, project_type: String, project_path: String) -> Result<Vec<ComponentResult>, String>` (registered in `tauri::generate_handler![...]` alongside the existing 5 commands; invoked from `main.ts` as `invoke<ComponentResult[]>("bind_project", { installRoot, projectType, projectPath })`). No other task depends on this — it is the plan's final task.

- [ ] **Step 1: Add the dialog plugin dependency**

In `crates/mlai-gui/src-tauri/Cargo.toml`, add to `[dependencies]` (matching this file's existing `tauri = { version = "2", features = [] }` precision):

```toml
tauri-plugin-dialog = { version = "2", features = [] }
```

In `crates/mlai-gui/src-tauri/src/lib.rs`, in the `run()` function, change:

```rust
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
```

to:

```rust
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
```

- [ ] **Step 2: Run a build to verify the plugin compiles**

Run: `cargo build -p mlai-gui`
Expected: builds successfully with the new dependency (the package name is `mlai-gui`; its lib target is named `mlai_gui_lib`, but `cargo build -p`/`cargo test -p` use the package name, not the lib target name).

- [ ] **Step 3: Write the failing test for the Tauri command's core logic**

`bind_project` needs an `AppHandle` to call `find_resource`, which isn't constructible in a plain unit test without a running Tauri app. Follow this file's own established pattern for this exact problem: `describe_options_for` is a plain, `AppHandle`-free function that `describe_component_options` (the actual `#[tauri::command]`) calls after resolving the manifest path itself — the free function is what's unit-tested, not the command. Do the same here: extract the core logic into a free function that takes an already-parsed `Manifest` instead of an `AppHandle`.

Add to `crates/mlai-gui/src-tauri/src/lib.rs`'s existing `#[cfg(test)] mod tests` block:

```rust
#[test]
fn bind_project_for_returns_an_error_string_when_nothing_matches() {
    let dir = tempdir().unwrap();
    let manifest = Manifest {
        manifest_version: "1.0.0".into(),
        components: vec![Component {
            name: "hello-component".into(),
            source_url: "https://example.com/hello-component.zip".into(),
            component_ref: "main".into(),
            default: true,
            setup: PlatformSetup::default(),
            health: PlatformHealth::default(),
            supports_options_protocol: PlatformFlag::default(),
            binds_to_project_type: None,
        }],
        removals: vec![],
    };
    let result = bind_project_for(&manifest, dir.path(), "UE5", "/fake/MyGame.uproject");
    assert!(result.is_err());
}
```

Add the needed `use mlai_core::manifest::{Component, PlatformFlag, PlatformHealth, PlatformSetup};` and `use tempfile::tempdir;` to the test module's imports if not already present (the module already imports `Path`/`fs`/`tempdir` per the existing `options_for_a_component_are_none_when_the_component_declares_no_support` test — check before adding a duplicate import).

- [ ] **Step 4: Run test to verify it fails**

Run: `cargo test -p mlai-gui bind_project_for_returns_an_error_string_when_nothing_matches`
Expected: FAIL (function `bind_project_for` not found).

- [ ] **Step 5: Implement `bind_project_for` and the `bind_project` Tauri command**

Add to `crates/mlai-gui/src-tauri/src/lib.rs`, near `describe_options_for`/`describe_component_options` (same section, same pattern):

```rust
fn bind_project_for(
    manifest: &Manifest,
    install_root: &Path,
    project_type: &str,
    project_path: &str,
) -> Result<Vec<ComponentResult>, String> {
    let fetcher = HttpFetcher {
        token: std::env::var("MLAI_TOKEN").ok(),
    };
    let raw_results = mlai_core::pipeline::bind_project(
        manifest,
        install_root,
        &fetcher,
        project_type,
        Path::new(project_path),
    );
    if raw_results.is_empty() {
        return Err(format!(
            "no installed component declares binds_to_project_type = \"{project_type}\""
        ));
    }
    Ok(raw_results
        .into_iter()
        .map(|(name, result)| match result {
            Ok(mlai_core::state::ComponentState::Healthy) => ComponentResult {
                name,
                outcome: "healthy".to_string(),
                message: None,
            },
            Ok(other) => ComponentResult {
                name,
                outcome: format!("{other:?}"),
                message: None,
            },
            Err(e) => ComponentResult {
                name,
                outcome: "failed".to_string(),
                message: Some(e.to_string()),
            },
        })
        .collect())
}

#[tauri::command]
fn bind_project(
    app: AppHandle,
    install_root: Option<String>,
    project_type: String,
    project_path: String,
) -> Result<Vec<ComponentResult>, String> {
    let manifest_path = find_resource(&app, "manifest.toml")
        .ok_or_else(|| "manifest.toml not found".to_string())?;
    let manifest = read_manifest_at(&manifest_path)?;
    let root = install_root
        .filter(|r| !r.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(mlai_core::paths::default_install_root);
    bind_project_for(&manifest, &root, &project_type, &project_path)
}
```

Register the command: in `run()`'s `tauri::generate_handler![...]` list, add `bind_project` after `run_install`.

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test -p mlai-gui bind_project_for_returns_an_error_string_when_nothing_matches`
Expected: PASS.

- [ ] **Step 7: Run full mlai-gui suite, lints, and commit the Rust side**

Run: `cargo test -p mlai-gui && cargo fmt --all -- --check && cargo clippy -p mlai-gui --all-targets -- -D warnings`
Expected: all clean.

```bash
git add crates/mlai-gui/src-tauri
git commit -m "feat(mlai-gui): add bind_project Tauri command"
```

- [ ] **Step 8: Add the frontend panel markup**

In `crates/mlai-gui/index.html`, this file's existing collapsible sections use a `<div id="..." class="hidden">` + `<h2>` pattern (see the existing `#model-options-section`), not `<section>` — match it exactly. Add the new block immediately after the closing `</div>` of `#model-options-section` and before the `<button id="install-button">Install</button>` line:

```html
      <div id="add-project-section" class="hidden">
        <h2>Bind a Project</h2>
        <label for="project-type-select">Engine</label>
        <select id="project-type-select"></select>
        <button id="pick-project-file-button" type="button">Choose Project File...</button>
        <span id="selected-project-path"></span>
        <button id="bind-project-button" type="button" disabled>Bind Project</button>
        <div id="bind-project-result"></div>
      </div>
```

- [ ] **Step 9: Add `@tauri-apps/plugin-dialog` npm dependency**

Run: `cd crates/mlai-gui && npm install @tauri-apps/plugin-dialog`
Expected: adds the package to `package.json`/`package-lock.json`.

- [ ] **Step 10: Wire the frontend logic**

In `crates/mlai-gui/src/main.ts`:

1. Add the import at the top, alongside the existing `@tauri-apps/api/core` and `@tauri-apps/api/event` imports:

```typescript
import { open } from "@tauri-apps/plugin-dialog";
```

2. Extend the existing `Component` interface (it currently has `name`, `source_url`, `component_ref`, `default`) to add:

```typescript
  binds_to_project_type?: string | null;
```

3. Add a new `ComponentResult` interface (not currently present in this file — `InstallDone` exists but `ComponentResult` doesn't):

```typescript
interface ComponentResult {
  name: string;
  outcome: string;
  message: string | null;
}
```

4. Add a module-level variable to remember the last-loaded manifest, alongside the existing `let logView`/`let installButton`/`let statusEl` declarations:

```typescript
let currentManifest: Manifest | null = null;
let selectedProjectPath: string | null = null;
```

5. In `loadComponents()`, store the fetched manifest — change:

```typescript
    const manifest = await invoke<Manifest>("list_components");
    renderComponents(manifest);
```

to:

```typescript
    const manifest = await invoke<Manifest>("list_components");
    currentManifest = manifest;
    renderComponents(manifest);
```

6. Add the dropdown-population function:

```typescript
function populateProjectTypeDropdown(installedStatus: InstalledStatus) {
  const section = document.querySelector<HTMLElement>("#add-project-section");
  const select = document.querySelector<HTMLSelectElement>("#project-type-select");
  if (!section || !select || !currentManifest) return;

  const installedNames = new Set(Object.keys(installedStatus.components ?? {}));
  const types = new Set(
    currentManifest.components
      .filter((c) => installedNames.has(c.name) && c.binds_to_project_type)
      .map((c) => c.binds_to_project_type as string),
  );

  select.innerHTML = "";
  for (const t of types) {
    const option = document.createElement("option");
    option.value = t;
    option.textContent = t;
    select.appendChild(option);
  }
  section.classList.toggle("hidden", types.size === 0);
}
```

7. Call it from the existing `refreshInstallStatus()` — change:

```typescript
async function refreshInstallStatus() {
  const statusSpan = document.querySelector<HTMLElement>("#install-status");
  try {
    const status = await invoke<InstalledStatus>("read_install_status", {
      installRoot: currentInstallRoot() || null,
    });
    if (statusSpan) {
```

to:

```typescript
async function refreshInstallStatus() {
  const statusSpan = document.querySelector<HTMLElement>("#install-status");
  try {
    const status = await invoke<InstalledStatus>("read_install_status", {
      installRoot: currentInstallRoot() || null,
    });
    populateProjectTypeDropdown(status);
    if (statusSpan) {
```

(leave the rest of the function, including its `catch` block, unchanged).

8. Add the file-picker and bind handlers:

```typescript
async function pickProjectFile() {
  const path = await open({ multiple: false, directory: false });
  if (typeof path === "string") {
    selectedProjectPath = path;
    const span = document.querySelector<HTMLSpanElement>("#selected-project-path");
    if (span) span.textContent = path;
    const bindButton = document.querySelector<HTMLButtonElement>("#bind-project-button");
    if (bindButton) bindButton.disabled = false;
  }
}

async function bindProject() {
  if (!selectedProjectPath) return;
  const projectType =
    document.querySelector<HTMLSelectElement>("#project-type-select")?.value ?? "";
  const resultDiv = document.querySelector<HTMLDivElement>("#bind-project-result");
  if (!resultDiv) return;
  try {
    const results = await invoke<ComponentResult[]>("bind_project", {
      installRoot: currentInstallRoot() || null,
      projectType,
      projectPath: selectedProjectPath,
    });
    resultDiv.textContent = `Bound ${results.length} component(s) to ${projectType}.`;
  } catch (err) {
    resultDiv.textContent = `Failed to bind project: ${err}`;
  }
}
```

9. Register the new listeners inside the existing `window.addEventListener("DOMContentLoaded", () => { ... })` block, alongside the existing `installButton?.addEventListener(...)`/`modeSelect?.addEventListener(...)` calls:

```typescript
  document
    .querySelector<HTMLButtonElement>("#pick-project-file-button")
    ?.addEventListener("click", pickProjectFile);
  document
    .querySelector<HTMLButtonElement>("#bind-project-button")
    ?.addEventListener("click", bindProject);
```

10. Refresh the dropdown after a successful install too — change the existing `install-done` listener:

```typescript
  listen<InstallDone>("install-done", (event) => {
    if (statusEl) {
      statusEl.textContent = event.payload.message;
      statusEl.className = "status " + (event.payload.success ? "status-ok" : "status-fail");
    }
    if (installButton) installButton.disabled = false;
  });
```

to:

```typescript
  listen<InstallDone>("install-done", (event) => {
    if (statusEl) {
      statusEl.textContent = event.payload.message;
      statusEl.className = "status " + (event.payload.success ? "status-ok" : "status-fail");
    }
    if (installButton) installButton.disabled = false;
    if (event.payload.success) refreshInstallStatus();
  });
```

(a newly-installed component may now qualify for project binding — without this, the panel wouldn't appear until the user separately edits the install-root field, which is the only other trigger for `refreshInstallStatus()`).

- [ ] **Step 11: Manual verification (documented, not automated — GUI click-through)**

Run: `cd crates/mlai-gui && npm run tauri dev` (this file's `package.json` has a `"tauri": "tauri"` script, so this invokes the real Tauri CLI's dev command)
Manually: install a manifest fixture with a `binds_to_project_type`-tagged component, confirm the "Bind a Project" section appears only after that component installs, click "Choose Project File...", pick any file, click "Bind Project", confirm the result message appears and names the right component count. Record the outcome in this step's own commit message (e.g. "verified manually on macOS: dropdown populates after install, file picker opens, bind succeeds and reports 1 component(s)").

- [ ] **Step 12: Update `docs/USAGE.md`**

Add a short note under the existing GUI wizard section documenting the new "Bind a Project" panel: when it appears (only once an installed component declares `binds_to_project_type` in the manifest), what it does, and that it's the GUI equivalent of `mlai bind-project`.

- [ ] **Step 13: Commit the frontend + docs**

```bash
git add crates/mlai-gui/index.html crates/mlai-gui/src crates/mlai-gui/package.json crates/mlai-gui/package-lock.json docs/USAGE.md
git commit -m "feat(mlai-gui): add Bind a Project panel"
```

- [ ] **Step 14: Mark this plan's checkboxes complete**

Update this file (`docs/superpowers/plans/2026-08-18-bind-project.md`), checking off every step above that was completed, then commit:

```bash
git add docs/superpowers/plans/2026-08-18-bind-project.md
git commit -m "docs: mark bind-project plan complete"
```
