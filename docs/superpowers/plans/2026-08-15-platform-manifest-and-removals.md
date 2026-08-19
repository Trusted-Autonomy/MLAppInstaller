# Per-Platform Manifest + Guarded Removals + Uninstall Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close two real gaps surfaced by comparing this project against a mature, tested prior-art Rust installer port (an unmerged branch, ~3,400 lines): (1) the manifest can't express a component whose setup/health genuinely differs by OS, and (2) there's no safe way to remove files on upgrade or fully uninstall. Both are ported closely from that branch's `manifest.rs`/`cleanup.rs`, generalized (no product-specific fields, TOML/snake_case instead of their legacy-compat PascalCase JSON).

**Architecture:** `Component.setup`/`health`/`supports_options_protocol` become per-platform (`windows`/`posix`) structs with `_for_current_os()` accessor methods — the manifest can express two different setup commands, but `mlai` itself stays one cross-compiled binary that picks the right one via `cfg!(target_os = ...)` at runtime (no script twins). A new `mlai-core::removals` module ports cleanup.rs's hardened `safe_target` path guard, `apply_removals` (per-manifest-version legacy cleanup), and `clean_install` (full uninstall) essentially verbatim — the algorithm has no product-specific assumptions at all. A new `mlai-core::versioning` module ports `compare_version` (dotted-version comparison) to decide which `Removals` entries apply on upgrade.

**Tech Stack:** No new dependencies. Same crates as Plan A/B.

## Global Constraints

- No per-OS script twins for `mlai` itself — one cross-compiled binary; per-platform *setup commands* are a manifest concern, not an installer-implementation concern (`docs/CONSTITUTION.md` §1.7).
- Any path removed during upgrade or uninstall must be validated to resolve inside the install root — no exceptions (`docs/CONSTITUTION.md` §3.3).
- Uninstall confirms before deleting, unless `--yes` is passed; `--dry-run` is always available and must reflect exactly what a real run would do (`docs/CONSTITUTION.md` §1.2).
- Before every commit: `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` all pass (`docs/CONSTITUTION.md` §5).
- Ported code should closely match the source installer's proven algorithm and test coverage, not be reinvented from a paraphrase — see each task's "Ported from" note for the exact source file/function.

## Out of scope for this plan

- Repair mode (re-run setup+health without redownloading) and `mlai update` (remote version resolution via GitHub API, upgrade-in-place) — genuinely separate, larger pieces from the source installer's own `versioning.rs`/`components.rs`; a follow-up plan.
- `mlai-cloud` (config generation + provider adapters) — a separate plan.
- The credential-source glue design (`docs/superpowers/specs/2026-08-15-credential-source-glue-design.md`) — explicitly on hold.
- Project-binding (the source installer's `BindsToProjectType`/UE5 concept) — worth generalizing eventually, not needed for this plan's scope.
- Windows setup commands actually being tested — CI remains `ubuntu-latest` only; the `windows` half of each `Platform*` struct is exercised by unit tests asserting the *selection logic*, not by running real Windows commands.

---

### Task 1: Per-platform manifest retrofit

**Files:**
- Modify: `crates/mlai-core/src/manifest.rs`
- Modify: `crates/mlai-core/src/pipeline.rs`
- Modify: `crates/mlai-cli/src/commands/install.rs`
- Modify: `crates/mlai-cli/tests/install.rs`

**Interfaces:**
- Produces: `mlai_core::manifest::{PlatformSetup, PlatformHealth, PlatformFlag}` (each `Default`). `Component.setup: PlatformSetup`, `Component.health: PlatformHealth`, `Component.supports_options_protocol: PlatformFlag` (all `#[serde(default)]`, replacing the Plan A/B flat `Option<SetupCommand>`/`Option<HealthCheck>`/`bool` fields — a breaking schema change, acceptable pre-1.0). `Component::setup_for_current_os(&self) -> Option<&SetupCommand>`, `Component::health_for_current_os(&self) -> Option<&HealthCheck>`, `Component::supports_options_protocol_for_current_os(&self) -> bool`.

**Ported from**: the source installer's `wizard/src-tauri/src/manifest.rs`'s `PlatformSetup`/`PlatformHealth`/`PlatformFlag`/`setup_for_current_os`/`health_for_current_os`/`supports_options_protocol_for_current_os` — field names generalized from their PascalCase-JSON (`Setup`/`Health`/`SupportsOptionsProtocol`) to this project's existing snake_case/TOML convention; the `windows`/`posix` split and the `cfg!(target_os = ...)` selection logic are unchanged.

- [x] **Step 1: Write the failing test**

In `crates/mlai-core/src/manifest.rs`, replace the existing `SAMPLE` constant and its dependent tests with:
```rust
    const SAMPLE: &str = r#"
manifest_version = "1.0.0"

[[components]]
name = "hello-component"
source_url = "https://example.com/hello-component.zip"
ref = "main"
default = true

[components.setup.posix]
command = "setup.sh"
args = []

[components.health.posix]
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
        assert_eq!(c.setup.posix.as_ref().unwrap().command, "setup.sh");
        assert!(c.setup.windows.is_none());
        assert_eq!(
            c.health.posix.as_ref().unwrap(),
            &HealthCheck::FileExists {
                path: "marker.txt".into()
            }
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

    #[test]
    fn supports_options_protocol_defaults_to_false_when_absent() {
        let manifest = Manifest::parse(SAMPLE).unwrap();
        assert!(!manifest.components[0].supports_options_protocol_for_current_os());
    }

    #[test]
    fn supports_options_protocol_parses_when_present() {
        let toml = r#"
manifest_version = "1.0.0"

[[components]]
name = "hello-component"
source_url = "https://example.com/hello-component.zip"
ref = "main"
default = true

[components.supports_options_protocol]
posix = true
"#;
        let manifest = Manifest::parse(toml).unwrap();
        assert!(manifest.components[0].supports_options_protocol_for_current_os());
    }

    #[test]
    fn setup_for_current_os_is_none_when_only_the_other_platform_is_declared() {
        let toml = r#"
manifest_version = "1.0.0"

[[components]]
name = "hello-component"
source_url = "https://example.com/hello-component.zip"
ref = "main"
default = true

[components.setup.windows]
command = "powershell"
args = ["-File", "setup.ps1"]
"#;
        let manifest = Manifest::parse(toml).unwrap();
        // This suite runs on ubuntu-latest, so "current OS" is posix — the
        // windows-only setup entry must not be selected.
        assert!(manifest.components[0].setup_for_current_os().is_none());
    }
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test manifest::`
Expected: FAIL to compile — `PlatformSetup`, `PlatformHealth`, `setup_for_current_os`, `health_for_current_os`, `supports_options_protocol_for_current_os` don't exist yet; the existing `Component` struct's fields don't match the new TOML shape.

- [x] **Step 3: Retrofit the manifest types**

In `crates/mlai-core/src/manifest.rs`, replace the `Component` struct and add the new platform types:
```rust
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
pub struct PlatformSetup {
    #[serde(default)]
    pub windows: Option<SetupCommand>,
    #[serde(default)]
    pub posix: Option<SetupCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
pub struct PlatformHealth {
    #[serde(default)]
    pub windows: Option<HealthCheck>,
    #[serde(default)]
    pub posix: Option<HealthCheck>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
pub struct PlatformFlag {
    #[serde(default)]
    pub windows: bool,
    #[serde(default)]
    pub posix: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Component {
    pub name: String,
    pub source_url: String,
    #[serde(rename = "ref")]
    pub component_ref: String,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub setup: PlatformSetup,
    #[serde(default)]
    pub health: PlatformHealth,
    #[serde(default)]
    pub supports_options_protocol: PlatformFlag,
}

impl Component {
    /// This platform's setup command, or `None` if this component has no
    /// setup script for the OS `mlai` is running on.
    pub fn setup_for_current_os(&self) -> Option<&SetupCommand> {
        if cfg!(target_os = "windows") {
            self.setup.windows.as_ref()
        } else {
            self.setup.posix.as_ref()
        }
    }

    pub fn health_for_current_os(&self) -> Option<&HealthCheck> {
        if cfg!(target_os = "windows") {
            self.health.windows.as_ref()
        } else {
            self.health.posix.as_ref()
        }
    }

    pub fn supports_options_protocol_for_current_os(&self) -> bool {
        if cfg!(target_os = "windows") {
            self.supports_options_protocol.windows
        } else {
            self.supports_options_protocol.posix
        }
    }
}
```
(This replaces the old flat `pub setup: Option<SetupCommand>`, `pub health: Option<HealthCheck>`, `pub supports_options_protocol: bool` fields entirely.)

- [x] **Step 4: Run test to verify it passes**

Run: `cd crates/mlai-core && cargo test manifest::`
Expected: FAIL — this compiles now, but `pipeline.rs` and `mlai-cli` don't, since they still reference the old field shapes. Continue to the next steps before expecting a full pass.

- [x] **Step 5: Fix `pipeline.rs`'s call sites and test fixtures**

In `crates/mlai-core/src/pipeline.rs`, change:
```rust
    if let Some(setup) = &component.setup {
        run_setup(&component_dir, setup, &opts.set_options)?;
    }
```
to:
```rust
    if let Some(setup) = component.setup_for_current_os() {
        run_setup(&component_dir, setup, &opts.set_options)?;
    }
```

And change:
```rust
    let final_state = match check_health(&component_dir, component.health.as_ref()) {
```
to:
```rust
    let final_state = match check_health(&component_dir, component.health_for_current_os()) {
```

In the same file's test module, replace `sample_component()`:
```rust
    fn sample_component() -> Component {
        Component {
            name: "hello-component".into(),
            source_url: "https://example.com/hello-component.zip".into(),
            component_ref: "main".into(),
            default: true,
            setup: PlatformSetup {
                windows: None,
                posix: Some(SetupCommand {
                    command: "sh".into(),
                    args: vec!["setup.sh".into()],
                }),
            },
            health: PlatformHealth {
                windows: None,
                posix: Some(HealthCheck::FileExists {
                    path: "marker.txt".into(),
                }),
            },
            supports_options_protocol: PlatformFlag::default(),
        }
    }
```
and update this test module's imports to include the new types:
```rust
    use crate::manifest::{Component, HealthCheck, PlatformFlag, PlatformHealth, PlatformSetup, SetupCommand};
```

Change `set_options_are_appended_as_set_flags_to_setup`'s setup line:
```rust
        let mut component = sample_component();
        component.supports_options_protocol = true;
```
to:
```rust
        let mut component = sample_component();
        component.supports_options_protocol.posix = true;
```

- [x] **Step 6: Fix `mlai-cli`'s call site**

In `crates/mlai-cli/src/commands/install.rs`, change:
```rust
        if !set_options.is_empty() && !component.supports_options_protocol {
```
to:
```rust
        if !set_options.is_empty() && !component.supports_options_protocol_for_current_os() {
```

- [x] **Step 7: Fix the integration test's TOML fixture**

In `crates/mlai-cli/tests/install.rs`, in `install_command_installs_default_components_and_reports_healthy`, change:
```toml
[components.setup]
command = "sh"
args = ["setup.sh"]

[components.health]
type = "file_exists"
path = "marker.txt"
```
to:
```toml
[components.setup.posix]
command = "sh"
args = ["setup.sh"]

[components.health.posix]
type = "file_exists"
path = "marker.txt"
```

- [x] **Step 8: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS — all tests across the workspace, including the new manifest tests from Step 1.

- [x] **Step 9: Commit**

```bash
git add crates/mlai-core/src/manifest.rs crates/mlai-core/src/pipeline.rs crates/mlai-cli/src/commands/install.rs crates/mlai-cli/tests/install.rs
git commit -m "feat(mlai-core): retrofit manifest for per-platform setup/health (ported from the source installer)"
```

---

### Task 2: Dotted-version comparison

**Files:**
- Create: `crates/mlai-core/src/versioning.rs`
- Modify: `crates/mlai-core/src/lib.rs`

**Interfaces:**
- Produces: `mlai_core::versioning::compare_version(a: &str, b: &str) -> std::cmp::Ordering`.

**Ported from**: the source installer's `wizard/src-tauri/src/versioning.rs`'s `compare_version` — verbatim algorithm (element-wise dotted-integer comparison, non-numeric segments treated as 0, shorter side implicitly zero-padded). `remote_version`/`extract_commit_sha`/the `Installed` JSON-schema types are NOT ported here — they belong to the deferred `mlai update` plan.

- [x] **Step 1: Write the failing test**

`crates/mlai-core/src/versioning.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn equal_versions_compare_equal() {
        assert_eq!(compare_version("1.2.0", "1.2.0"), Ordering::Equal);
    }

    #[test]
    fn compares_numerically_not_lexically() {
        // Lexical comparison would put "1.10.0" before "1.2.0" — must not happen.
        assert_eq!(compare_version("1.10.0", "1.2.0"), Ordering::Greater);
    }

    #[test]
    fn shorter_version_is_implicitly_zero_padded() {
        assert_eq!(compare_version("1.2", "1.2.0"), Ordering::Equal);
        assert_eq!(compare_version("1.2.1", "1.2"), Ordering::Greater);
    }

    #[test]
    fn non_numeric_segments_are_treated_as_zero() {
        assert_eq!(compare_version("1.x.0", "1.0.0"), Ordering::Equal);
    }

    #[test]
    fn empty_string_compares_as_all_zero() {
        assert_eq!(compare_version("", "0.0.0"), Ordering::Equal);
    }
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test versioning::`
Expected: FAIL to compile — module `versioning` doesn't exist yet.

- [x] **Step 3: Write the implementation**

Prepend to the top of `crates/mlai-core/src/versioning.rs`:
```rust
use std::cmp::Ordering;

/// Dotted-version comparison ("1.2.0" vs "1.10.0", element-wise as integers,
/// non-numeric segments treated as 0, the shorter side implicitly
/// zero-padded). Ported from the source installer's `compare_version`.
pub fn compare_version(a: &str, b: &str) -> Ordering {
    let parts = |s: &str| -> Vec<i64> {
        if s.is_empty() {
            vec![0]
        } else {
            s.split('.').map(|seg| seg.parse().unwrap_or(0)).collect()
        }
    };
    let pa = parts(a);
    let pb = parts(b);
    let n = pa.len().max(pb.len());
    for i in 0..n {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}
```

Add to `crates/mlai-core/src/lib.rs`:
```rust
pub mod versioning;
```

- [x] **Step 4: Run test to verify it passes**

Run: `cd crates/mlai-core && cargo test versioning::`
Expected: PASS — 5 tests.

- [x] **Step 5: Commit**

```bash
git add crates/mlai-core/src/versioning.rs crates/mlai-core/src/lib.rs
git commit -m "feat(mlai-core): add dotted-version comparison (ported from the source installer)"
```

---

### Task 3: Guarded removals (`safe_target` + `apply_removals`)

**Files:**
- Modify: `crates/mlai-core/src/manifest.rs`
- Create: `crates/mlai-core/src/removals.rs`
- Modify: `crates/mlai-core/src/lib.rs`

**Interfaces:**
- Consumes: `mlai_core::versioning::compare_version` (Task 2).
- Produces: `mlai_core::manifest::RemovalEntry { version: String, paths: Vec<String> }`, `Manifest.removals: Vec<RemovalEntry>` (new field, `#[serde(default)]`). `mlai_core::removals::{safe_target, apply_removals}`. `safe_target(install_root: &Path, rel: &str) -> Option<PathBuf>`. `apply_removals(removals: &[RemovalEntry], installed_version: Option<&str>, install_root: &Path, dry_run: bool) -> usize`.

**Ported from**: the source installer's `wizard/src-tauri/src/cleanup.rs`'s `safe_target` and `apply_removals` — verbatim algorithm, including the component-by-component path resolution that fixes a real prefix-confusion vulnerability present in their own PowerShell original (documented in that file's header comment). `RemovalEntry` generalized from their PascalCase JSON (`Version`/`Paths`) to this project's snake_case/TOML fields.

- [x] **Step 1: Add `RemovalEntry` and the manifest field**

In `crates/mlai-core/src/manifest.rs`, add after the `HealthCheck` enum:
```rust
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct RemovalEntry {
    pub version: String,
    pub paths: Vec<String>,
}
```

Modify the `Manifest` struct to add the new field:
```rust
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Manifest {
    pub manifest_version: String,
    pub components: Vec<Component>,
    #[serde(default)]
    pub removals: Vec<RemovalEntry>,
}
```

Add a test to the existing `#[cfg(test)] mod tests` block:
```rust
    #[test]
    fn removals_default_to_empty_when_absent() {
        let manifest = Manifest::parse(SAMPLE).unwrap();
        assert!(manifest.removals.is_empty());
    }

    #[test]
    fn removals_parse_when_present() {
        let toml = r#"
manifest_version = "1.1.0"

[[components]]
name = "hello-component"
source_url = "https://example.com/hello-component.zip"
ref = "main"
default = true

[[removals]]
version = "1.1.0"
paths = ["hello-component/legacy_tool.py"]
"#;
        let manifest = Manifest::parse(toml).unwrap();
        assert_eq!(manifest.removals.len(), 1);
        assert_eq!(manifest.removals[0].version, "1.1.0");
        assert_eq!(manifest.removals[0].paths, vec!["hello-component/legacy_tool.py"]);
    }
```

Run: `cd crates/mlai-core && cargo test manifest::removals`
Expected: PASS — both new tests (the `#[serde(default)]` field parses with zero code beyond the struct/field additions).

- [x] **Step 2: Write the failing test for the removals module**

`crates/mlai-core/src/removals.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::RemovalEntry;
    use std::path::PathBuf;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mlai-removals-test-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn safe_target_rejects_parent_dir_escape() {
        let root = temp_root("escape");
        assert!(safe_target(&root, "../../etc/passwd").is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn safe_target_rejects_absolute_path() {
        let root = temp_root("absolute");
        assert!(safe_target(&root, "/etc/passwd").is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn safe_target_accepts_a_normal_relative_child() {
        let root = temp_root("normal-child");
        let result = safe_target(&root, "old-component").unwrap();
        assert_eq!(result, root.join("old-component"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn safe_target_accepts_a_nested_relative_child() {
        let root = temp_root("nested-child");
        let result = safe_target(&root, "hello-component/legacy_tool.py").unwrap();
        assert_eq!(result, root.join("hello-component").join("legacy_tool.py"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn safe_target_allows_an_internal_dotdot_that_stays_inside_root() {
        let root = temp_root("internal-dotdot");
        let result = safe_target(&root, "a/../b").unwrap();
        assert_eq!(result, root.join("b"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn safe_target_rejects_empty_rel_as_targeting_root_itself() {
        let root = temp_root("empty-rel");
        assert!(safe_target(&root, "").is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn apply_removals_skipped_entirely_on_fresh_install() {
        let root = temp_root("removals-fresh");
        let legacy = root.join("old-thing");
        std::fs::write(&legacy, "x").unwrap();
        let removals = vec![RemovalEntry {
            version: "1.1.0".to_string(),
            paths: vec!["old-thing".to_string()],
        }];

        let applied = apply_removals(&removals, None, &root, false);

        assert_eq!(applied, 0);
        assert!(legacy.exists(), "fresh install must not touch anything");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn apply_removals_only_applies_entries_newer_than_installed_version() {
        let root = temp_root("removals-versioned");
        std::fs::write(root.join("still-current"), "x").unwrap();
        std::fs::write(root.join("deprecated"), "x").unwrap();
        let removals = vec![
            RemovalEntry {
                version: "1.0.0".to_string(),
                paths: vec!["still-current".to_string()],
            },
            RemovalEntry {
                version: "1.5.0".to_string(),
                paths: vec!["deprecated".to_string()],
            },
        ];

        let applied = apply_removals(&removals, Some("1.2.0"), &root, false);

        assert_eq!(applied, 1);
        assert!(root.join("still-current").exists());
        assert!(!root.join("deprecated").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn apply_removals_dry_run_makes_no_filesystem_changes() {
        let root = temp_root("removals-dry-run");
        std::fs::write(root.join("deprecated"), "x").unwrap();
        let removals = vec![RemovalEntry {
            version: "1.5.0".to_string(),
            paths: vec!["deprecated".to_string()],
        }];

        let applied = apply_removals(&removals, Some("1.2.0"), &root, true);

        assert_eq!(applied, 1, "dry-run still reports what WOULD be removed");
        assert!(
            root.join("deprecated").exists(),
            "dry-run must not delete anything"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn apply_removals_traversal_attempt_is_rejected_not_deleted() {
        let root = temp_root("removals-traversal");
        let sibling_secret = root
            .parent()
            .unwrap()
            .join(format!(
                "{}-sibling-secret.txt",
                root.file_name().unwrap().to_string_lossy()
            ));
        std::fs::write(&sibling_secret, "do not delete me").unwrap();
        let escape_rel = format!(
            "../{}",
            sibling_secret.file_name().unwrap().to_string_lossy()
        );
        let removals = vec![RemovalEntry {
            version: "1.5.0".to_string(),
            paths: vec![escape_rel],
        }];

        let applied = apply_removals(&removals, Some("1.2.0"), &root, false);

        assert_eq!(applied, 0, "an out-of-root path must not count as applied");
        assert!(
            sibling_secret.exists(),
            "the traversal target must survive untouched"
        );
        std::fs::remove_file(&sibling_secret).ok();
        std::fs::remove_dir_all(&root).ok();
    }
}
```

- [x] **Step 3: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test removals::`
Expected: FAIL to compile — module `removals` doesn't exist yet.

- [x] **Step 4: Write the implementation**

Prepend to the top of `crates/mlai-core/src/removals.rs`:
```rust
// Guarded removals: apply per-manifest-version legacy cleanup and full
// uninstall, with a path guard that resolves a manifest-supplied relative
// path component-by-component so the result can never leave install_root's
// own subtree. This fixes a real prefix-confusion vulnerability class: a
// naive `path.starts_with(root)` check incorrectly accepts a sibling
// directory like "MyAppEvil" when root is "MyApp". This construction-based
// guard is a stronger fix, not just a different implementation of the same
// check.

use crate::manifest::RemovalEntry;
use crate::versioning::compare_version;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// Resolves `install_root` joined with `rel` (untrusted, manifest-supplied)
/// into an absolute path, constructing it component-by-component so the
/// result can never leave `install_root`'s own subtree: a `..` that would
/// pop above the install root is rejected outright, and an absolute path
/// smuggled into `rel` is rejected too.
pub fn safe_target(install_root: &Path, rel: &str) -> Option<PathBuf> {
    let root_canon = install_root.canonicalize().ok()?;
    let root_depth = root_canon.components().count();
    let mut result = root_canon.clone();

    for comp in Path::new(rel).components() {
        match comp {
            std::path::Component::Normal(seg) => result.push(seg),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if result.components().count() <= root_depth {
                    return None; // would escape above the install root
                }
                result.pop();
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => return None,
        }
    }

    if result == root_canon {
        return None; // an empty/no-op rel would target the root itself -- unsafe
    }
    Some(result)
}

fn remove_path(target: &Path) -> std::io::Result<()> {
    if target.is_dir() {
        std::fs::remove_dir_all(target)
    } else {
        std::fs::remove_file(target)
    }
}

/// Applies every `RemovalEntry` whose `version` is strictly newer than
/// `installed_version`. Skipped entirely when `installed_version` is `None`
/// (a fresh install has no legacy to clean). Returns the count of paths
/// actually removed (or that would be removed, under `dry_run`).
pub fn apply_removals(
    removals: &[RemovalEntry],
    installed_version: Option<&str>,
    install_root: &Path,
    dry_run: bool,
) -> usize {
    let Some(installed_version) = installed_version else {
        return 0;
    };
    let mut applied = 0;
    for entry in removals {
        if compare_version(&entry.version, installed_version) != Ordering::Greater {
            continue;
        }
        for rel in &entry.paths {
            let Some(target) = safe_target(install_root, rel) else {
                eprintln!("removal skipped (outside install root): {rel}");
                continue;
            };
            if !target.exists() {
                continue;
            }
            if dry_run {
                eprintln!("[dry-run] would remove legacy: {rel} (from {})", entry.version);
            } else {
                eprintln!("removing legacy: {rel} (deprecated in {})", entry.version);
                let _ = remove_path(&target);
            }
            applied += 1;
        }
    }
    applied
}
```

Add to `crates/mlai-core/src/lib.rs`:
```rust
pub mod removals;
```

- [x] **Step 5: Run test to verify it passes**

Run: `cd crates/mlai-core && cargo test removals::`
Expected: PASS — 9 tests.

- [x] **Step 6: Commit**

```bash
git add crates/mlai-core/src/manifest.rs crates/mlai-core/src/removals.rs crates/mlai-core/src/lib.rs
git commit -m "feat(mlai-core): add guarded removals (fixes a real path-guard bug)"
```

---

### Task 4: Full uninstall (`clean_install`, `remove_orphaned_components`)

**Files:**
- Modify: `crates/mlai-core/src/removals.rs`

**Interfaces:**
- Consumes: `safe_target` (Task 3, same file).
- Produces: `mlai_core::removals::{clean_install, remove_orphaned_components}`. `clean_install(component_names: &[String], install_root: &Path, dry_run: bool) -> usize`. `remove_orphaned_components(install_root: &Path, known_names: &[String], dry_run: bool) -> usize`.

**Ported from**: the source installer's `wizard/src-tauri/src/cleanup.rs`'s `clean_install`/`remove_orphaned_components` — verbatim algorithm, its own equivalent state-directory reserved name generalized to `.mlai-install` (matching this project's existing state-directory name from Plan A) with `venv` kept as a second reserved name (a shared virtualenv directory is a plausible cross-component convention worth preserving, matching the source installer's own).

- [x] **Step 1: Write the failing test**

Append to `crates/mlai-core/src/removals.rs`'s existing `#[cfg(test)] mod tests` block:
```rust
    #[test]
    fn clean_install_removes_named_components_and_state_dir() {
        let root = temp_root("clean-basic");
        std::fs::create_dir_all(root.join("hello-component")).unwrap();
        std::fs::create_dir_all(root.join("other-component")).unwrap();
        std::fs::create_dir_all(root.join(".mlai-install")).unwrap();
        std::fs::write(root.join("unrelated-file.txt"), "keep me").unwrap();

        let removed = clean_install(
            &["hello-component".to_string(), "other-component".to_string()],
            &root,
            false,
        );

        assert_eq!(removed, 3); // 2 components + .mlai-install
        assert!(!root.join("hello-component").exists());
        assert!(!root.join("other-component").exists());
        assert!(!root.join(".mlai-install").exists());
        assert!(
            root.join("unrelated-file.txt").exists(),
            "clean only touches known targets"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn clean_install_dry_run_makes_no_filesystem_changes() {
        let root = temp_root("clean-dry-run");
        std::fs::create_dir_all(root.join("hello-component")).unwrap();

        let removed = clean_install(&["hello-component".to_string()], &root, true);

        assert_eq!(removed, 1);
        assert!(
            root.join("hello-component").exists(),
            "dry-run must not delete anything"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn clean_install_on_nonexistent_root_is_a_no_op() {
        let root = std::env::temp_dir().join(format!(
            "mlai-removals-test-never-created-{}",
            std::process::id()
        ));
        let removed = clean_install(&["hello-component".to_string()], &root, false);
        assert_eq!(removed, 0);
    }

    #[test]
    fn remove_orphaned_components_removes_a_folder_matching_no_known_name() {
        let root = temp_root("orphan-basic");
        std::fs::create_dir_all(root.join("renamed-old-component")).unwrap();
        std::fs::write(root.join("renamed-old-component").join("data.txt"), "x").unwrap();

        let removed =
            remove_orphaned_components(&root, &["hello-component".to_string()], false);

        assert_eq!(removed, 1);
        assert!(!root.join("renamed-old-component").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn remove_orphaned_components_leaves_known_and_reserved_paths_alone() {
        let root = temp_root("orphan-known-reserved");
        std::fs::create_dir_all(root.join("hello-component")).unwrap();
        std::fs::create_dir_all(root.join(".mlai-install")).unwrap();
        std::fs::create_dir_all(root.join("venv")).unwrap();

        let removed =
            remove_orphaned_components(&root, &["hello-component".to_string()], false);

        assert_eq!(removed, 0);
        assert!(root.join("hello-component").exists());
        assert!(root.join(".mlai-install").exists());
        assert!(root.join("venv").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn remove_orphaned_components_dry_run_makes_no_filesystem_changes() {
        let root = temp_root("orphan-dry-run");
        std::fs::create_dir_all(root.join("renamed-old-component")).unwrap();

        let removed =
            remove_orphaned_components(&root, &["hello-component".to_string()], true);

        assert_eq!(removed, 1, "dry-run still reports what WOULD be removed");
        assert!(
            root.join("renamed-old-component").exists(),
            "dry-run must not delete anything"
        );
        std::fs::remove_dir_all(&root).ok();
    }
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test removals::`
Expected: FAIL to compile — `clean_install`/`remove_orphaned_components` don't exist yet.

- [x] **Step 3: Write the implementation**

Append to `crates/mlai-core/src/removals.rs` (after `apply_removals`, before the `#[cfg(test)]` module):
```rust
/// Full uninstall: removes every named component folder plus
/// `.mlai-install` under `install_root`. Returns the count removed (or
/// that would be removed, under `dry_run`).
pub fn clean_install(component_names: &[String], install_root: &Path, dry_run: bool) -> usize {
    if !install_root.exists() {
        return 0;
    }
    let mut targets: Vec<String> = component_names.to_vec();
    targets.push(".mlai-install".to_string());

    let mut removed = 0;
    for name in &targets {
        let Some(target) = safe_target(install_root, name) else {
            eprintln!("clean skipped (unsafe target): {name}");
            continue;
        };
        if !target.exists() {
            continue;
        }
        if dry_run {
            eprintln!("[dry-run] would UNINSTALL: {name}");
        } else {
            eprintln!("uninstalling: {name}");
            let _ = remove_path(&target);
        }
        removed += 1;
    }
    removed
}

/// Scans `install_root`'s top-level entries and removes anything that is
/// neither a current manifest component name nor a reserved path
/// (`.mlai-install`, `venv`) — a component removed or renamed from the
/// manifest since it was installed. Independent of any particular run's
/// component selection: a whole-install-root reconciliation against what
/// the manifest currently names, not scoped to what's being installed now.
pub fn remove_orphaned_components(
    install_root: &Path,
    known_names: &[String],
    dry_run: bool,
) -> usize {
    if !install_root.exists() {
        return 0;
    }
    const RESERVED: [&str; 2] = [".mlai-install", "venv"];
    let Ok(entries) = std::fs::read_dir(install_root) else {
        return 0;
    };

    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if known_names.iter().any(|k| k == name_str.as_ref())
            || RESERVED.contains(&name_str.as_ref())
        {
            continue;
        }
        let Some(target) = safe_target(install_root, &name_str) else {
            eprintln!("orphan cleanup skipped (unsafe target): {name_str}");
            continue;
        };
        if dry_run {
            eprintln!("[dry-run] would remove orphaned component: {name_str}");
        } else {
            eprintln!("removing orphaned component: {name_str}");
            let _ = remove_path(&target);
        }
        removed += 1;
    }
    removed
}
```

- [x] **Step 4: Run test to verify it passes**

Run: `cd crates/mlai-core && cargo test removals::`
Expected: PASS — 14 tests total in this module (9 from Task 3 + 5 new).

- [x] **Step 5: Run the full mlai-core suite**

Run: `cd crates/mlai-core && cargo test`
Expected: PASS — all modules green.

- [x] **Step 6: Commit**

```bash
git add crates/mlai-core/src/removals.rs
git commit -m "feat(mlai-core): add full uninstall + orphaned-component cleanup (ported from the source installer)"
```

---

### Task 5: `mlai uninstall` CLI command

**Files:**
- Create: `crates/mlai-cli/src/commands/uninstall.rs`
- Modify: `crates/mlai-cli/src/commands/mod.rs`
- Modify: `crates/mlai-cli/src/main.rs`
- Create: `crates/mlai-cli/tests/uninstall.rs`

**Interfaces:**
- Consumes: `mlai_core::manifest::Manifest` (Plan A), `mlai_core::removals::clean_install` (Task 4).
- Produces: `mlai uninstall --manifest <path> --install-root <dir> [--yes] [--dry-run]`.

- [x] **Step 1: Write the failing integration test**

`crates/mlai-cli/tests/uninstall.rs`:
```rust
use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
use tempfile::tempdir;

fn write_manifest(path: &std::path::Path) {
    fs::write(
        path,
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
}

#[test]
fn uninstall_with_yes_removes_the_component_directory() {
    let manifest_dir = tempdir().unwrap();
    let manifest_path = manifest_dir.path().join("manifest.toml");
    write_manifest(&manifest_path);

    let install_root = tempdir().unwrap();
    fs::create_dir_all(install_root.path().join("hello-component")).unwrap();

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("uninstall")
        .arg("--manifest")
        .arg(&manifest_path)
        .arg("--install-root")
        .arg(install_root.path())
        .arg("--yes");

    cmd.assert().success().stdout(contains("Removed 1"));
    assert!(!install_root.path().join("hello-component").exists());
}

#[test]
fn uninstall_dry_run_reports_without_deleting() {
    let manifest_dir = tempdir().unwrap();
    let manifest_path = manifest_dir.path().join("manifest.toml");
    write_manifest(&manifest_path);

    let install_root = tempdir().unwrap();
    fs::create_dir_all(install_root.path().join("hello-component")).unwrap();

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("uninstall")
        .arg("--manifest")
        .arg(&manifest_path)
        .arg("--install-root")
        .arg(install_root.path())
        .arg("--dry-run")
        .arg("--yes");

    cmd.assert().success().stdout(contains("Would remove 1"));
    assert!(install_root.path().join("hello-component").exists());
}

#[test]
fn uninstall_without_yes_or_a_tty_fails_clearly_rather_than_hanging() {
    let manifest_dir = tempdir().unwrap();
    let manifest_path = manifest_dir.path().join("manifest.toml");
    write_manifest(&manifest_path);

    let install_root = tempdir().unwrap();
    fs::create_dir_all(install_root.path().join("hello-component")).unwrap();

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("uninstall")
        .arg("--manifest")
        .arg(&manifest_path)
        .arg("--install-root")
        .arg(install_root.path())
        .write_stdin(""); // EOF immediately — simulates a non-interactive/no-tty run

    cmd.assert()
        .failure()
        .stderr(contains("confirmation required"));
    assert!(install_root.path().join("hello-component").exists());
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test --workspace`
Expected: FAIL to compile — `crates/mlai-cli/src/commands/uninstall.rs` doesn't exist yet, and the CLI doesn't have an `uninstall` subcommand.

- [x] **Step 3: Write `commands/uninstall.rs`**

`crates/mlai-cli/src/commands/uninstall.rs`:
```rust
use anyhow::{bail, Context, Result};
use mlai_core::manifest::Manifest;
use mlai_core::removals::clean_install;
use std::fs;
use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

pub fn run(manifest_path: &Path, install_root: &Path, yes: bool, dry_run: bool) -> Result<()> {
    let manifest_str = fs::read_to_string(manifest_path)
        .with_context(|| format!("reading manifest at {}", manifest_path.display()))?;
    let manifest = Manifest::parse(&manifest_str)
        .with_context(|| format!("parsing manifest at {}", manifest_path.display()))?;
    let component_names: Vec<String> = manifest.components.iter().map(|c| c.name.clone()).collect();

    if !yes && !dry_run {
        confirm_or_bail(install_root)?;
    }

    let removed = clean_install(&component_names, install_root, dry_run);

    if dry_run {
        println!("Would remove {removed} item(s) from {}", install_root.display());
    } else {
        println!("Removed {removed} item(s) from {}", install_root.display());
    }
    Ok(())
}

fn confirm_or_bail(install_root: &Path) -> Result<()> {
    if !std::io::stdin().is_terminal() {
        bail!(
            "confirmation required to uninstall {} — pass --yes to proceed non-interactively",
            install_root.display()
        );
    }
    eprint!(
        "This will permanently remove all components under {}. Continue? [y/N] ",
        install_root.display()
    );
    std::io::stderr().flush().ok();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    if answer.trim().eq_ignore_ascii_case("y") {
        Ok(())
    } else {
        bail!("uninstall cancelled");
    }
}
```

Modify `crates/mlai-cli/src/commands/mod.rs`:
```rust
pub mod install;
pub mod uninstall;
```

- [x] **Step 4: Wire the CLI subcommand**

In `crates/mlai-cli/src/main.rs`, add to the `Commands` enum (after `Install`):
```rust
    /// Remove all installed components
    Uninstall {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        install_root: PathBuf,
        /// Skip the confirmation prompt
        #[arg(long)]
        yes: bool,
        /// Report what would be removed without deleting anything
        #[arg(long)]
        dry_run: bool,
    },
```

And add to the `match cli.command` block:
```rust
        Commands::Uninstall { manifest, install_root, yes, dry_run } => {
            commands::uninstall::run(&manifest, &install_root, yes, dry_run)
        }
```

- [x] **Step 5: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS. Note: the third test (`uninstall_without_yes_or_a_tty_fails_clearly_rather_than_hanging`) relies on `assert_cmd`'s subprocess having no TTY attached by default (true for CI and for `Command::cargo_bin` in general) — `is_terminal()` returns `false`, so the bail path triggers instead of the interactive prompt blocking the test.

- [x] **Step 6: Commit**

```bash
git add crates/mlai-cli
git commit -m "feat(mlai-cli): add uninstall command with confirmation and --dry-run"
```

---

### Task 6: Apply removals during install, and persist `manifest_version`

**Files:**
- Modify: `crates/mlai-core/src/pipeline.rs`

**Interfaces:**
- Consumes: `mlai_core::removals::apply_removals` (Task 3), `mlai_core::manifest::Manifest` (for `removals` and `manifest_version`).
- Produces: `install_component`'s signature changes to accept the full `Manifest` (for its `removals` and `manifest_version`) rather than only a single `Component` — see Step 3 for the exact new signature. `InstalledState.manifest_version` is now written on every successful install.

- [x] **Step 1: Write the failing test**

Add to `crates/mlai-core/src/pipeline.rs`'s existing `#[cfg(test)] mod tests` block:
```rust
    #[test]
    fn removals_older_than_the_manifest_are_applied_on_reinstall() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        let zip_path = fixture_dir.path().join("bundle.zip");
        build_fixture_zip(&zip_path);

        let component = sample_component();
        let fetcher = FixtureFetcher { zip_path };

        // First install at manifest_version "1.0.0" — leaves a legacy file behind
        // that a later manifest version will mark for removal.
        let manifest_v1 = Manifest {
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
        };
        install_component(&component, &manifest_v1, &opts).unwrap();
        fs::write(
            root.path().join("hello-component").join("legacy_tool.py"),
            "old",
        )
        .unwrap();

        // Second install at manifest_version "1.1.0", which declares that file
        // deprecated as of 1.1.0 — must be removed by this install.
        let manifest_v2 = Manifest {
            manifest_version: "1.1.0".into(),
            components: vec![component.clone()],
            removals: vec![mlai_core_removal_entry("1.1.0", "hello-component/legacy_tool.py")],
        };
        install_component(&component, &manifest_v2, &opts).unwrap();

        assert!(!root
            .path()
            .join("hello-component")
            .join("legacy_tool.py")
            .exists());

        let state = InstalledState::load(root.path()).unwrap();
        assert_eq!(state.manifest_version, "1.1.0");
    }

    fn mlai_core_removal_entry(version: &str, path: &str) -> crate::manifest::RemovalEntry {
        crate::manifest::RemovalEntry {
            version: version.to_string(),
            paths: vec![path.to_string()],
        }
    }
```

Also update this test module's existing tests (`installs_a_component_end_to_end_and_records_healthy_state`, `skips_reinstall_when_already_healthy_at_same_version`, `backs_up_existing_install_before_replacing_it`, `set_options_are_appended_as_set_flags_to_setup`) to pass a `Manifest` as `install_component`'s new second argument. In each test, insert **one** `manifest` binding (declared once per test, right after `let component = sample_component();` or equivalent, reused across every `install_component` call in that test — `backs_up_existing_install_before_replacing_it` has two calls, both against the same manifest, since only the component's `version` changes between them, not the manifest's):
```rust
        let manifest = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![component.clone()],
            removals: vec![],
        };
```
and change every `install_component(&component, &opts)` call (and `&opts_v1`, `&opts_v2` in `backs_up_existing_install_before_replacing_it`) to `install_component(&component, &manifest, &opts)`.

Add `Manifest` to this test module's imports:
```rust
    use crate::manifest::{Component, HealthCheck, Manifest, PlatformFlag, PlatformHealth, PlatformSetup, SetupCommand};
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test pipeline::`
Expected: FAIL to compile — `install_component` doesn't accept a `Manifest` argument yet.

- [x] **Step 3: Update `install_component`'s signature and body**

In `crates/mlai-core/src/pipeline.rs`, change the function signature:
```rust
pub fn install_component(
    component: &Component,
    manifest: &crate::manifest::Manifest,
    opts: &PipelineOptions,
) -> Result<ComponentState, PipelineError> {
```

Insert this block immediately after `let mut state = InstalledState::load(&opts.install_root)?;` and **before** the existing short-circuit block (the `if let Some(existing) = state.components.get(...)` check that returns early when already healthy). This ordering matters for two reasons the test in Step 1 exercises directly: removals must run even when this call's own component is already healthy and would otherwise short-circuit (the manifest version can advance, carrying new removals, independently of any single component needing reinstall) — and `state.manifest_version` must be persisted unconditionally too, before any early return, or a short-circuited call would silently leave the old `manifest_version` on disk forever.
```rust
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
```

`record_state` itself needs no changes — leave its signature and the four existing call sites exactly as they are.

- [x] **Step 4: Fix the now-broken call site in mlai-cli**

In `crates/mlai-cli/src/commands/install.rs`, change:
```rust
        let result = install_component(component, &opts)
```
to:
```rust
        let result = install_component(component, &manifest, &opts)
```

- [x] **Step 5: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS — all tests, including the new removals-on-reinstall test.

- [x] **Step 6: Commit**

```bash
git add crates/mlai-core/src/pipeline.rs crates/mlai-cli/src/commands/install.rs
git commit -m "feat(mlai-core): apply guarded removals during install, persist manifest_version"
```

---

### Task 7: Docs + final verification

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
Expected: all four PASS. Fix any findings before proceeding.

- [x] **Step 2: Update `docs/USAGE.md`**

Replace the manifest-format code block's `[components.setup]`/`[components.health]` sections with the per-platform shape, and add an "Uninstalling" section plus a "Removals (legacy cleanup)" section. Replace the existing manifest format block:
```markdown
## Manifest format

```toml
manifest_version = "1.0.0"

[[components]]
name = "hello-component"
source_url = "https://example.com/hello-component.zip"
ref = "main"
default = true

[components.setup.posix]
command = "setup.sh"
args = []

[components.health.posix]
type = "file_exists"
path = "marker.txt"
```

- `source_url` — a direct HTTPS URL to a zip archive. The archive's single
  top-level folder is renamed to the component's `name` after extraction.
- `ref` — recorded as the installed version; used to detect whether a
  re-run needs to reinstall.
- `setup`/`health`/`supports_options_protocol` are per-platform (`posix`/
  `windows`) — `mlai` picks the entry matching the OS it's running on. A
  component with no entry for the current OS simply has no setup/health/
  options step on that platform. A component with no `health` block at all
  is always considered healthy once setup succeeds.

## Removals (legacy cleanup)

A manifest can declare `[[removals]]` entries — paths to delete once an
install crosses a given `manifest_version`:

```toml
[[removals]]
version = "1.1.0"
paths = ["hello-component/legacy_tool.py"]
```

Applied automatically during `mlai install` when the previously-recorded
`manifest_version` is older than an entry's `version`. Every path is
validated to resolve inside the install root before removal — a malformed
or malicious manifest can never delete anything outside it.

## Uninstalling

```bash
mlai uninstall --manifest manifest.toml --install-root ~/my-app
```

Prompts for confirmation unless `--yes` is passed (never prompts when
`--dry-run` is also given — dry-run is always safe to run non-interactively).
Removes every component named in the manifest plus `<install-root>/.mlai-install`.
```

Also update the "Not yet implemented" list — remove `uninstall`:
```markdown
## Not yet implemented

`repair`, `update`, cloud config generation, and the credential-source glue
layer are planned follow-ups — see
`docs/superpowers/specs/2026-08-14-foundation-design.md`.
```

- [x] **Step 3: Commit**

```bash
git add docs/USAGE.md
git commit -m "docs: document per-platform setup, removals, and uninstall"
```

- [x] **Step 4: Final full-workspace verification**

Run: `cargo test --workspace`
Expected: PASS. Approximate total test count: `mlai-core` grows to ~48 (27 from Plan B + 6 new manifest tests + 5 versioning + 14 removals − 4 replaced pipeline-signature tests re-passing + 1 new removals-on-reinstall test — exact count isn't load-bearing, all green is), `mlai-credentials` unchanged at 9, `mlai-cli` grows by 3 (`uninstall.rs`).

---

## Self-Review Notes

- **Spec coverage**: per-platform setup/health (the real gap found comparing against a prior-art installer) and guarded removals/uninstall are both covered, ported closely from proven, tested source rather than re-derived. Repair mode and `mlai update` are explicitly out of scope (see "Out of scope" above), not gaps.
- **Placeholder scan**: no TBD/TODO markers; every step has complete, runnable code, including every existing test/call site broken by Task 1's and Task 6's signature changes.
- **Type consistency**: `PlatformSetup`/`PlatformHealth`/`PlatformFlag`, `RemovalEntry`, `compare_version`, `safe_target`/`apply_removals`/`clean_install`/`remove_orphaned_components`, and `install_component`'s new `(component, manifest, opts)` signature are each defined once and consumed identically everywhere they're used afterward.
