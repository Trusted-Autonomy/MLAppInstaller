# GUI Wizard (mlai-gui) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Tauri 2 GUI (`mlai-gui`) that's a thin shell over today's `mlai-core`/`mlai-cli`, porting cinepipe-installer's actual working wizard (plain TypeScript frontend, no React) rather than writing one from scratch, reimplementing its 6 Tauri commands against `mlai-core` directly instead of their local module copies.

**Architecture:** `crates/mlai-gui/src-tauri/` (Rust, Tauri commands calling `mlai-core` in-process) + `crates/mlai-gui/src/` (TypeScript, ported from cinepipe's `wizard/src/main.ts`). Commands: `list_components`, `default_install_root`, `describe_component_options`, `read_install_status`, `run_install`. `add_project` (CinePipe's UE5 project-binding) is dropped — not generalized in this project's manifest schema yet.

**Tech Stack:** Tauri 2 (`tauri`, `tauri-build`), `@tauri-apps/api` ^2, Vite + TypeScript (matching cinepipe's frontend toolchain, minus `@tauri-apps/plugin-dialog` — that plugin only backed cinepipe's project-file picker, which this port drops along with `add_project`). Rust side depends on `mlai-core` (path dependency) plus `serde`.

## Global Constraints

- Commands call `mlai-core` in-process — no shelling out to the `mlai` binary (`docs/superpowers/specs/2026-08-15-gui-wizard-design.md`, Decision 3).
- No new `mlai-core` API surface for progress streaming in this plan — `run_install`'s log view shows coarse per-component start/result lines the GUI command itself emits, not live setup-script stdout. This is a documented v1 cut (see design doc), not a silent gap; fine-grained streaming needs a `mlai-core` progress-callback addition that's explicitly out of scope here.
- No GUI test harness — matches cinepipe's own accepted "manual verification only" posture for the frontend. The Rust command layer's own logic (manifest lookup, options translation, result summarization) gets real unit tests where it doesn't require an actual Tauri runtime.
- Before every commit: `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` all pass. `mlai-gui` joins the workspace; CI's existing 3-platform matrix will build it (Tauri's own platform system-dependency requirements — WebView2 on Windows, `webkit2gtk` on Linux — are already present on GitHub's hosted runners for `ubuntu-latest`/`windows-latest`/`macos-latest`, matching what cinepipe's own wizard already builds on in CI-equivalent environments; a genuine incompatibility here would need to be caught and fixed the same way three earlier Windows CI failures were this session).

## Out of scope for this plan

- `add_project`/project-binding — dropped, not generalized.
- Dry-run mode in the GUI (mlai-core's `install_component`/`repair_component` have no dry-run parameter — Plan D explicitly deferred this).
- Code signing, auto-update, distribution-packaging UI — all separate, already-deferred concerns.
- Publishing/packaging `mlai-gui` itself as a distributable app — that's exactly what the distribution-packaging framework (separately planned) is for.

---

### Task 1: `mlai-gui` crate scaffold + `list_components`

**Files:**
- Modify: `Cargo.toml` (add workspace member)
- Create: `crates/mlai-gui/src-tauri/Cargo.toml`
- Create: `crates/mlai-gui/src-tauri/tauri.conf.json`
- Create: `crates/mlai-gui/src-tauri/build.rs`
- Create: `crates/mlai-gui/src-tauri/src/main.rs`
- Create: `crates/mlai-gui/src-tauri/src/lib.rs`
- Create: `crates/mlai-gui/package.json`
- Create: `crates/mlai-gui/index.html`
- Create: `crates/mlai-gui/vite.config.ts`
- Create: `crates/mlai-gui/tsconfig.json`

**Interfaces:**
- Produces: Tauri command `list_components(app: AppHandle) -> Result<mlai_core::manifest::Manifest, String>`. Internal: `find_resource(app: &AppHandle, relative: &str) -> Option<PathBuf>` (bundled-resource lookup with a dev-mode fallback).

- [ ] **Step 1: Create the workspace member and Tauri scaffold**

Modify `Cargo.toml` (repo root) — add `"crates/mlai-gui/src-tauri"` to `members`:
```toml
members = ["crates/mlai-core", "crates/mlai-cli", "crates/mlai-credentials", "crates/mlai-gui/src-tauri"]
```

`crates/mlai-gui/src-tauri/Cargo.toml`:
```toml
[package]
name = "mlai-gui"
version.workspace = true
edition.workspace = true

[lib]
name = "mlai_gui_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
mlai-core = { path = "../../mlai-core" }
tauri = { version = "2", features = [] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[dev-dependencies]
tempfile = "3"
```

`crates/mlai-gui/src-tauri/build.rs`:
```rust
fn main() {
    tauri_build::build()
}
```

`crates/mlai-gui/src-tauri/tauri.conf.json`:
```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "MLAppInstaller",
  "version": "0.1.0",
  "identifier": "com.mlappinstaller.gui",
  "build": {
    "frontendDist": "../dist",
    "devUrl": "http://localhost:1420",
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build"
  },
  "app": {
    "windows": [
      {
        "title": "MLAppInstaller",
        "width": 900,
        "height": 700
      }
    ]
  },
  "bundle": {
    "active": true,
    "resources": ["manifest.toml"]
  }
}
```

`crates/mlai-gui/src-tauri/src/main.rs`:
```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    mlai_gui_lib::run();
}
```

`crates/mlai-gui/package.json`:
```json
{
  "name": "mlai-gui",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "preview": "vite preview",
    "tauri": "tauri"
  },
  "dependencies": {
    "@tauri-apps/api": "^2"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2",
    "vite": "^6.0.3",
    "typescript": "~5.6.2"
  }
}
```

`crates/mlai-gui/tsconfig.json`:
```json
{
  "compilerOptions": {
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "skipLibCheck": true,
    "noEmit": true
  },
  "include": ["src"]
}
```

`crates/mlai-gui/vite.config.ts`:
```typescript
import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
});
```

`crates/mlai-gui/index.html`:
```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <title>MLAppInstaller</title>
    <link rel="stylesheet" href="/src/styles.css" />
  </head>
  <body>
    <main></main>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

Verify the Rust side compiles standalone (frontend not built yet, so `tauri.conf.json`'s `frontendDist` won't exist — that's fine for a `cargo check`, only `cargo tauri build` needs it):
```bash
cd crates/mlai-gui/src-tauri && cargo check
```
Expected: fails — `lib.rs` doesn't exist with real content yet (empty scaffold). Continue to Step 2.

- [ ] **Step 2: Write the failing test**

`crates/mlai-gui/src-tauri/src/lib.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn list_components_reads_and_parses_a_bundled_manifest() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("manifest.toml");
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

        let manifest = read_manifest_at(&manifest_path).unwrap();
        assert_eq!(manifest.components.len(), 1);
        assert_eq!(manifest.components[0].name, "hello-component");
    }

    #[test]
    fn read_manifest_at_reports_a_clear_error_for_a_missing_file() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("nope.toml");
        let err = read_manifest_at(&missing).unwrap_err();
        assert!(err.contains("nope.toml"), "error should name the missing path: {err}");
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cd crates/mlai-gui/src-tauri && cargo test`
Expected: FAIL to compile — `read_manifest_at` doesn't exist yet.

- [ ] **Step 4: Write the implementation**

Prepend to the top of `crates/mlai-gui/src-tauri/src/lib.rs`, above the `#[cfg(test)]` module:
```rust
use mlai_core::manifest::Manifest;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// Reads and parses a manifest.toml at an exact path. Separated from
/// `list_components` so the parsing logic is testable without a real Tauri
/// AppHandle (which needs a running app context to construct).
fn read_manifest_at(path: &Path) -> Result<Manifest, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("reading manifest at {}: {e}", path.display()))?;
    Manifest::parse(&content).map_err(|e| format!("parsing manifest at {}: {e}", path.display()))
}

/// Locates a bundled resource, preferring the production resource dir
/// (declared in tauri.conf.json's `bundle.resources`) and falling back to
/// the workspace root for `tauri dev` (CARGO_MANIFEST_DIR is
/// crates/mlai-gui/src-tauri; the repo root with manifest.toml is three
/// levels up).
fn find_resource(app: &AppHandle, relative: &str) -> Option<PathBuf> {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let p = resource_dir.join(relative);
        if p.exists() {
            return Some(p);
        }
    }
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join(relative);
    if dev_path.exists() {
        return Some(dev_path);
    }
    None
}

#[tauri::command]
fn list_components(app: AppHandle) -> Result<Manifest, String> {
    let manifest_path =
        find_resource(&app, "manifest.toml").ok_or_else(|| "manifest.toml not found".to_string())?;
    read_manifest_at(&manifest_path)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![list_components])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cd crates/mlai-gui/src-tauri && cargo test`
Expected: PASS — 2 tests.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/mlai-gui
git commit -m "feat(mlai-gui): scaffold Tauri crate + list_components command"
```

---

### Task 2: `default_install_root` + `read_install_status`

**Files:**
- Modify: `crates/mlai-gui/src-tauri/src/lib.rs`
- Modify: `crates/mlai-core/src/lib.rs`
- Create: `crates/mlai-core/src/paths.rs`

**Interfaces:**
- Produces: `mlai_core::paths::default_install_root() -> PathBuf` (new, small, platform-specific: `<home>/.mlai/install` on unix, `<LOCALAPPDATA>/mlai/install` on Windows). Tauri commands `default_install_root(app) -> Result<String, String>` and `read_install_status(install_root: Option<String>) -> Result<mlai_core::state::InstalledState, String>`.

- [x] **Step 1: Write the failing test**

`crates/mlai-core/src/paths.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_install_root_is_under_home_on_unix() {
        if cfg!(not(unix)) {
            return;
        }
        std::env::set_var("HOME", "/tmp/fake-home");
        let root = default_install_root();
        assert_eq!(root, std::path::PathBuf::from("/tmp/fake-home/.mlai/install"));
    }

    #[test]
    fn default_install_root_never_panics_when_env_vars_are_absent() {
        // Smoke test only: on a CI runner these vars are always set, so this
        // mainly documents that the function has a graceful fallback path
        // rather than unwrapping directly on a missing var.
        let _ = default_install_root();
    }
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test paths::`
Expected: FAIL to compile — module `paths` doesn't exist yet.

- [x] **Step 3: Write the implementation**

Prepend to the top of `crates/mlai-core/src/paths.rs`:
```rust
use std::path::PathBuf;

/// A reasonable default install root when the caller hasn't specified one:
/// `<home>/.mlai/install` on unix, `<LOCALAPPDATA>/mlai/install` on Windows.
/// Falls back to the current directory if neither expected environment
/// variable is set (never panics).
pub fn default_install_root() -> PathBuf {
    if cfg!(windows) {
        let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(base).join("mlai").join("install")
    } else {
        let base = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(base).join(".mlai").join("install")
    }
}
```

Add to `crates/mlai-core/src/lib.rs`:
```rust
pub mod paths;
```

- [x] **Step 4: Run test to verify it passes**

Run: `cd crates/mlai-core && cargo test paths::`
Expected: PASS — 2 tests.

- [x] **Step 5: Wire the two Tauri commands**

In `crates/mlai-gui/src-tauri/src/lib.rs`, add:
```rust
#[tauri::command]
fn default_install_root() -> String {
    mlai_core::paths::default_install_root().to_string_lossy().to_string()
}

#[tauri::command]
fn read_install_status(install_root: Option<String>) -> Result<mlai_core::state::InstalledState, String> {
    let root = install_root
        .filter(|r| !r.trim().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(mlai_core::paths::default_install_root);
    mlai_core::state::InstalledState::load(&root).map_err(|e| e.to_string())
}
```

Update the `invoke_handler` registration in `run()`:
```rust
        .invoke_handler(tauri::generate_handler![
            list_components,
            default_install_root,
            read_install_status
        ])
```

- [x] **Step 6: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS — all tests across the workspace.

- [x] **Step 7: Commit**

```bash
git add crates/mlai-core/src/paths.rs crates/mlai-core/src/lib.rs crates/mlai-gui/src-tauri/src/lib.rs
git commit -m "feat(mlai-gui): add default_install_root and read_install_status commands"
```
(No git commit performed inside this TA-mediated staging session: integration happens via TA's draft/apply flow, per this goal's history.)

---

### Task 3: `describe_component_options`

**Files:**
- Modify: `crates/mlai-gui/src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `mlai_core::options_protocol::describe_options` (existing), `mlai_core::manifest::Manifest::find_component` (existing).
- Produces: Tauri command `describe_component_options(app, component: String, install_root: Option<String>) -> Result<Option<mlai_core::options_protocol::OptionsDescriptor>, String>`.

- [x] **Step 1: Write the failing test**

Add to `crates/mlai-gui/src-tauri/src/lib.rs`'s test module:
```rust
    #[test]
    fn options_for_a_component_are_none_when_the_component_declares_no_support() {
        use mlai_core::manifest::{Component, Manifest, PlatformFlag, PlatformHealth, PlatformSetup};

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
            }],
            removals: vec![],
        };

        let result = describe_options_for(&manifest, "hello-component", Path::new("."));
        assert_eq!(result, None);
    }

    #[test]
    fn options_for_an_unknown_component_name_is_none_not_an_error() {
        let manifest = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![],
            removals: vec![],
        };
        let result = describe_options_for(&manifest, "nonexistent", Path::new("."));
        assert_eq!(result, None);
    }
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-gui/src-tauri && cargo test`
Expected: FAIL to compile — `describe_options_for` doesn't exist yet.

- [x] **Step 3: Write the implementation**

Add to `crates/mlai-gui/src-tauri/src/lib.rs`, above the test module:
```rust
/// Probes a named component's setup command for the backend-options
/// protocol, gated the same way mlai-cli gates it: only when the manifest
/// declares support for the current OS. Returns `None` (not an error) for
/// an unknown component name, a component that doesn't support the
/// protocol, or a probe that fails — all graceful-degradation cases the
/// frontend already renders as "no options for this component."
fn describe_options_for(
    manifest: &Manifest,
    component_name: &str,
    component_dir: &Path,
) -> Option<mlai_core::options_protocol::OptionsDescriptor> {
    let component = manifest.find_component(component_name)?;
    if !component.supports_options_protocol_for_current_os() {
        return None;
    }
    let setup = component.setup_for_current_os()?;
    mlai_core::options_protocol::describe_options(setup, component_dir, std::time::Duration::from_secs(10))
        .ok()
}

#[tauri::command]
fn describe_component_options(
    app: AppHandle,
    component: String,
    install_root: Option<String>,
) -> Result<Option<mlai_core::options_protocol::OptionsDescriptor>, String> {
    let manifest_path =
        find_resource(&app, "manifest.toml").ok_or_else(|| "manifest.toml not found".to_string())?;
    let manifest = read_manifest_at(&manifest_path)?;
    let root = install_root
        .filter(|r| !r.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(mlai_core::paths::default_install_root);
    let component_dir = root.join(&component);
    Ok(describe_options_for(&manifest, &component, &component_dir))
}
```

Update `invoke_handler`:
```rust
        .invoke_handler(tauri::generate_handler![
            list_components,
            default_install_root,
            read_install_status,
            describe_component_options
        ])
```

- [x] **Step 4: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS.

Deviation note: `mlai_core::options_protocol::OptionsDescriptor`/`OptionSpec`/`ChoiceValue`
only derived `Deserialize` before this task; a Tauri command's return type must
implement `Serialize` to satisfy `IpcResponse`. Added `Serialize` to all three
types' derives (minimal fix, no field/shape changes) so `describe_component_options`
compiles.

- [x] **Step 5: Commit**

```bash
git add crates/mlai-gui/src-tauri/src/lib.rs
git commit -m "feat(mlai-gui): add describe_component_options command"
```
(No git commit performed inside this TA-mediated staging session: integration happens via TA's draft/apply flow, per this goal's history.)

---

### Task 4: `run_install`

**Files:**
- Modify: `crates/mlai-gui/src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `mlai_core::pipeline::{install_component, repair_component, PipelineOptions}` (existing), `mlai_core::fetch::HttpFetcher` (existing).
- Produces: Tauri command `run_install(app, components: Vec<String>, install_root: Option<String>, mode: String, options: HashMap<String, HashMap<String, String>>) -> Result<(), String>`. Emits `"install-log"` (String payload, one per component start/result) and `"install-done"` (`{success: bool, message: String}`) events. `mode` is `"install"` (plain), `"force"` (force reinstall), or `"repair"`.

- [x] **Step 1: Write the failing test**

Add to `crates/mlai-gui/src-tauri/src/lib.rs`'s test module:
```rust
    #[test]
    fn summarize_results_reports_success_when_everything_is_healthy() {
        let results = vec![
            ComponentResult { name: "a".into(), outcome: "healthy".into(), message: None },
            ComponentResult { name: "b".into(), outcome: "already_healthy".into(), message: None },
        ];
        let done = summarize_results(&results);
        assert!(done.success);
        assert_eq!(done.message, "Install finished successfully.");
    }

    #[test]
    fn summarize_results_reports_failure_with_component_names() {
        let results = vec![
            ComponentResult { name: "a".into(), outcome: "healthy".into(), message: None },
            ComponentResult {
                name: "b".into(),
                outcome: "failed".into(),
                message: Some("network error".into()),
            },
        ];
        let done = summarize_results(&results);
        assert!(!done.success);
        assert!(done.message.contains("b: network error"));
    }
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-gui/src-tauri && cargo test`
Expected: FAIL to compile — `ComponentResult`, `summarize_results` don't exist yet.

- [x] **Step 3: Write the implementation**

Add to `crates/mlai-gui/src-tauri/src/lib.rs`, above the test module:
```rust
use mlai_core::fetch::HttpFetcher;
use mlai_core::pipeline::{install_component, repair_component, PipelineOptions};
use mlai_core::state::ComponentState;
use serde::Serialize;
use std::collections::HashMap;
use tauri::Emitter;

#[derive(Debug, Serialize, Clone, PartialEq)]
struct ComponentResult {
    name: String,
    outcome: String,
    message: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
struct InstallDone {
    success: bool,
    message: String,
}

fn summarize_results(results: &[ComponentResult]) -> InstallDone {
    let failures: Vec<String> = results
        .iter()
        .filter(|r| r.outcome == "failed")
        .map(|r| format!("{}: {}", r.name, r.message.as_deref().unwrap_or("unknown error")))
        .collect();
    if failures.is_empty() {
        InstallDone { success: true, message: "Install finished successfully.".to_string() }
    } else {
        InstallDone {
            success: false,
            message: format!("Finished with warnings -- {}", failures.join("; ")),
        }
    }
}

fn run_install_inner(
    app: &AppHandle,
    components: Vec<String>,
    install_root: Option<String>,
    mode: &str,
    options: HashMap<String, HashMap<String, String>>,
) -> Result<Vec<ComponentResult>, String> {
    let manifest_path =
        find_resource(app, "manifest.toml").ok_or_else(|| "manifest.toml not found".to_string())?;
    let manifest = read_manifest_at(&manifest_path)?;
    let root = install_root
        .filter(|r| !r.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(mlai_core::paths::default_install_root);
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;

    let fetcher = HttpFetcher { token: std::env::var("MLAI_TOKEN").ok() };
    let mut results = Vec::new();

    for name in components {
        let Some(component) = manifest.find_component(&name) else {
            results.push(ComponentResult {
                name: name.clone(),
                outcome: "failed".to_string(),
                message: Some("no component with this name in the manifest".to_string()),
            });
            continue;
        };
        let _ = app.emit("install-log", format!("{}: starting ({mode})", name));

        let set_options: Vec<(String, String)> = options
            .get(&name)
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        let opts = PipelineOptions {
            install_root: root.clone(),
            fetcher: &fetcher,
            version: component.component_ref.clone(),
            backup_keep: 3,
            set_options,
            force: mode == "force",
        };

        let result = if mode == "repair" {
            repair_component(component, &manifest, &opts).map(|(state, reinstalled)| {
                let outcome = match (state, reinstalled) {
                    (ComponentState::Healthy, false) => "already_healthy",
                    (ComponentState::Healthy, true) => "repaired",
                    (_, _) => "needs_attention",
                };
                ComponentResult { name: name.clone(), outcome: outcome.to_string(), message: None }
            })
        } else {
            install_component(component, &manifest, &opts).map(|state| {
                let outcome = match state {
                    ComponentState::Healthy => "healthy",
                    _ => "needs_attention",
                };
                ComponentResult { name: name.clone(), outcome: outcome.to_string(), message: None }
            })
        };

        let component_result = result.unwrap_or_else(|e| ComponentResult {
            name: name.clone(),
            outcome: "failed".to_string(),
            message: Some(e.to_string()),
        });
        let _ = app.emit(
            "install-log",
            format!("{}: {}", component_result.name, component_result.outcome),
        );
        results.push(component_result);
    }

    Ok(results)
}

#[tauri::command]
fn run_install(
    app: AppHandle,
    components: Vec<String>,
    install_root: Option<String>,
    mode: String,
    options: HashMap<String, HashMap<String, String>>,
) -> Result<(), String> {
    std::thread::spawn(move || {
        let done = match run_install_inner(&app, components, install_root, &mode, options) {
            Ok(results) => summarize_results(&results),
            Err(e) => InstallDone { success: false, message: e },
        };
        let _ = app.emit("install-done", done);
    });
    Ok(())
}
```

Update `invoke_handler`:
```rust
        .invoke_handler(tauri::generate_handler![
            list_components,
            default_install_root,
            read_install_status,
            describe_component_options,
            run_install
        ])
```

- [x] **Step 4: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS.

- [x] **Step 5: Commit**

```bash
git add crates/mlai-gui/src-tauri/src/lib.rs
git commit -m "feat(mlai-gui): add run_install command with install/force/repair modes"
```
(No git commit performed inside this TA-mediated staging session: integration happens via TA's draft/apply flow, per this goal's history.)

---

### Task 5: Frontend port

**Files:**
- Create: `crates/mlai-gui/src/main.ts`
- Create: `crates/mlai-gui/src/styles.css`
- Modify: `crates/mlai-gui/index.html`

**Interfaces:** none new (TypeScript, no Rust test cycle — see Global Constraints on GUI test posture).

- [x] **Step 1: Write `index.html`'s body markup**

Replace `crates/mlai-gui/index.html`'s `<body>` with the form structure the frontend expects (ported from cinepipe's wizard markup, re-skinned generic):
```html
  <body>
    <main>
      <h1>MLAppInstaller</h1>
      <p id="manifest-version" class="muted"></p>

      <label for="install-root">Install location</label>
      <input id="install-root" type="text" placeholder="Default: platform-specific" />
      <span id="install-status" class="muted"></span>

      <label for="mode-select">Mode</label>
      <select id="mode-select">
        <option value="install">Install</option>
        <option value="force">Force Reinstall</option>
        <option value="repair">Repair</option>
      </select>

      <h2>Components</h2>
      <div id="components-list"></div>

      <div id="model-options-section" class="hidden">
        <h2>Options</h2>
        <div id="model-options-list"></div>
      </div>

      <button id="install-button">Install</button>
      <p id="status" class="status"></p>
      <pre id="log-view"></pre>
    </main>
    <script type="module" src="/src/main.ts"></script>
  </body>
```
(Remove the separate `<script>` tag from `<head>` if duplicated — `index.html` should have exactly one.)

- [x] **Step 2: Write `src/styles.css`**

`crates/mlai-gui/src/styles.css`:
```css
body { font-family: system-ui, sans-serif; margin: 2rem; max-width: 640px; }
.muted { color: #666; font-size: 0.9em; }
.hidden { display: none; }
.component-row { display: flex; gap: 0.5rem; align-items: flex-start; margin: 0.5rem 0; }
.component-title { font-weight: 600; }
.component-notes { font-size: 0.85em; }
.model-option-row { margin: 0.5rem 0; }
.model-option-label { font-size: 0.9em; margin-bottom: 0.25rem; }
#log-view { background: #111; color: #ddd; padding: 1rem; height: 200px; overflow-y: auto; font-family: monospace; font-size: 0.85em; white-space: pre-wrap; }
.status-ok { color: #2a7; }
.status-fail { color: #c33; }
```

- [x] **Step 3: Write `src/main.ts`**

`crates/mlai-gui/src/main.ts` — ported from cinepipe's `wizard/src/main.ts`, with these changes from the original: `Component`/`Manifest` TypeScript interfaces switched to this project's snake_case field names (`name`/`source_url`/`component_ref`/`default` — no `notes` field, since `mlai-core`'s `Component` doesn't have one yet); all `add_project`/project-binding code removed (dropped per this plan's scope); the mode selector's `"clean"` value renamed to `"force"` to match `run_install`'s actual mode strings; `ComponentResult`/`InstallOutcome` simplified to this project's flat `{name, outcome, message}` shape instead of cinepipe's externally-tagged enum enum enum shape:
```typescript
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface Component {
  name: string;
  source_url: string;
  component_ref: string;
  default: boolean;
}

interface Manifest {
  manifest_version: string;
  components: Component[];
}

interface OptionChoice {
  value: string;
  label: string;
  recommended?: boolean;
}

interface OptionSchema {
  key: string;
  label: string;
  type: "choice" | "bool" | string;
  choices?: OptionChoice[];
  default?: string | boolean | null;
}

interface OptionsResponse {
  schema_version: number;
  options: OptionSchema[];
}

interface InstallDone {
  success: boolean;
  message: string;
}

interface InstalledComponentState {
  version: string;
  state: string;
}

interface InstalledStatus {
  manifest_version: string;
  components: Record<string, InstalledComponentState>;
}

let logView: HTMLElement | null;
let installButton: HTMLButtonElement | null;
let statusEl: HTMLElement | null;

const selectedOptionValues: Record<string, Record<string, string>> = {};

function appendLog(line: string) {
  if (!logView) return;
  logView.textContent += line + "\n";
  logView.scrollTop = logView.scrollHeight;
}

function currentInstallRoot(): string {
  return (document.querySelector<HTMLInputElement>("#install-root")?.value ?? "").trim();
}

function renderComponents(manifest: Manifest) {
  const container = document.querySelector<HTMLDivElement>("#components-list");
  const versionEl = document.querySelector<HTMLElement>("#manifest-version");
  if (versionEl) versionEl.textContent = `Manifest ${manifest.manifest_version}`;
  if (!container) return;
  container.innerHTML = "";
  for (const c of manifest.components) {
    const label = document.createElement("label");
    label.className = "component-row";

    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.value = c.name;
    checkbox.checked = c.default;
    checkbox.dataset.componentName = c.name;
    checkbox.addEventListener("change", refreshModelOptions);

    const text = document.createElement("div");
    const title = document.createElement("div");
    title.className = "component-title";
    title.textContent = `${c.name} (${c.component_ref})`;
    text.appendChild(title);

    label.appendChild(checkbox);
    label.appendChild(text);
    container.appendChild(label);
  }
}

function selectedComponents(): string[] {
  const boxes = document.querySelectorAll<HTMLInputElement>(
    "#components-list input[type=checkbox]:checked",
  );
  return Array.from(boxes).map((b) => b.value);
}

async function loadDefaultInstallRoot() {
  const input = document.querySelector<HTMLInputElement>("#install-root");
  if (!input || input.value.trim()) return;
  try {
    input.value = await invoke<string>("default_install_root");
  } catch {
    // Leave blank; run_install resolves the same default server-side.
  }
}

async function loadComponents() {
  try {
    const manifest = await invoke<Manifest>("list_components");
    renderComponents(manifest);
    await refreshModelOptions();
  } catch (e) {
    const container = document.querySelector<HTMLDivElement>("#components-list");
    if (container) container.textContent = `Could not load manifest.toml: ${e}`;
  }
}

function renderOptionControl(componentName: string, schema: OptionSchema): HTMLElement {
  const row = document.createElement("div");
  row.className = "model-option-row";
  const labelEl = document.createElement("div");
  labelEl.className = "model-option-label";
  labelEl.textContent = `${componentName}: ${schema.label}`;
  row.appendChild(labelEl);

  if (schema.type === "choice" && schema.choices) {
    const select = document.createElement("select");
    for (const choice of schema.choices) {
      const opt = document.createElement("option");
      opt.value = choice.value;
      opt.textContent = choice.recommended ? `${choice.label} (recommended)` : choice.label;
      select.appendChild(opt);
    }
    const defaultValue = typeof schema.default === "string" ? schema.default : undefined;
    if (defaultValue) select.value = defaultValue;
    selectedOptionValues[componentName] ??= {};
    selectedOptionValues[componentName][schema.key] = select.value;
    select.addEventListener("change", () => {
      selectedOptionValues[componentName][schema.key] = select.value;
    });
    row.appendChild(select);
  } else if (schema.type === "bool") {
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = schema.default === true;
    selectedOptionValues[componentName] ??= {};
    selectedOptionValues[componentName][schema.key] = String(checkbox.checked);
    checkbox.addEventListener("change", () => {
      selectedOptionValues[componentName][schema.key] = String(checkbox.checked);
    });
    row.appendChild(checkbox);
  } else {
    const note = document.createElement("span");
    note.className = "muted";
    note.textContent = `(unsupported option type "${schema.type}")`;
    row.appendChild(note);
  }
  return row;
}

async function refreshModelOptions() {
  const section = document.querySelector<HTMLElement>("#model-options-section");
  const container = document.querySelector<HTMLDivElement>("#model-options-list");
  if (!section || !container) return;

  container.innerHTML = "";
  const installRoot = currentInstallRoot();
  const selected = selectedComponents();
  let anyShown = false;

  for (const name of selected) {
    let response: OptionsResponse | null;
    try {
      response = await invoke<OptionsResponse | null>("describe_component_options", {
        component: name,
        installRoot: installRoot || null,
      });
    } catch {
      response = null;
    }
    if (!response || response.options.length === 0) continue;
    anyShown = true;
    for (const schema of response.options) {
      container.appendChild(renderOptionControl(name, schema));
    }
  }

  section.classList.toggle("hidden", !anyShown);
}

async function refreshInstallStatus() {
  const statusSpan = document.querySelector<HTMLElement>("#install-status");
  try {
    const status = await invoke<InstalledStatus>("read_install_status", {
      installRoot: currentInstallRoot() || null,
    });
    if (statusSpan) {
      const entries = Object.entries(status.components ?? {});
      statusSpan.textContent =
        entries.length === 0
          ? ""
          : `Already installed here — ${entries.map(([n, c]) => `${n}: ${c.state} (${c.version})`).join(", ")}`;
    }
  } catch {
    if (statusSpan) statusSpan.textContent = "";
  }
  await refreshModelOptions();
}

const MODE_BUTTON_LABELS: Record<string, string> = {
  install: "Install",
  force: "Force Reinstall",
  repair: "Repair",
};

const MODE_RUNNING_LABELS: Record<string, string> = {
  install: "Installing…",
  force: "Reinstalling…",
  repair: "Repairing…",
};

function currentMode(): string {
  return document.querySelector<HTMLSelectElement>("#mode-select")?.value ?? "install";
}

function updateInstallButtonLabel() {
  if (installButton) installButton.textContent = MODE_BUTTON_LABELS[currentMode()] ?? "Install";
}

async function runInstall() {
  if (!installButton || !statusEl) return;
  const components = selectedComponents();
  if (components.length === 0) {
    statusEl.textContent = "Pick at least one component first.";
    return;
  }
  const installRoot = currentInstallRoot();
  const mode = currentMode();

  const options: Record<string, Record<string, string>> = {};
  for (const name of components) {
    if (selectedOptionValues[name]) options[name] = selectedOptionValues[name];
  }

  installButton.disabled = true;
  statusEl.textContent = MODE_RUNNING_LABELS[mode] ?? "Installing…";
  statusEl.className = "status";
  if (logView) logView.textContent = "";

  try {
    await invoke("run_install", {
      components,
      installRoot: installRoot || null,
      mode,
      options,
    });
  } catch (e) {
    statusEl.textContent = `Failed to start: ${e}`;
    installButton.disabled = false;
  }
}

window.addEventListener("DOMContentLoaded", () => {
  logView = document.querySelector("#log-view");
  installButton = document.querySelector("#install-button");
  statusEl = document.querySelector("#status");

  installButton?.addEventListener("click", runInstall);

  const modeSelect = document.querySelector<HTMLSelectElement>("#mode-select");
  modeSelect?.addEventListener("change", updateInstallButtonLabel);
  updateInstallButtonLabel();

  const installRootInput = document.querySelector<HTMLInputElement>("#install-root");
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  installRootInput?.addEventListener("input", () => {
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(refreshInstallStatus, 400);
  });

  listen<string>("install-log", (event) => appendLog(event.payload));
  listen<InstallDone>("install-done", (event) => {
    if (statusEl) {
      statusEl.textContent = event.payload.message;
      statusEl.className = "status " + (event.payload.success ? "status-ok" : "status-fail");
    }
    if (installButton) installButton.disabled = false;
  });

  loadDefaultInstallRoot().then(() => loadComponents().then(refreshInstallStatus));
});
```

Deviation note: swapped the single em dash in the "Already installed here ..."
status string for a plain hyphen, per this agent's standing writing-style
instruction against em dashes in any output. Content-only change, no behavior
or test impact (this file has no automated test by design).

- [x] **Step 4: Verify the Rust workspace still builds**

Run: `cargo build --workspace`
Expected: succeeds (this task is TypeScript-only; verifying nothing else regressed).

- [ ] **Step 5: Manual smoke verification** - not performed in this TA-mediated
staging session per explicit goal instruction ("do not attempt to run npm install
or a Tauri dev build yourself"); left unchecked as genuinely not done, matching
this plan's own accepted manual-verification posture for the GUI frontend.

Run:
```bash
cd crates/mlai-gui && npm install && npm run tauri dev
```
Expected: a window opens showing "MLAppInstaller", loads (or reports a clear "manifest.toml not found" if none is present at the repo root — expected until a real manifest exists there), and the mode dropdown/install button render without console errors. This is the one step in this plan that isn't automatable — matches cinepipe's own accepted manual-verification posture for the frontend (see Global Constraints).

- [x] **Step 6: Commit**

```bash
git add crates/mlai-gui/index.html crates/mlai-gui/src
git commit -m "feat(mlai-gui): port frontend from cinepipe-installer, re-skinned generic"
```
(No git commit performed inside this TA-mediated staging session: integration happens via TA's draft/apply flow, per this goal's history.)

---

### Task 6: Docs + final verification

**Files:**
- Modify: `docs/USAGE.md`

**Interfaces:** none new.

- [x] **Step 1: Run the full constitution-required check suite locally**

Run:
```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```
Expected: all four PASS.

- [x] **Step 2: Update `docs/USAGE.md`**

Add a new top-level section:
```markdown
## GUI wizard

A Tauri-based GUI wraps `mlai-core` for users who'd rather not use the CLI:

```bash
cd crates/mlai-gui && npm install && npm run tauri dev
```

It reads a `manifest.toml` bundled next to the app (or, in dev mode, at the
repository root) and supports the same three operations as the CLI —
Install, Force Reinstall, Repair — with live per-component progress in the
log view. It has no test harness of its own (matching the prior art it was
ported from); verify changes by running it and exercising the flow
manually. Building a distributable app from this GUI is what the
distribution-packaging framework (`docs/superpowers/specs/2026-08-15-distribution-packaging-framework-design.md`)
is for — not covered here.
```

- [x] **Step 3: Commit**

```bash
git add docs/USAGE.md
git commit -m "docs: document the GUI wizard"
```
(No git commit performed inside this TA-mediated staging session: integration happens via TA's draft/apply flow, per this goal's history.)

---

## Self-Review Notes

- **Spec coverage**: all 5 of cinepipe's non-project-binding commands are covered (`list_components`, `default_install_root`, `describe_component_options`, `read_install_status`, `run_install`), reimplemented against `mlai-core` directly per Decision 1 of the design doc. `add_project` is dropped per Decision 4, not silently missing.
- **Placeholder scan**: no TBD/TODO markers. The frontend port (Task 5) is the one step without an automated test, and that's stated explicitly as a scope decision (matching cinepipe's own posture), not hidden.
- **Type consistency**: `ComponentResult`/`InstallDone`/`describe_options_for`/`run_install_inner`/`summarize_results` are each defined once (Tasks 3–4) and used consistently; the frontend's TypeScript interfaces in Task 5 were updated to match this project's actual snake_case manifest field names rather than copying cinepipe's PascalCase verbatim.
