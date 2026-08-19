# Local Install Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a working, testable local component installer: `mlai install --manifest manifest.toml --install-root <dir>` downloads, unpacks, sets up, and health-checks components from a TOML manifest, with resumable state and backup-before-overwrite.

**Architecture:** Two crates in a Cargo workspace. `mlai-core` is a library owning the manifest schema, installed-state tracking, backup, health checks, HTTP fetch + zip unpack, and the pipeline orchestration that wires them together. `mlai-cli` is a thin `clap`-based binary (`mlai`) exposing one subcommand (`install`) over `mlai-core`.

**Tech Stack:** Rust (workspace, edition 2021), `serde`/`toml`/`serde_json` (manifest + state), `reqwest` blocking (HTTP fetch), `zip` 8.x (archive unpack), `thiserror` (library errors), `anyhow` (CLI errors), `clap` derive (CLI), `chrono` (timestamps). Dev/test: `tempfile`, `mockito` 1.x, `assert_cmd`, `predicates`.

## Global Constraints

- Single Rust codebase, cross-compiled — no per-OS script twins (`docs/CONSTITUTION.md` §1.7). This plan's CI runs on `ubuntu-latest` only; a Windows/macOS CI matrix is explicit follow-up work, not silently assumed done.
- Backup before overwrite: a component's live directory is copied aside before replacement, never overwritten in place (`docs/CONSTITUTION.md` §1.4).
- A component is not "installed" until its declared health check passes; failures report `NeedsAttention`, never silent success (`docs/CONSTITUTION.md` §3.2).
- Install state persists after every pipeline stage so a partial/crashed install resumes rather than restarts blindly (`docs/CONSTITUTION.md` §3.1).
- Every error type carries what happened and enough context to act on it (`docs/CONSTITUTION.md` §1.3) — use `thiserror` with named fields, never a bare string.
- Manifest format is TOML (`docs/superpowers/specs/2026-08-14-foundation-design.md`, "Additional decisions").
- Before every commit: `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` all pass (`docs/CONSTITUTION.md` §5).
- All filesystem-touching tests use `tempfile::tempdir()` — no hardcoded paths, no cross-run pollution.

## Out of scope for this plan

Deferred to follow-up plans (see `docs/superpowers/specs/2026-08-14-foundation-design.md`):
- `mlai-credentials` (vault) and the backend-options protocol (local vs. hosted model selection) — Plan B.
- `mlai repair` / `mlai uninstall` / `mlai update` CLI commands and versioned `removals` — Plan C.
- `mlai-cloud` (config generation + provider adapters) — Plan D.
- GUI wizard — later fast-follow phase.

---

### Task 1: Workspace scaffold + manifest parsing

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `crates/mlai-core/Cargo.toml`
- Create: `crates/mlai-core/src/lib.rs`
- Create: `crates/mlai-core/src/manifest.rs`

**Interfaces:**
- Produces: `mlai_core::manifest::{Manifest, Component, SetupCommand, HealthCheck, ManifestError}`. `Manifest::parse(toml_str: &str) -> Result<Manifest, ManifestError>`. `Manifest::default_components(&self) -> Vec<&Component>`. `Manifest::find_component(&self, name: &str) -> Option<&Component>`. `Component` fields: `name: String`, `source_url: String`, `component_ref: String` (TOML key `ref`), `default: bool`, `setup: Option<SetupCommand>`, `health: Option<HealthCheck>`.

- [x] **Step 1: Create the workspace and crate skeleton**

`Cargo.toml` (repo root):
```toml
[workspace]
resolver = "2"
members = ["crates/mlai-core", "crates/mlai-cli"]

[workspace.package]
version = "0.1.0"
edition = "2021"
```

`crates/mlai-core/Cargo.toml`:
```toml
[package]
name = "mlai-core"
version.workspace = true
edition.workspace = true

[dependencies]
serde = { version = "1", features = ["derive"] }
toml = "0.8"
serde_json = "1"
thiserror = "1"
reqwest = { version = "0.12", features = ["blocking"] }
zip = "8"
chrono = { version = "0.4", features = ["clock"] }

[dev-dependencies]
tempfile = "3"
mockito = "1"
```

`crates/mlai-core/src/lib.rs`:
```rust
pub mod manifest;
```

Note: `crates/mlai-cli` doesn't exist yet, so the workspace won't build until Task 7. Verify the crate compiles standalone instead:
```bash
cd crates/mlai-core && cargo build
```
Expected: succeeds (empty `manifest` module, no types yet — module file created next step).

- [x] **Step 2: Write the failing test**

`crates/mlai-core/src/manifest.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
manifest_version = "1.0.0"

[[components]]
name = "hello-component"
source_url = "https://example.com/hello-component.zip"
ref = "main"
default = true

[components.setup]
command = "setup.sh"
args = []

[components.health]
type = "file_exists"
path = "marker.txt"
"#;

    #[test]
    fn parses_a_component_with_setup_and_health() {
        let manifest = Manifest::parse(SAMPLE).expect("valid manifest");
        assert_eq!(manifest.manifest_version, "1.0.0");
        assert_eq!(manifest.components.len(), 1);
        let c = &manifest.components[0];
        assert_eq!(c.name, "hello-component");
        assert!(c.default);
        assert_eq!(c.component_ref, "main");
        assert_eq!(c.setup.as_ref().unwrap().command, "setup.sh");
        assert_eq!(
            c.health.as_ref().unwrap(),
            &HealthCheck::FileExists { path: "marker.txt".into() }
        );
    }

    #[test]
    fn find_component_returns_none_for_unknown_name() {
        let manifest = Manifest::parse(SAMPLE).unwrap();
        assert!(manifest.find_component("nope").is_none());
    }

    #[test]
    fn default_components_filters_on_default_flag() {
        let manifest = Manifest::parse(SAMPLE).unwrap();
        assert_eq!(manifest.default_components().len(), 1);
    }

    #[test]
    fn rejects_invalid_toml() {
        let err = Manifest::parse("not valid toml [[[").unwrap_err();
        assert!(matches!(err, ManifestError::Parse(_)));
    }
}
```

- [x] **Step 3: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test`
Expected: FAIL to compile — `Manifest`, `HealthCheck`, `ManifestError` are not defined.

- [x] **Step 4: Write the implementation**

Prepend this to the top of `crates/mlai-core/src/manifest.rs`, above the `#[cfg(test)]` module:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Manifest {
    pub manifest_version: String,
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Component {
    pub name: String,
    pub source_url: String,
    #[serde(rename = "ref")]
    pub component_ref: String,
    #[serde(default)]
    pub default: bool,
    pub setup: Option<SetupCommand>,
    pub health: Option<HealthCheck>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SetupCommand {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HealthCheck {
    FileExists { path: String },
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("failed to parse manifest TOML: {0}")]
    Parse(#[from] toml::de::Error),
}

impl Manifest {
    pub fn parse(toml_str: &str) -> Result<Manifest, ManifestError> {
        toml::from_str(toml_str).map_err(ManifestError::from)
    }

    pub fn default_components(&self) -> Vec<&Component> {
        self.components.iter().filter(|c| c.default).collect()
    }

    pub fn find_component(&self, name: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.name == name)
    }
}
```

- [x] **Step 5: Run test to verify it passes**

Run: `cd crates/mlai-core && cargo test`
Expected: PASS — 4 tests.

- [x] **Step 6: Commit**

```bash
git add Cargo.toml crates/mlai-core/Cargo.toml crates/mlai-core/src/lib.rs crates/mlai-core/src/manifest.rs
git commit -m "feat(mlai-core): add manifest schema and TOML parsing"
```

---

### Task 2: Installed-state tracking

**Files:**
- Create: `crates/mlai-core/src/state.rs`
- Modify: `crates/mlai-core/src/lib.rs`

**Interfaces:**
- Consumes: none new.
- Produces: `mlai_core::state::{ComponentState, ComponentRecord, InstalledState, StateError}`. `ComponentState` variants: `Downloaded, Unpacked, SetupRun, Healthy, NeedsAttention`. `InstalledState::state_path(install_root: &Path) -> PathBuf`. `InstalledState::load(install_root: &Path) -> Result<InstalledState, StateError>` (returns `InstalledState::default()` if no file exists). `InstalledState::save(&self, install_root: &Path) -> Result<(), StateError>`. `InstalledState.components: BTreeMap<String, ComponentRecord>`. `ComponentRecord { version: String, component_ref: String, state: ComponentState, installed_at: String }`.

- [x] **Step 1: Write the failing test**

`crates/mlai-core/src/state.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_returns_default_when_no_file_exists() {
        let dir = tempdir().unwrap();
        let state = InstalledState::load(dir.path()).unwrap();
        assert_eq!(state, InstalledState::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempdir().unwrap();
        let mut state = InstalledState::default();
        state.manifest_version = "1.0.0".into();
        state.components.insert(
            "hello-component".into(),
            ComponentRecord {
                version: "abc123".into(),
                component_ref: "main".into(),
                state: ComponentState::Healthy,
                installed_at: "2026-08-14T00:00:00Z".into(),
            },
        );
        state.save(dir.path()).unwrap();

        let loaded = InstalledState::load(dir.path()).unwrap();
        assert_eq!(loaded, state);
    }

    #[test]
    fn state_path_is_under_dot_mlai_install() {
        let dir = tempdir().unwrap();
        let path = InstalledState::state_path(dir.path());
        assert_eq!(path, dir.path().join(".mlai-install").join("installed.json"));
    }
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test state::`
Expected: FAIL to compile — module `state` doesn't exist yet.

- [x] **Step 3: Write the implementation**

Prepend to the top of `crates/mlai-core/src/state.rs`:
```rust
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentState {
    Downloaded,
    Unpacked,
    SetupRun,
    Healthy,
    NeedsAttention,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ComponentRecord {
    pub version: String,
    #[serde(rename = "ref")]
    pub component_ref: String,
    pub state: ComponentState,
    pub installed_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct InstalledState {
    #[serde(default)]
    pub manifest_version: String,
    #[serde(default)]
    pub components: BTreeMap<String, ComponentRecord>,
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("failed to read installed state at {path}: {source}")]
    Read { path: PathBuf, #[source] source: std::io::Error },
    #[error("failed to write installed state at {path}: {source}")]
    Write { path: PathBuf, #[source] source: std::io::Error },
    #[error("failed to parse installed state JSON at {path}: {source}")]
    Parse { path: PathBuf, #[source] source: serde_json::Error },
}

impl InstalledState {
    pub fn state_path(install_root: &Path) -> PathBuf {
        install_root.join(".mlai-install").join("installed.json")
    }

    pub fn load(install_root: &Path) -> Result<InstalledState, StateError> {
        let path = Self::state_path(install_root);
        if !path.exists() {
            return Ok(InstalledState::default());
        }
        let contents = std::fs::read_to_string(&path)
            .map_err(|source| StateError::Read { path: path.clone(), source })?;
        serde_json::from_str(&contents).map_err(|source| StateError::Parse { path, source })
    }

    pub fn save(&self, install_root: &Path) -> Result<(), StateError> {
        let path = Self::state_path(install_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|source| StateError::Write { path: path.clone(), source })?;
        }
        let json = serde_json::to_string_pretty(self).expect("InstalledState always serializes");
        std::fs::write(&path, json).map_err(|source| StateError::Write { path, source })
    }
}
```

Add to `crates/mlai-core/src/lib.rs`:
```rust
pub mod state;
```

- [x] **Step 4: Run test to verify it passes**

Run: `cd crates/mlai-core && cargo test state::`
Expected: PASS — 3 tests.

- [x] **Step 5: Commit**

```bash
git add crates/mlai-core/src/state.rs crates/mlai-core/src/lib.rs
git commit -m "feat(mlai-core): add installed-state tracking with JSON round-trip"
```

---

### Task 3: Backup before overwrite

**Files:**
- Create: `crates/mlai-core/src/backup.rs`
- Modify: `crates/mlai-core/src/lib.rs`

**Interfaces:**
- Consumes: none new.
- Produces: `mlai_core::backup::{backup_component, prune_backups, BackupError}`. `backup_component(install_root: &Path, component_name: &str, timestamp: &str) -> Result<PathBuf, BackupError>` — copies `install_root/<name>` to `install_root/.mlai-install/backups/<timestamp>/<name>`. `prune_backups(install_root: &Path, keep: usize) -> Result<(), BackupError>` — keeps the `keep` lexicographically-newest timestamp directories.

- [x] **Step 1: Write the failing test**

`crates/mlai-core/src/backup.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn backup_component_copies_directory_contents() {
        let root = tempdir().unwrap();
        let comp_dir = root.path().join("hello-component");
        fs::create_dir_all(comp_dir.join("nested")).unwrap();
        fs::write(comp_dir.join("file.txt"), b"v1").unwrap();
        fs::write(comp_dir.join("nested/inner.txt"), b"inner").unwrap();

        let dest = backup_component(root.path(), "hello-component", "2026-08-14T00-00-00").unwrap();

        assert_eq!(fs::read_to_string(dest.join("file.txt")).unwrap(), "v1");
        assert_eq!(fs::read_to_string(dest.join("nested/inner.txt")).unwrap(), "inner");
    }

    #[test]
    fn backup_component_errors_when_component_missing() {
        let root = tempdir().unwrap();
        let err = backup_component(root.path(), "missing", "ts").unwrap_err();
        assert!(matches!(err, BackupError::ComponentMissing(_)));
    }

    #[test]
    fn prune_backups_keeps_only_the_newest_n() {
        let root = tempdir().unwrap();
        for ts in [
            "2026-08-01T00-00-00",
            "2026-08-02T00-00-00",
            "2026-08-03T00-00-00",
            "2026-08-04T00-00-00",
        ] {
            fs::create_dir_all(root.path().join(".mlai-install").join("backups").join(ts)).unwrap();
        }
        prune_backups(root.path(), 3).unwrap();
        let remaining: Vec<_> = fs::read_dir(root.path().join(".mlai-install").join("backups"))
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().to_string()))
            .collect();
        assert_eq!(remaining.len(), 3);
        assert!(!remaining.contains(&"2026-08-01T00-00-00".to_string()));
    }
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test backup::`
Expected: FAIL to compile — module `backup` doesn't exist yet.

- [x] **Step 3: Write the implementation**

Prepend to the top of `crates/mlai-core/src/backup.rs`:
```rust
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("component directory not found at {0}")]
    ComponentMissing(PathBuf),
    #[error("backup I/O failure at {path}: {source}")]
    Io { path: PathBuf, #[source] source: std::io::Error },
}

fn backups_dir(install_root: &Path) -> PathBuf {
    install_root.join(".mlai-install").join("backups")
}

pub fn backup_component(
    install_root: &Path,
    component_name: &str,
    timestamp: &str,
) -> Result<PathBuf, BackupError> {
    let component_dir = install_root.join(component_name);
    if !component_dir.exists() {
        return Err(BackupError::ComponentMissing(component_dir));
    }
    let dest = backups_dir(install_root).join(timestamp).join(component_name);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|source| BackupError::Io { path: parent.to_path_buf(), source })?;
    }
    copy_dir_recursive(&component_dir, &dest)?;
    Ok(dest)
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), BackupError> {
    fs::create_dir_all(dest).map_err(|source| BackupError::Io { path: dest.to_path_buf(), source })?;
    for entry in fs::read_dir(src).map_err(|source| BackupError::Io { path: src.to_path_buf(), source })? {
        let entry = entry.map_err(|source| BackupError::Io { path: src.to_path_buf(), source })?;
        let file_type = entry
            .file_type()
            .map_err(|source| BackupError::Io { path: entry.path(), source })?;
        let dest_path = dest.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            fs::copy(entry.path(), &dest_path).map_err(|source| BackupError::Io { path: entry.path(), source })?;
        }
    }
    Ok(())
}

pub fn prune_backups(install_root: &Path, keep: usize) -> Result<(), BackupError> {
    let dir = backups_dir(install_root);
    if !dir.exists() {
        return Ok(());
    }
    let mut timestamps: Vec<PathBuf> = fs::read_dir(&dir)
        .map_err(|source| BackupError::Io { path: dir.clone(), source })?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    timestamps.sort();
    if timestamps.len() > keep {
        let to_remove = timestamps.len() - keep;
        for path in &timestamps[..to_remove] {
            fs::remove_dir_all(path).map_err(|source| BackupError::Io { path: path.clone(), source })?;
        }
    }
    Ok(())
}
```

Add to `crates/mlai-core/src/lib.rs`:
```rust
pub mod backup;
```

- [x] **Step 4: Run test to verify it passes**

Run: `cd crates/mlai-core && cargo test backup::`
Expected: PASS — 3 tests.

- [x] **Step 5: Commit**

```bash
git add crates/mlai-core/src/backup.rs crates/mlai-core/src/lib.rs
git commit -m "feat(mlai-core): add backup-before-overwrite with newest-N pruning"
```

---

### Task 4: Health checks

**Files:**
- Create: `crates/mlai-core/src/health.rs`
- Modify: `crates/mlai-core/src/lib.rs`

**Interfaces:**
- Consumes: `mlai_core::manifest::HealthCheck` (Task 1).
- Produces: `mlai_core::health::{HealthStatus, check_health}`. `HealthStatus` variants: `Healthy`, `NeedsAttention(String)`. `check_health(component_dir: &Path, health: Option<&HealthCheck>) -> HealthStatus` — `None` is always `Healthy`.

- [x] **Step 1: Write the failing test**

`crates/mlai-core/src/health.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::HealthCheck;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn no_health_check_declared_is_healthy() {
        let dir = tempdir().unwrap();
        assert_eq!(check_health(dir.path(), None), HealthStatus::Healthy);
    }

    #[test]
    fn file_exists_passes_when_file_present() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("marker.txt"), b"ok").unwrap();
        let health = HealthCheck::FileExists { path: "marker.txt".into() };
        assert_eq!(check_health(dir.path(), Some(&health)), HealthStatus::Healthy);
    }

    #[test]
    fn file_exists_fails_when_file_missing() {
        let dir = tempdir().unwrap();
        let health = HealthCheck::FileExists { path: "marker.txt".into() };
        let status = check_health(dir.path(), Some(&health));
        assert!(matches!(status, HealthStatus::NeedsAttention(_)));
    }
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test health::`
Expected: FAIL to compile — module `health` doesn't exist yet.

- [x] **Step 3: Write the implementation**

Prepend to the top of `crates/mlai-core/src/health.rs`:
```rust
use crate::manifest::HealthCheck;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
    Healthy,
    NeedsAttention(String),
}

pub fn check_health(component_dir: &Path, health: Option<&HealthCheck>) -> HealthStatus {
    let Some(health) = health else {
        return HealthStatus::Healthy;
    };
    match health {
        HealthCheck::FileExists { path } => {
            let target = component_dir.join(path);
            if target.exists() {
                HealthStatus::Healthy
            } else {
                HealthStatus::NeedsAttention(format!(
                    "expected file not found: {}",
                    target.display()
                ))
            }
        }
    }
}
```

Add to `crates/mlai-core/src/lib.rs`:
```rust
pub mod health;
```

- [x] **Step 4: Run test to verify it passes**

Run: `cd crates/mlai-core && cargo test health::`
Expected: PASS — 3 tests.

- [x] **Step 5: Commit**

```bash
git add crates/mlai-core/src/health.rs crates/mlai-core/src/lib.rs
git commit -m "feat(mlai-core): add health-check evaluation"
```

---

### Task 5: HTTP fetch + zip unpack

**Files:**
- Create: `crates/mlai-core/src/fetch.rs`
- Modify: `crates/mlai-core/src/lib.rs`, `crates/mlai-core/Cargo.toml`

**Interfaces:**
- Consumes: none new.
- Produces: `mlai_core::fetch::{Fetcher, HttpFetcher, unpack_zip, FetchError}`. `trait Fetcher { fn fetch(&self, url: &str, dest_zip: &Path) -> Result<(), FetchError>; }`. `HttpFetcher { pub token: Option<String> }` implements `Fetcher`. `unpack_zip(zip_path: &Path, dest_dir: &Path, component_name: &str) -> Result<PathBuf, FetchError>` — extracts, finds the archive's single top-level directory, renames it to `dest_dir/<component_name>`.

- [x] **Step 1: Add dev-dependency for archive fixtures in tests**

Add `mockito = "1"` is already present from Task 1's `Cargo.toml`; the `zip` dependency (production) is also already present. No `Cargo.toml` change needed for this task — confirm with:
```bash
grep -E '^(zip|mockito) = ' crates/mlai-core/Cargo.toml
```
Expected: both lines present (added in Task 1).

- [x] **Step 2: Write the failing test**

`crates/mlai-core/src/fetch.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn http_fetcher_downloads_bytes_to_dest() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("GET", "/bundle.zip")
            .with_status(200)
            .with_body(b"fake-zip-bytes")
            .create();

        let dir = tempdir().unwrap();
        let dest = dir.path().join("bundle.zip");
        let fetcher = HttpFetcher { token: None };
        fetcher
            .fetch(&format!("{}/bundle.zip", server.url()), &dest)
            .unwrap();

        assert_eq!(fs::read(&dest).unwrap(), b"fake-zip-bytes");
    }

    #[test]
    fn http_fetcher_errors_on_non_success_status() {
        let mut server = mockito::Server::new();
        let _mock = server.mock("GET", "/missing.zip").with_status(404).create();

        let dir = tempdir().unwrap();
        let dest = dir.path().join("missing.zip");
        let fetcher = HttpFetcher { token: None };
        let err = fetcher
            .fetch(&format!("{}/missing.zip", server.url()), &dest)
            .unwrap_err();

        assert!(matches!(err, FetchError::Status { status: 404, .. }));
    }

    fn build_fixture_zip(path: &Path) {
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.add_directory("hello-component-main/", options).unwrap();
        zip.start_file("hello-component-main/marker.txt", options).unwrap();
        zip.write_all(b"ok").unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn unpack_zip_renames_top_level_dir_to_component_name() {
        let dir = tempdir().unwrap();
        let zip_path = dir.path().join("bundle.zip");
        build_fixture_zip(&zip_path);

        let result_dir = unpack_zip(&zip_path, dir.path(), "hello-component").unwrap();

        assert_eq!(result_dir, dir.path().join("hello-component"));
        assert_eq!(fs::read_to_string(result_dir.join("marker.txt")).unwrap(), "ok");
    }
}
```

- [x] **Step 3: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test fetch::`
Expected: FAIL to compile — module `fetch` doesn't exist yet.

- [x] **Step 4: Write the implementation**

Prepend to the top of `crates/mlai-core/src/fetch.rs`:
```rust
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("HTTP request to {url} failed: {source}")]
    Http { url: String, #[source] source: reqwest::Error },
    #[error("HTTP request to {url} returned status {status}")]
    Status { url: String, status: u16 },
    #[error("I/O failure at {path}: {source}")]
    Io { path: PathBuf, #[source] source: std::io::Error },
    #[error("zip extraction failed: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("downloaded archive at {0} had no top-level directory")]
    NoTopLevelDir(PathBuf),
}

pub trait Fetcher {
    fn fetch(&self, url: &str, dest_zip: &Path) -> Result<(), FetchError>;
}

pub struct HttpFetcher {
    pub token: Option<String>,
}

impl Fetcher for HttpFetcher {
    fn fetch(&self, url: &str, dest_zip: &Path) -> Result<(), FetchError> {
        let client = reqwest::blocking::Client::new();
        let mut request = client.get(url);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .map_err(|source| FetchError::Http { url: url.to_string(), source })?;
        if !response.status().is_success() {
            return Err(FetchError::Status { url: url.to_string(), status: response.status().as_u16() });
        }
        let bytes = response
            .bytes()
            .map_err(|source| FetchError::Http { url: url.to_string(), source })?;
        if let Some(parent) = dest_zip.parent() {
            fs::create_dir_all(parent).map_err(|source| FetchError::Io { path: parent.to_path_buf(), source })?;
        }
        fs::write(dest_zip, &bytes).map_err(|source| FetchError::Io { path: dest_zip.to_path_buf(), source })
    }
}

/// Unpacks `zip_path` into `dest_dir`, renaming the archive's single
/// top-level folder to `component_name` so components land as predictable
/// sibling directories.
pub fn unpack_zip(zip_path: &Path, dest_dir: &Path, component_name: &str) -> Result<PathBuf, FetchError> {
    let file = fs::File::open(zip_path).map_err(|source| FetchError::Io { path: zip_path.to_path_buf(), source })?;
    let mut archive = zip::ZipArchive::new(file)?;

    let staging = dest_dir.join(format!(".{component_name}-staging"));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|source| FetchError::Io { path: staging.clone(), source })?;
    }
    archive.extract(&staging)?;

    let top_level = fs::read_dir(&staging)
        .map_err(|source| FetchError::Io { path: staging.clone(), source })?
        .filter_map(|e| e.ok())
        .find(|e| e.path().is_dir())
        .map(|e| e.path())
        .ok_or_else(|| FetchError::NoTopLevelDir(staging.clone()))?;

    let final_dir = dest_dir.join(component_name);
    if final_dir.exists() {
        fs::remove_dir_all(&final_dir).map_err(|source| FetchError::Io { path: final_dir.clone(), source })?;
    }
    fs::rename(&top_level, &final_dir).map_err(|source| FetchError::Io { path: final_dir.clone(), source })?;
    fs::remove_dir_all(&staging).ok();

    Ok(final_dir)
}
```

Add to `crates/mlai-core/src/lib.rs`:
```rust
pub mod fetch;
```

- [x] **Step 5: Run test to verify it passes**

Run: `cd crates/mlai-core && cargo test fetch::`
Expected: PASS — 3 tests.

- [x] **Step 6: Commit**

```bash
git add crates/mlai-core/src/fetch.rs crates/mlai-core/src/lib.rs
git commit -m "feat(mlai-core): add HTTP fetch and zip unpack with top-level rename"
```

---

### Task 6: Pipeline orchestration

**Files:**
- Create: `crates/mlai-core/src/pipeline.rs`
- Modify: `crates/mlai-core/src/lib.rs`

**Interfaces:**
- Consumes: `mlai_core::manifest::{Component, SetupCommand}` (Task 1), `mlai_core::state::{InstalledState, ComponentRecord, ComponentState}` (Task 2), `mlai_core::backup::{backup_component, prune_backups}` (Task 3), `mlai_core::health::{check_health, HealthStatus}` (Task 4), `mlai_core::fetch::{Fetcher, unpack_zip}` (Task 5).
- Produces: `mlai_core::pipeline::{PipelineOptions, install_component, PipelineError}`. `PipelineOptions<'a> { install_root: PathBuf, fetcher: &'a dyn Fetcher, version: String, backup_keep: usize }`. `install_component(component: &Component, opts: &PipelineOptions) -> Result<ComponentState, PipelineError>` — runs fetch → unpack → (backup if a live install exists) → setup → health, persisting `InstalledState` after every stage; short-circuits to the existing state when the component is already `Healthy` at the requested `version`.

- [x] **Step 1: Write the failing test**

`crates/mlai-core/src/pipeline.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Component, HealthCheck, SetupCommand};
    use std::fs;
    use std::io::Write;
    use std::path::Path;
    use tempfile::tempdir;

    struct FixtureFetcher {
        zip_path: PathBuf,
    }

    impl Fetcher for FixtureFetcher {
        fn fetch(&self, _url: &str, dest_zip: &Path) -> Result<(), crate::fetch::FetchError> {
            if let Some(parent) = dest_zip.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::copy(&self.zip_path, dest_zip).unwrap();
            Ok(())
        }
    }

    fn build_fixture_zip(path: &Path) {
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.add_directory("hello-component-main/", options).unwrap();
        zip.start_file("hello-component-main/setup.sh", options).unwrap();
        zip.write_all(b"#!/bin/sh\ntouch marker.txt\n").unwrap();
        zip.finish().unwrap();
    }

    fn sample_component() -> Component {
        Component {
            name: "hello-component".into(),
            source_url: "https://example.com/hello-component.zip".into(),
            component_ref: "main".into(),
            default: true,
            setup: Some(SetupCommand { command: "sh".into(), args: vec!["setup.sh".into()] }),
            health: Some(HealthCheck::FileExists { path: "marker.txt".into() }),
        }
    }

    #[test]
    fn installs_a_component_end_to_end_and_records_healthy_state() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        let zip_path = fixture_dir.path().join("bundle.zip");
        build_fixture_zip(&zip_path);

        let component = sample_component();
        let fetcher = FixtureFetcher { zip_path };
        let opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "abc123".into(),
            backup_keep: 3,
        };

        let result = install_component(&component, &opts).unwrap();
        assert_eq!(result, ComponentState::Healthy);

        let state = InstalledState::load(root.path()).unwrap();
        let record = state.components.get("hello-component").unwrap();
        assert_eq!(record.state, ComponentState::Healthy);
        assert_eq!(record.version, "abc123");
    }

    #[test]
    fn skips_reinstall_when_already_healthy_at_same_version() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        let zip_path = fixture_dir.path().join("bundle.zip");
        build_fixture_zip(&zip_path);

        let component = sample_component();
        let fetcher = FixtureFetcher { zip_path };
        let opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "abc123".into(),
            backup_keep: 3,
        };
        install_component(&component, &opts).unwrap();

        // Remove the setup script so a real re-run would fail — proves the
        // second call short-circuits instead of re-running setup.
        fs::remove_file(root.path().join("hello-component").join("setup.sh")).unwrap();

        let result = install_component(&component, &opts).unwrap();
        assert_eq!(result, ComponentState::Healthy);
    }

    #[test]
    fn backs_up_existing_install_before_replacing_it() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        let zip_path = fixture_dir.path().join("bundle.zip");
        build_fixture_zip(&zip_path);

        let component = sample_component();
        let fetcher = FixtureFetcher { zip_path };
        let opts_v1 = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "v1".into(),
            backup_keep: 3,
        };
        install_component(&component, &opts_v1).unwrap();

        let opts_v2 = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "v2".into(),
            backup_keep: 3,
        };
        install_component(&component, &opts_v2).unwrap();

        let backups_dir = root.path().join(".mlai-install").join("backups");
        assert!(backups_dir.join("v1").join("hello-component").exists());
    }
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test pipeline::`
Expected: FAIL to compile — module `pipeline` doesn't exist yet.

- [x] **Step 3: Write the implementation**

Prepend to the top of `crates/mlai-core/src/pipeline.rs`:
```rust
use crate::backup::backup_component;
use crate::fetch::{unpack_zip, Fetcher};
use crate::health::{check_health, HealthStatus};
use crate::manifest::{Component, SetupCommand};
use crate::state::{ComponentRecord, ComponentState, InstalledState};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error(transparent)]
    Fetch(#[from] crate::fetch::FetchError),
    #[error(transparent)]
    Backup(#[from] crate::backup::BackupError),
    #[error(transparent)]
    State(#[from] crate::state::StateError),
    #[error("setup command '{command}' failed to launch: {source}")]
    SetupLaunch { command: String, #[source] source: std::io::Error },
    #[error("setup command '{command}' exited with status {status}")]
    SetupFailed { command: String, status: i32 },
}

pub struct PipelineOptions<'a> {
    pub install_root: PathBuf,
    pub fetcher: &'a dyn Fetcher,
    pub version: String,
    pub backup_keep: usize,
}

pub fn install_component(component: &Component, opts: &PipelineOptions) -> Result<ComponentState, PipelineError> {
    let mut state = InstalledState::load(&opts.install_root)?;

    if let Some(existing) = state.components.get(&component.name) {
        if existing.version == opts.version && existing.state == ComponentState::Healthy {
            return Ok(ComponentState::Healthy);
        }
    }

    let component_dir = opts.install_root.join(&component.name);
    if component_dir.exists() {
        backup_component(&opts.install_root, &component.name, &opts.version)?;
        crate::backup::prune_backups(&opts.install_root, opts.backup_keep)?;
    }

    let zip_path = opts
        .install_root
        .join(".mlai-install")
        .join("downloads")
        .join(format!("{}.zip", component.name));
    opts.fetcher.fetch(&component.source_url, &zip_path)?;
    record_state(&mut state, opts, component, ComponentState::Downloaded)?;

    let component_dir = unpack_zip(&zip_path, &opts.install_root, &component.name)?;
    record_state(&mut state, opts, component, ComponentState::Unpacked)?;

    if let Some(setup) = &component.setup {
        run_setup(&component_dir, setup)?;
    }
    record_state(&mut state, opts, component, ComponentState::SetupRun)?;

    let final_state = match check_health(&component_dir, component.health.as_ref()) {
        HealthStatus::Healthy => ComponentState::Healthy,
        HealthStatus::NeedsAttention(_) => ComponentState::NeedsAttention,
    };
    record_state(&mut state, opts, component, final_state)?;

    Ok(final_state)
}

fn record_state(
    state: &mut InstalledState,
    opts: &PipelineOptions,
    component: &Component,
    component_state: ComponentState,
) -> Result<(), PipelineError> {
    state.components.insert(
        component.name.clone(),
        ComponentRecord {
            version: opts.version.clone(),
            component_ref: component.component_ref.clone(),
            state: component_state,
            installed_at: chrono::Utc::now().to_rfc3339(),
        },
    );
    state.save(&opts.install_root)?;
    Ok(())
}

fn run_setup(component_dir: &Path, setup: &SetupCommand) -> Result<(), PipelineError> {
    let status = Command::new(&setup.command)
        .args(&setup.args)
        .current_dir(component_dir)
        .status()
        .map_err(|source| PipelineError::SetupLaunch { command: setup.command.clone(), source })?;
    if !status.success() {
        return Err(PipelineError::SetupFailed {
            command: setup.command.clone(),
            status: status.code().unwrap_or(-1),
        });
    }
    Ok(())
}
```

Add to `crates/mlai-core/src/lib.rs`:
```rust
pub mod pipeline;
```

- [x] **Step 4: Run test to verify it passes**

Run: `cd crates/mlai-core && cargo test pipeline::`
Expected: PASS — 3 tests.

- [x] **Step 5: Run the full mlai-core suite**

Run: `cd crates/mlai-core && cargo test`
Expected: PASS — all tests across manifest, state, backup, health, fetch, pipeline (16 tests total).

- [x] **Step 6: Commit**

```bash
git add crates/mlai-core/src/pipeline.rs crates/mlai-core/src/lib.rs
git commit -m "feat(mlai-core): wire fetch/unpack/backup/setup/health into install pipeline"
```

---

### Task 7: `mlai-cli` install command

**Files:**
- Create: `crates/mlai-cli/Cargo.toml`
- Create: `crates/mlai-cli/src/main.rs`
- Create: `crates/mlai-cli/src/commands/mod.rs`
- Create: `crates/mlai-cli/src/commands/install.rs`
- Create: `crates/mlai-cli/tests/install.rs`

**Interfaces:**
- Consumes: `mlai_core::manifest::Manifest` (Task 1), `mlai_core::fetch::HttpFetcher` (Task 5), `mlai_core::pipeline::{install_component, PipelineOptions}` (Task 6), `mlai_core::state::ComponentState` (Task 2).
- Produces: `mlai` binary with `mlai install --manifest <path> --install-root <dir> [--component <name>]`.

- [x] **Step 1: Create the crate skeleton**

`crates/mlai-cli/Cargo.toml`:
```toml
[package]
name = "mlai-cli"
version.workspace = true
edition.workspace = true

[[bin]]
name = "mlai"
path = "src/main.rs"

[dependencies]
mlai-core = { path = "../mlai-core" }
clap = { version = "4", features = ["derive"] }
anyhow = "1"

[dev-dependencies]
assert_cmd = "2"
predicates = "3"
tempfile = "3"
mockito = "1"
zip = "8"
```

`crates/mlai-cli/src/commands/mod.rs`:
```rust
pub mod install;
```

`crates/mlai-cli/src/main.rs`:
```rust
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod commands;

#[derive(Parser)]
#[command(name = "mlai", version, about = "MLAppInstaller: cross-platform installer engine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install components from a manifest
    Install {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        install_root: PathBuf,
        #[arg(long)]
        component: Option<String>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Install { manifest, install_root, component } => {
            commands::install::run(&manifest, &install_root, component.as_deref())
        }
    }
}
```

Verify the workspace now builds (with `install.rs` still empty, this will fail — that's expected and is this task's failing state):
```bash
cargo build --workspace
```
Expected: FAIL — `commands::install::run` not found (file created next step).

- [x] **Step 2: Write the failing integration test**

`crates/mlai-cli/tests/install.rs`:
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
    zip.start_file("hello-component-main/setup.sh", options).unwrap();
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

    cmd.assert().success().stdout(contains("hello-component -> healthy"));

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
```

- [x] **Step 3: Run tests to verify they fail**

Run: `cargo test --workspace`
Expected: FAIL to compile — `crates/mlai-cli/src/commands/install.rs` doesn't exist yet.

- [x] **Step 4: Write the implementation**

`crates/mlai-cli/src/commands/install.rs`:
```rust
use anyhow::{bail, Context, Result};
use mlai_core::fetch::HttpFetcher;
use mlai_core::manifest::Manifest;
use mlai_core::pipeline::{install_component, PipelineOptions};
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
        bail!("no components to install (manifest has no default components and none were named)");
    }

    fs::create_dir_all(install_root)
        .with_context(|| format!("creating install root at {}", install_root.display()))?;

    let fetcher = HttpFetcher { token: std::env::var("MLAI_TOKEN").ok() };

    for component in components {
        println!("Installing {}...", component.name);
        let opts = PipelineOptions {
            install_root: install_root.to_path_buf(),
            fetcher: &fetcher,
            version: component.component_ref.clone(),
            backup_keep: 3,
        };
        let result = install_component(component, &opts)
            .with_context(|| format!("installing component '{}'", component.name))?;
        match result {
            ComponentState::Healthy => println!("  {} -> healthy", component.name),
            other => println!("  {} -> {other:?} (NEEDS ATTENTION)", component.name),
        }
    }

    Ok(())
}
```

- [x] **Step 5: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS — all `mlai-core` unit tests plus both `mlai-cli` integration tests.

- [x] **Step 6: Commit**

```bash
git add crates/mlai-cli
git commit -m "feat(mlai-cli): add install command wired to the mlai-core pipeline"
```

---

### Task 8: Docs + CI verification

**Files:**
- Create: `docs/USAGE.md`
- Modify: none (verifies existing `.github/workflows/ci.yml` now runs the real path)

**Interfaces:** none new.

- [x] **Step 1: Run the full constitution-required check suite locally**

Run:
```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```
Expected: all four PASS. If `cargo fmt` fails, run `cargo fmt --all` and re-check; if `cargo clippy` reports warnings, fix them before proceeding (per `docs/CONSTITUTION.md` §5, all four must pass before commit).

- [x] **Step 2: Write `docs/USAGE.md`**

`docs/USAGE.md`:
```markdown
# Using MLAppInstaller

## Installing components

`mlai install` reads a TOML manifest and installs every component marked
`default = true` (or a single named component via `--component`).

```bash
mlai install --manifest manifest.toml --install-root ~/my-app
```

## Manifest format

```toml
manifest_version = "1.0.0"

[[components]]
name = "hello-component"
source_url = "https://example.com/hello-component.zip"
ref = "main"
default = true

[components.setup]
command = "setup.sh"
args = []

[components.health]
type = "file_exists"
path = "marker.txt"
```

- `source_url` — a direct HTTPS URL to a zip archive. The archive's single
  top-level folder is renamed to the component's `name` after extraction.
- `ref` — recorded as the installed version; used to detect whether a
  re-run needs to reinstall.
- `setup` — optional command run inside the unpacked component directory
  after unpack.
- `health` — optional check run after setup; today supports `file_exists`.
  A component with no `health` block is always considered healthy once
  setup succeeds.

## Install state

State is written to `<install-root>/.mlai-install/installed.json` after
every pipeline stage (`downloaded`, `unpacked`, `setup_run`, `healthy`, or
`needs_attention`), so a crashed install resumes rather than restarts.
Re-running `mlai install` against a component already `healthy` at the
manifest's `ref` is a no-op.

## Backups

Before a component directory is replaced, its previous contents are copied
to `<install-root>/.mlai-install/backups/<ref>/<component-name>`. The
newest 3 backups are kept; older ones are pruned automatically.

## Private sources

Set `MLAI_TOKEN` in the environment to send a bearer token with the
component download request (for private/authenticated source URLs).

## Not yet implemented

`repair`, `uninstall`, `update`, local-vs-hosted backend selection, and
cloud config generation are planned follow-ups — see
`docs/superpowers/specs/2026-08-14-foundation-design.md`.
```

- [x] **Step 3: Commit**

```bash
git add docs/USAGE.md
git commit -m "docs: add USAGE.md for the local install engine"
```

- [ ] **Step 4: Push and verify CI runs the real (non-no-op) path**

```bash
git push -u origin <branch-name>
gh pr create --title "feat: local install engine (mlai-core + mlai-cli)" --body "Implements docs/superpowers/plans/2026-08-14-local-install-engine.md"
gh pr checks <pr-number> --watch
```
Expected: the `test` check passes, and its log (via the printed Actions URL) shows `cargo fmt`/`clippy`/`test`/`build` actually executing — not the "No Rust workspace yet" no-op branch from `.github/workflows/ci.yml`, since `Cargo.toml` now exists at the repo root.

**Not executed for this goal (dropped for this goal, not deferred to a future version):** this is a TA-mediated goal — the agent's changes are reviewed and integrated via TA's own draft/apply flow, not by the agent pushing directly and opening a PR itself (the goal's task instructions explicitly call for "never commit directly to main... not pushed directly"). CI verification of the real (non-no-op) path should happen once TA applies this work back to the source repo and it is pushed/PR'd through the normal human-mediated flow described in the root `CLAUDE.md`.

---

## Self-Review Notes

- **Spec coverage**: manifest schema, install pipeline (download/unpack/backup/setup/health), idempotent resumable state, and a CLI surface are all covered (spec sections "Architecture" and "Install pipeline"). Credentials, backend-option protocol, removals, repair/uninstall/update, and cloud config generation are explicitly out of scope for this plan (see "Out of scope" above) and are follow-up plans, not gaps in this one.
- **Placeholder scan**: no TBD/TODO markers; every step has complete, runnable code.
- **Type consistency**: `ComponentState`, `Component`, `SetupCommand`, `HealthCheck`, `Fetcher`, `PipelineOptions`, and `install_component` are defined once (Tasks 1/2/4/5/6) and consumed with matching names/signatures in every later task (7's `install.rs` imports match Task 6's exact public signatures).
