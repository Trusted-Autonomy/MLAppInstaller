# Repair + Force Reinstall Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the last capability gap between this project and cinepipe-installer's proven Rust engine: `mlai repair` (re-verify a component directly against disk, ignoring `installed.json`'s recorded state, so a manually broken install gets fixed rather than silently staying "healthy" forever) and `mlai install --force` (reinstall a component unconditionally, matching cinepipe's `-Force`).

**Architecture:** Extracts the existing `install_component`'s backup→fetch→unpack→setup→health→record sequence into a shared private helper (`run_install_sequence`), so `install_component` (trusts recorded state, unless `force`) and a new `repair_component` (always re-verifies disk, regardless of recorded state) share one implementation of the actual install mechanics rather than duplicating it. This mirrors cinepipe-installer's own `install_component`/`repair_component` split (`components.rs`) exactly — both call a shared `download_unpack_setup_and_health` tail.

**Tech Stack:** No new dependencies.

## Global Constraints

- `repair` must make zero filesystem changes when a component is genuinely healthy on disk, even if `installed.json` has no recorded state for it at all (`docs/CONSTITUTION.md` §3.2 — a component is not "installed" until health passes; `repair`'s whole purpose is to answer that question honestly from disk, not from a cached record).
- `force` bypasses only the "already healthy at this version" trust shortcut — backup-before-overwrite (§1.4) and guarded removals (§3.3) still apply unconditionally.
- Before every commit: `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` all pass, on all three CI platforms (`docs/CONSTITUTION.md` §5; `.github/workflows/ci.yml` now runs ubuntu-latest/macos-latest/windows-latest).
- Tests that depend on POSIX shell fixtures (`sh`, `#!/bin/sh`) are gated `#[cfg(unix)]`, matching the existing convention in `pipeline.rs`'s test module — do not write new sh-fixture tests outside that gate.

## Out of scope for this plan

- Remote-version resolution (detecting that a newer commit/release exists upstream and should be pulled) — cinepipe's `remote_version` resolves this via the GitHub commits API specifically, which would reintroduce the GitHub-specific coupling this project's generic `source_url` design deliberately avoided (see `docs/superpowers/specs/2026-08-14-foundation-design.md`). `--force` covers "reinstall from whatever `source_url` currently serves" without deciding that architecture question; real update-detection is a future design exploration, not decided here.
- `--dry-run` for `install`/`repair` — `uninstall` already has it (the highest-risk operation); extending it to install/repair is a reasonable follow-up, not required for this plan's scope.
- `mlai-cloud`, the credential-source glue design, and TA/CinePipe migration guides remain separate, already-tracked follow-ups.

---

### Task 1: Extract shared install sequence, add `force`

**Files:**
- Modify: `crates/mlai-core/src/pipeline.rs`
- Modify: `crates/mlai-cli/src/commands/install.rs`

**Interfaces:**
- Produces: `PipelineOptions.force: bool` (new field). `install_component`'s existing "already healthy at this version" short-circuit is skipped entirely when `opts.force` is `true`. Internal-only: `run_install_sequence(component: &Component, state: &mut InstalledState, opts: &PipelineOptions) -> Result<ComponentState, PipelineError>` and `apply_removals_and_persist_manifest_version(manifest: &Manifest, opts: &PipelineOptions) -> Result<InstalledState, PipelineError>` (private, not `pub` — Task 2 uses them from within the same module).

- [ ] **Step 1: Write the failing test**

In `crates/mlai-core/src/pipeline.rs`'s existing `#[cfg(all(test, unix))] mod tests` block, add (after `removals_older_than_the_manifest_are_applied_on_reinstall`):
```rust
    #[test]
    fn force_bypasses_the_already_healthy_short_circuit() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        let zip_path = fixture_dir.path().join("bundle.zip");
        build_fixture_zip(&zip_path);

        let component = sample_component();
        let fetcher = FixtureFetcher { zip_path };
        let manifest = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![component.clone()],
            removals: vec![],
        };
        let opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "abc123".into(),
            backup_keep: 3,
            set_options: vec![],
            force: false,
        };
        install_component(&component, &manifest, &opts).unwrap();

        // Break health without changing the recorded state — a plain
        // (non-forced) re-run must still trust the record and leave the
        // break in place.
        fs::remove_file(root.path().join("hello-component").join("marker.txt")).unwrap();
        let result = install_component(&component, &manifest, &opts).unwrap();
        assert_eq!(result, ComponentState::Healthy);
        assert!(
            !root.path().join("hello-component").join("marker.txt").exists(),
            "force: false must not touch disk when the record says healthy"
        );

        let forced_opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "abc123".into(),
            backup_keep: 3,
            set_options: vec![],
            force: true,
        };
        let result = install_component(&component, &manifest, &forced_opts).unwrap();
        assert_eq!(result, ComponentState::Healthy);
        assert!(
            root.path().join("hello-component").join("marker.txt").exists(),
            "force: true must bypass the short-circuit and actually reinstall"
        );
    }
```

Also add `force: false,` to every existing `PipelineOptions { .. }` literal in this test module: `installs_a_component_end_to_end_and_records_healthy_state`, `skips_reinstall_when_already_healthy_at_same_version`, both literals (`opts_v1`, `opts_v2`) in `backs_up_existing_install_before_replacing_it`, `set_options_are_appended_as_set_flags_to_setup`, and `removals_older_than_the_manifest_are_applied_on_reinstall`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test pipeline::`
Expected: FAIL to compile — `PipelineOptions` has no field `force` yet.

- [ ] **Step 3: Refactor the pipeline**

In `crates/mlai-core/src/pipeline.rs`, replace the `PipelineOptions` struct and `install_component` function with:
```rust
pub struct PipelineOptions<'a> {
    pub install_root: PathBuf,
    pub fetcher: &'a dyn Fetcher,
    pub version: String,
    pub backup_keep: usize,
    pub set_options: Vec<(String, String)>,
    pub force: bool,
}

pub fn install_component(
    component: &Component,
    manifest: &crate::manifest::Manifest,
    opts: &PipelineOptions,
) -> Result<ComponentState, PipelineError> {
    let mut state = apply_removals_and_persist_manifest_version(manifest, opts)?;

    if !opts.force {
        if let Some(existing) = state.components.get(&component.name) {
            if existing.version == opts.version && existing.state == ComponentState::Healthy {
                return Ok(ComponentState::Healthy);
            }
        }
    }

    run_install_sequence(component, &mut state, opts)
}

fn apply_removals_and_persist_manifest_version(
    manifest: &crate::manifest::Manifest,
    opts: &PipelineOptions,
) -> Result<InstalledState, PipelineError> {
    let mut state = InstalledState::load(&opts.install_root)?;
    let previous_manifest_version = if state.manifest_version.is_empty() {
        None
    } else {
        Some(state.manifest_version.clone())
    };
    crate::removals::apply_removals(
        &manifest.removals,
        previous_manifest_version.as_deref(),
        &opts.install_root,
        false,
    );
    if state.manifest_version != manifest.manifest_version {
        state.manifest_version = manifest.manifest_version.clone();
        state.save(&opts.install_root)?;
    }
    Ok(state)
}

fn run_install_sequence(
    component: &Component,
    state: &mut InstalledState,
    opts: &PipelineOptions,
) -> Result<ComponentState, PipelineError> {
    let existing_version = state
        .components
        .get(&component.name)
        .map(|r| r.version.clone());

    let component_dir = opts.install_root.join(&component.name);
    if component_dir.exists() {
        let backup_label = existing_version.as_deref().unwrap_or(&opts.version);
        backup_component(&opts.install_root, &component.name, backup_label)?;
        crate::backup::prune_backups(&opts.install_root, opts.backup_keep)?;
    }

    let zip_path = opts
        .install_root
        .join(".mlai-install")
        .join("downloads")
        .join(format!("{}.zip", component.name));
    opts.fetcher.fetch(&component.source_url, &zip_path)?;
    record_state(state, opts, component, ComponentState::Downloaded)?;

    let component_dir = unpack_zip(&zip_path, &opts.install_root, &component.name)?;
    record_state(state, opts, component, ComponentState::Unpacked)?;

    if let Some(setup) = component.setup_for_current_os() {
        run_setup(&component_dir, setup, &opts.set_options)?;
    }
    record_state(state, opts, component, ComponentState::SetupRun)?;

    let final_state = match check_health(&component_dir, component.health_for_current_os()) {
        HealthStatus::Healthy => ComponentState::Healthy,
        HealthStatus::NeedsAttention(_) => ComponentState::NeedsAttention,
    };
    record_state(state, opts, component, final_state)?;

    Ok(final_state)
}
```
(This is a refactor of the existing function body — same logic, split into three functions instead of one, plus the new `force` check. `record_state` itself is unchanged; only its callers' variable name changes from `&mut state` to `state` since it's now a `&mut InstalledState` parameter rather than a local.)

- [ ] **Step 4: Fix the now-broken call site in mlai-cli**

In `crates/mlai-cli/src/commands/install.rs`, add `force: false,` to the `PipelineOptions { .. }` literal (this task doesn't wire up a `--force` CLI flag yet — that's Task 4):
```rust
        let opts = PipelineOptions {
            install_root: install_root.to_path_buf(),
            fetcher: &fetcher,
            version: component.component_ref.clone(),
            backup_keep: 3,
            set_options: set_options.to_vec(),
            force: false,
        };
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS — all existing tests plus the new `force_bypasses_the_already_healthy_short_circuit`.

- [ ] **Step 6: Commit**

```bash
git add crates/mlai-core/src/pipeline.rs crates/mlai-cli/src/commands/install.rs
git commit -m "refactor(mlai-core): extract shared install sequence, add force option"
```

---

### Task 2: `repair_component`

**Files:**
- Modify: `crates/mlai-core/src/pipeline.rs`

**Interfaces:**
- Consumes: `run_install_sequence`, `apply_removals_and_persist_manifest_version` (Task 1, same file).
- Produces: `mlai_core::pipeline::repair_component(component: &Component, manifest: &Manifest, opts: &PipelineOptions) -> Result<(ComponentState, bool), PipelineError>` — the `bool` is `reinstalled`: `false` when the component was found genuinely healthy on disk (zero filesystem changes made), `true` when it fell through to a full reinstall.

**Ported from**: cinepipe-installer `feat/unified-rust-installer:wizard/src-tauri/src/components.rs`'s `repair_component` — same real-disk-health-check-first semantics (ignores `installed.json`'s recorded state entirely, unlike `install_component`'s trust-based shortcut), same "zero filesystem changes when genuinely healthy" guarantee their own test suite verifies.

- [ ] **Step 1: Write the failing test**

Add to `crates/mlai-core/src/pipeline.rs`'s test module:
```rust
    #[test]
    fn repair_reinstalls_when_disk_is_broken_despite_recorded_healthy_state() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        let zip_path = fixture_dir.path().join("bundle.zip");
        build_fixture_zip(&zip_path);

        let component = sample_component();
        let fetcher = FixtureFetcher { zip_path };
        let manifest = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![component.clone()],
            removals: vec![],
        };
        let opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "abc123".into(),
            backup_keep: 3,
            set_options: vec![],
            force: false,
        };
        install_component(&component, &manifest, &opts).unwrap();

        // Real bug this guards: installed.json says healthy, but the health
        // target is missing from disk (e.g. a user deleted it by hand).
        fs::remove_file(root.path().join("hello-component").join("marker.txt")).unwrap();

        let (state_after, reinstalled) = repair_component(&component, &manifest, &opts).unwrap();

        assert_eq!(state_after, ComponentState::Healthy);
        assert!(reinstalled, "repair must have gone through a real reinstall");
        assert!(
            root.path().join("hello-component").join("marker.txt").exists(),
            "repair should have fixed the health target"
        );
    }

    #[test]
    fn repair_makes_zero_filesystem_changes_when_genuinely_healthy_even_with_no_recorded_state() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        // Deliberately never build a fixture zip — repair must never reach
        // the download step for a genuinely healthy component.
        let zip_path = fixture_dir.path().join("bundle.zip");

        let component = sample_component();
        let component_dir = root.path().join("hello-component");
        fs::create_dir_all(&component_dir).unwrap();
        fs::write(component_dir.join("marker.txt"), "real marker").unwrap();

        let fetcher = FixtureFetcher { zip_path };
        let manifest = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![component.clone()],
            removals: vec![],
        };
        let opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "abc123".into(),
            backup_keep: 3,
            set_options: vec![],
            force: false,
        };

        // Deliberately no prior install_component call — installed.json has
        // no record for this component at all. Repair must decide from disk.
        let (state_after, reinstalled) = repair_component(&component, &manifest, &opts).unwrap();

        assert_eq!(state_after, ComponentState::Healthy);
        assert!(!reinstalled, "a genuinely healthy component must not be reinstalled");
        assert!(
            !root.path().join(".mlai-install").join("backups").exists(),
            "repair must not back up a genuinely healthy component"
        );
    }

    #[test]
    fn repair_reinstalls_when_the_component_directory_does_not_exist() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        let zip_path = fixture_dir.path().join("bundle.zip");
        build_fixture_zip(&zip_path);

        let component = sample_component();
        let fetcher = FixtureFetcher { zip_path };
        let manifest = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![component.clone()],
            removals: vec![],
        };
        let opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "abc123".into(),
            backup_keep: 3,
            set_options: vec![],
            force: false,
        };

        let (state_after, reinstalled) = repair_component(&component, &manifest, &opts).unwrap();

        assert_eq!(state_after, ComponentState::Healthy);
        assert!(reinstalled);
        assert!(root.path().join("hello-component").join("marker.txt").exists());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test pipeline::repair`
Expected: FAIL to compile — `repair_component` doesn't exist yet.

- [ ] **Step 3: Write the implementation**

Add to `crates/mlai-core/src/pipeline.rs`, after `install_component`:
```rust
/// Re-verifies a component's health directly against disk, ignoring any
/// recorded state in `installed.json` entirely — unlike `install_component`'s
/// trust-based shortcut, which skips reinstalling based on the record alone.
/// This is what a manually deleted or corrupted health target needs: a
/// plain re-install would silently skip it forever (the record still says
/// "healthy"), but repair always asks the filesystem directly. Returns
/// `(state, reinstalled)` — `reinstalled` is `false` when the component was
/// found genuinely healthy and zero filesystem changes were made.
pub fn repair_component(
    component: &Component,
    manifest: &crate::manifest::Manifest,
    opts: &PipelineOptions,
) -> Result<(ComponentState, bool), PipelineError> {
    let mut state = apply_removals_and_persist_manifest_version(manifest, opts)?;

    let component_dir = opts.install_root.join(&component.name);
    let genuinely_healthy = component_dir.exists()
        && check_health(&component_dir, component.health_for_current_os()) == HealthStatus::Healthy;
    if genuinely_healthy {
        return Ok((ComponentState::Healthy, false));
    }

    run_install_sequence(component, &mut state, opts).map(|final_state| (final_state, true))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd crates/mlai-core && cargo test pipeline::`
Expected: PASS — all tests, including the 3 new repair tests.

- [ ] **Step 5: Run the full mlai-core suite**

Run: `cd crates/mlai-core && cargo test`
Expected: PASS — all modules green.

- [ ] **Step 6: Commit**

```bash
git add crates/mlai-core/src/pipeline.rs
git commit -m "feat(mlai-core): add repair_component (ported from cinepipe-installer)"
```

---

### Task 3: `mlai repair` CLI command

**Files:**
- Create: `crates/mlai-cli/src/commands/repair.rs`
- Modify: `crates/mlai-cli/src/commands/mod.rs`
- Modify: `crates/mlai-cli/src/main.rs`
- Create: `crates/mlai-cli/tests/repair.rs`

**Interfaces:**
- Consumes: `mlai_core::pipeline::repair_component` (Task 2), `mlai_core::manifest::Manifest` (Plan A).
- Produces: `mlai repair --manifest <path> --install-root <dir> [--component <name>]`.

- [ ] **Step 1: Write the failing integration test**

`crates/mlai-cli/tests/repair.rs`:
```rust
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
    fs::remove_file(install_root.path().join("hello-component").join("marker.txt")).unwrap();

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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --workspace`
Expected: FAIL to compile — `crates/mlai-cli/src/commands/repair.rs` doesn't exist yet, and the CLI has no `repair` subcommand.

- [ ] **Step 3: Write `commands/repair.rs`**

`crates/mlai-cli/src/commands/repair.rs`:
```rust
use anyhow::{bail, Context, Result};
use mlai_core::fetch::HttpFetcher;
use mlai_core::manifest::Manifest;
use mlai_core::pipeline::{repair_component, PipelineOptions};
use mlai_core::state::ComponentState;
use std::fs;
use std::path::Path;

pub fn run(manifest_path: &Path, install_root: &Path, component_name: Option<&str>) -> Result<()> {
    let manifest_str = fs::read_to_string(manifest_path)
        .with_context(|| format!("reading manifest at {}", manifest_path.display()))?;
    let manifest = Manifest::parse(&manifest_str)
        .with_context(|| format!("parsing manifest at {}", manifest_path.display()))?;

    let components: Vec<_> = match component_name {
        Some(name) => match manifest.find_component(name) {
            Some(c) => vec![c],
            None => bail!("no component named '{name}' in {}", manifest_path.display()),
        },
        None => manifest.default_components(),
    };

    if components.is_empty() {
        bail!("no components to repair (manifest has no default components and none were named)");
    }

    let fetcher = HttpFetcher {
        token: std::env::var("MLAI_TOKEN").ok(),
    };

    for component in components {
        println!("Checking {}...", component.name);
        let opts = PipelineOptions {
            install_root: install_root.to_path_buf(),
            fetcher: &fetcher,
            version: component.component_ref.clone(),
            backup_keep: 3,
            set_options: Vec::new(),
            force: false,
        };
        let (state, reinstalled) = repair_component(component, &manifest, &opts)
            .with_context(|| format!("repairing component '{}'", component.name))?;
        match (state, reinstalled) {
            (ComponentState::Healthy, false) => {
                println!("  {} -> already healthy", component.name)
            }
            (ComponentState::Healthy, true) => println!("  {} -> repaired", component.name),
            (other, _) => println!("  {} -> {other:?} (NEEDS ATTENTION)", component.name),
        }
    }

    Ok(())
}
```

Modify `crates/mlai-cli/src/commands/mod.rs`:
```rust
pub mod install;
pub mod repair;
pub mod uninstall;
```

- [ ] **Step 4: Wire the CLI subcommand**

In `crates/mlai-cli/src/main.rs`, add to the `Commands` enum (after `Install`):
```rust
    /// Re-verify installed components against disk and fix any that are broken
    Repair {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        install_root: PathBuf,
        #[arg(long)]
        component: Option<String>,
    },
```

And to the `match cli.command` block:
```rust
        Commands::Repair { manifest, install_root, component } => {
            commands::repair::run(&manifest, &install_root, component.as_deref())
        }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS. Note: `repair_fixes_a_component_broken_on_disk` is `#[cfg(unix)]`-gated (its fixture uses `sh`); `repair_reports_already_healthy_without_reinstalling` runs on every platform.

- [ ] **Step 6: Commit**

```bash
git add crates/mlai-cli
git commit -m "feat(mlai-cli): add repair command"
```

---

### Task 4: `mlai install --force`

**Files:**
- Modify: `crates/mlai-cli/src/main.rs`
- Modify: `crates/mlai-cli/src/commands/install.rs`
- Modify: `crates/mlai-cli/tests/install.rs`

**Interfaces:**
- Produces: `mlai install ... --force` — forces reinstall of every selected component regardless of recorded state.

- [ ] **Step 1: Write the failing integration test**

Add to `crates/mlai-cli/tests/install.rs` (after `install_command_rejects_set_for_a_component_without_protocol_support`), gated the same way as the file's other sh-dependent test:
```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --workspace`
Expected: FAIL — `--force` isn't a recognized flag yet (clap reports an unknown argument), and `mock.assert()` would fail even if it were, since `PipelineOptions.force` isn't wired to anything yet.

- [ ] **Step 3: Wire the flag**

In `crates/mlai-cli/src/main.rs`, add to the `Install` variant:
```rust
    Install {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        install_root: PathBuf,
        #[arg(long)]
        component: Option<String>,
        #[arg(long = "set", value_parser = parse_set_option)]
        set: Vec<(String, String)>,
        /// Reinstall even components already healthy at their current version
        #[arg(long)]
        force: bool,
    },
```

Update the match arm:
```rust
        Commands::Install {
            manifest,
            install_root,
            component,
            set,
            force,
        } => commands::install::run(&manifest, &install_root, component.as_deref(), &set, force),
```

In `crates/mlai-cli/src/commands/install.rs`, add a `force: bool` parameter to `run` and thread it through:
```rust
pub fn run(
    manifest_path: &Path,
    install_root: &Path,
    component_name: Option<&str>,
    set_options: &[(String, String)],
    force: bool,
) -> Result<()> {
```
and change the `PipelineOptions { .. }` literal's `force: false,` (from Task 1) to `force,`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS — including the new force-reinstall test, which asserts (via `mock.assert()`) that the download endpoint was actually hit twice.

- [ ] **Step 5: Commit**

```bash
git add crates/mlai-cli
git commit -m "feat(mlai-cli): add install --force"
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

Add after the "Uninstalling" section:
```markdown
## Repairing

`mlai repair` re-verifies every component directly against disk, ignoring
whatever `installed.json` has recorded — the fix for a component a plain
re-run of `install` would silently keep trusting even after something on
disk broke it by hand:

```bash
mlai repair --manifest manifest.toml --install-root ~/my-app
```

A genuinely healthy component is left completely untouched (no download, no
setup re-run). A broken one goes through the same backup-then-reinstall
sequence `install` uses.

## Forcing a reinstall

```bash
mlai install --manifest manifest.toml --install-root ~/my-app --force
```

Reinstalls every selected component from `source_url` regardless of its
recorded state — the same backup-before-overwrite safety as a normal
install, just without the "already healthy, skip" shortcut. This is the
generic form of "get whatever is currently being served" — detecting that
a specific *newer* version exists upstream (vs. blindly re-pulling
`source_url`) isn't implemented yet; see
`docs/superpowers/specs/2026-08-14-foundation-design.md` for status.
```

Update "Not yet implemented":
```markdown
## Not yet implemented

Remote-version detection (upgrade because something changed upstream, not
just `--force`), cloud config generation, and the credential-source glue
layer are planned follow-ups — see
`docs/superpowers/specs/2026-08-14-foundation-design.md`.
```

- [ ] **Step 3: Commit**

```bash
git add docs/USAGE.md
git commit -m "docs: document repair and install --force"
```

- [ ] **Step 4: Final full-workspace verification**

Run: `cargo test --workspace`
Expected: PASS on the local platform. CI verifies all three (ubuntu-latest, macos-latest, windows-latest) once pushed.

---

## Self-Review Notes

- **Spec coverage**: `repair` (ported closely from cinepipe-installer's proven `repair_component`, matching its exact "zero changes when genuinely healthy, even with no recorded state" guarantee) and `--force` (matching cinepipe's `-Force`) are both covered. Remote-version detection is explicitly out of scope (architecture question deferred, not a gap) — see "Out of scope" above.
- **Placeholder scan**: no TBD/TODO markers; every step has complete, runnable code.
- **Type consistency**: `PipelineOptions.force`, `run_install_sequence`, `apply_removals_and_persist_manifest_version`, and `repair_component`'s `(ComponentState, bool)` return shape are each defined once (Tasks 1–2) and consumed identically in Tasks 3–4.
