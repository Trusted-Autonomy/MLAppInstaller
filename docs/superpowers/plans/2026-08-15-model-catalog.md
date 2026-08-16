# Model Catalog Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The first piece of the distribution-packaging framework: a model catalog mechanism in `mlai-core` that lets multiple independently-developed sub-projects each contribute hardware→model tier data for purposes they own, merged with hard-error conflict detection rather than a single central file or silent coalescing — generalizing cinepipe's proven mechanism (not its data) for a world with no central authority.

**Architecture:** New `mlai-core::catalog` module: `HardwareProfile` (OS, GPU vendor, raw + effective VRAM, disk), `ModelTier` (min VRAM plus optional vendor/OS constraints), `Purpose` (owner + tiers), `CatalogFragment` (one TOML file, parsed), a merge function that loads N fragments and either produces one `MergedCatalog` or a named conflict error, and `MergedCatalog::resolve` (purpose + profile → best-fit model). A `mlai catalog resolve` CLI command exposes this to any setup script regardless of language.

**Tech Stack:** No new dependencies (`serde`/`toml`/`thiserror` already in `mlai-core`).

## Global Constraints

- Conflict detection is a hard error naming both fragments' owners, never a silent pick or coalesce (`docs/superpowers/specs/2026-08-15-distribution-packaging-framework-design.md`, Decision 3 — this is the mechanism that prevents the fragmentation bug cinepipe already hit once).
- Real hardware auto-detection is out of scope — `resolve` takes a `HardwareProfile` as a plain input value; how it's produced isn't this plan's concern (design doc, "Explicitly out of scope").
- Before every commit: `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` all pass on all three CI platforms.

## Out of scope for this plan

- `mlai-package` (the distribution-profile/packaging crate) and `mlai init` — separate, later plans that consume this catalog mechanism.
- Real hardware detection (`nvidia-smi`/Metal/WMI parsing).

---

### Task 1: Catalog types + fragment parsing

**Files:**
- Create: `crates/mlai-core/src/catalog.rs`
- Modify: `crates/mlai-core/src/lib.rs`

**Interfaces:**
- Produces: `mlai_core::catalog::{Os, GpuVendor, HardwareProfile, ModelTier, Purpose, CatalogFragment, CatalogError}`. `CatalogFragment::parse(toml_str: &str) -> Result<CatalogFragment, CatalogError>`.

- [x] **Step 1: Write the failing test**

`crates/mlai-core/src/catalog.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[purposes.text-structured-json]
owner = "cinepipe-stories"

[[purposes.text-structured-json.tiers]]
min_vram_gb = 24
model = "qwen3:32b"

[[purposes.text-structured-json.tiers]]
min_vram_gb = 8
model = "qwen3:8b"
notes = "recommended baseline"

[purposes.voice-transcription]
owner = "trusted-autonomy"

[[purposes.voice-transcription.tiers]]
min_vram_gb = 0
model = "parakeet-mlx"
requires_vendor = ["apple"]
requires_os = ["macos"]
"#;

    #[test]
    fn parses_purposes_with_tiers_and_constraints() {
        let fragment = CatalogFragment::parse(SAMPLE).unwrap();
        assert_eq!(fragment.purposes.len(), 2);

        let text_json = &fragment.purposes["text-structured-json"];
        assert_eq!(text_json.owner, "cinepipe-stories");
        assert_eq!(text_json.tiers.len(), 2);
        assert_eq!(text_json.tiers[0].min_vram_gb, 24.0);
        assert_eq!(text_json.tiers[0].model, "qwen3:32b");
        assert_eq!(text_json.tiers[1].notes, "recommended baseline");

        let voice = &fragment.purposes["voice-transcription"];
        assert_eq!(voice.tiers[0].requires_vendor, vec![GpuVendor::Apple]);
        assert_eq!(voice.tiers[0].requires_os, vec![Os::Macos]);
    }

    #[test]
    fn tiers_default_to_no_constraints_and_empty_notes() {
        let toml = r#"
[purposes.simple]
owner = "example"

[[purposes.simple.tiers]]
min_vram_gb = 4
model = "small-model"
"#;
        let fragment = CatalogFragment::parse(toml).unwrap();
        let tier = &fragment.purposes["simple"].tiers[0];
        assert_eq!(tier.notes, "");
        assert!(tier.requires_vendor.is_empty());
        assert!(tier.requires_os.is_empty());
    }

    #[test]
    fn a_purpose_with_no_tiers_is_a_valid_reference() {
        let toml = r#"
[purposes.text-structured-json]
owner = "cinepipe-stories"
"#;
        let fragment = CatalogFragment::parse(toml).unwrap();
        assert!(fragment.purposes["text-structured-json"].tiers.is_empty());
    }

    #[test]
    fn rejects_invalid_toml() {
        let err = CatalogFragment::parse("not valid toml [[[").unwrap_err();
        assert!(matches!(err, CatalogError::Parse(_)));
    }
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test catalog::`
Expected: FAIL to compile — module `catalog` doesn't exist yet.

- [x] **Step 3: Write the implementation**

Prepend to the top of `crates/mlai-core/src/catalog.rs`:
```rust
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Windows,
    Macos,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Apple,
    Intel,
    None,
}

/// Hardware capability, as an input to `MergedCatalog::resolve` — this
/// project does not detect any of these values itself; see this plan's
/// "Out of scope."
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HardwareProfile {
    pub os: Os,
    pub gpu_vendor: GpuVendor,
    /// Raw detected VRAM.
    pub vram_gb: f64,
    /// VRAM after platform-specific derating (e.g. Apple unified-memory
    /// derating) — this is what `resolve` actually compares tiers against.
    pub effective_vram_gb: f64,
    pub disk_free_gb: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ModelTier {
    pub min_vram_gb: f64,
    pub model: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub requires_vendor: Vec<GpuVendor>,
    #[serde(default)]
    pub requires_os: Vec<Os>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Purpose {
    pub owner: String,
    #[serde(default)]
    pub tiers: Vec<ModelTier>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
pub struct CatalogFragment {
    #[serde(default)]
    pub purposes: BTreeMap<String, Purpose>,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("failed to parse catalog TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error(
        "purpose '{purpose}' is defined with conflicting tiers by both '{owner_a}' and '{owner_b}' — \
         only the owning fragment may define a purpose's tiers; a non-owner must reference it \
         (declare the purpose with no tiers), not redefine it"
    )]
    Conflict {
        purpose: String,
        owner_a: String,
        owner_b: String,
    },
}

impl CatalogFragment {
    pub fn parse(toml_str: &str) -> Result<CatalogFragment, CatalogError> {
        toml::from_str(toml_str).map_err(CatalogError::from)
    }
}
```

Add to `crates/mlai-core/src/lib.rs`:
```rust
pub mod catalog;
```

- [x] **Step 4: Run test to verify it passes**

Run: `cd crates/mlai-core && cargo test catalog::`
Expected: PASS — 4 tests.

- [x] **Step 5: Commit**

```bash
git add crates/mlai-core/src/catalog.rs crates/mlai-core/src/lib.rs
git commit -m "feat(mlai-core): add model catalog types and fragment parsing"
```

---

### Task 2: Merge with conflict detection

**Files:**
- Modify: `crates/mlai-core/src/catalog.rs`

**Interfaces:**
- Produces: `mlai_core::catalog::{MergedCatalog, merge_fragments}`. `merge_fragments(fragments: &[CatalogFragment]) -> Result<MergedCatalog, CatalogError>`.

- [x] **Step 1: Write the failing test**

Add to `crates/mlai-core/src/catalog.rs`'s test module:
```rust
    fn purpose(owner: &str, tiers: Vec<ModelTier>) -> Purpose {
        Purpose { owner: owner.to_string(), tiers }
    }

    fn tier(min_vram_gb: f64, model: &str) -> ModelTier {
        ModelTier {
            min_vram_gb,
            model: model.to_string(),
            notes: String::new(),
            requires_vendor: vec![],
            requires_os: vec![],
        }
    }

    #[test]
    fn merges_purposes_from_multiple_fragments_that_dont_overlap() {
        let mut a = CatalogFragment::default();
        a.purposes.insert("text-structured-json".into(), purpose("cinepipe-stories", vec![tier(8.0, "qwen3:8b")]));
        let mut b = CatalogFragment::default();
        b.purposes.insert("voice-transcription".into(), purpose("trusted-autonomy", vec![tier(0.0, "parakeet-mlx")]));

        let merged = merge_fragments(&[a, b]).unwrap();
        assert!(merged.resolve("text-structured-json", &profile(8.0), 0.0).is_some());
        assert!(merged.resolve("voice-transcription", &profile(0.0), 0.0).is_some());
    }

    #[test]
    fn a_reference_fragment_with_no_tiers_does_not_conflict_with_the_owner() {
        let mut owner_fragment = CatalogFragment::default();
        owner_fragment
            .purposes
            .insert("text-structured-json".into(), purpose("cinepipe-stories", vec![tier(8.0, "qwen3:8b")]));
        let mut reference_fragment = CatalogFragment::default();
        reference_fragment
            .purposes
            .insert("text-structured-json".into(), purpose("cinepipe-stories", vec![]));

        let merged = merge_fragments(&[owner_fragment, reference_fragment]).unwrap();
        assert_eq!(
            merged.resolve("text-structured-json", &profile(8.0), 0.0),
            Some("qwen3:8b")
        );
    }

    #[test]
    fn two_fragments_defining_the_same_purpose_identically_is_not_a_conflict() {
        let mut a = CatalogFragment::default();
        a.purposes.insert("text-structured-json".into(), purpose("cinepipe-stories", vec![tier(8.0, "qwen3:8b")]));
        let b = a.clone();

        let merged = merge_fragments(&[a, b]).unwrap();
        assert_eq!(
            merged.resolve("text-structured-json", &profile(8.0), 0.0),
            Some("qwen3:8b")
        );
    }

    #[test]
    fn two_fragments_disagreeing_on_the_same_purpose_is_a_hard_error() {
        let mut a = CatalogFragment::default();
        a.purposes.insert("text-structured-json".into(), purpose("cinepipe-stories", vec![tier(8.0, "qwen3:8b")]));
        let mut b = CatalogFragment::default();
        b.purposes.insert("text-structured-json".into(), purpose("cinepipe-director", vec![tier(8.0, "llama3:8b")]));

        let err = merge_fragments(&[a, b]).unwrap_err();
        match err {
            CatalogError::Conflict { purpose, owner_a, owner_b } => {
                assert_eq!(purpose, "text-structured-json");
                assert_eq!(owner_a, "cinepipe-stories");
                assert_eq!(owner_b, "cinepipe-director");
            }
            other => panic!("expected Conflict, got {other:?}"),
        }
    }

    #[test]
    fn different_owners_for_the_same_purpose_is_a_hard_error_even_with_no_tiers() {
        let mut a = CatalogFragment::default();
        a.purposes.insert("text-structured-json".into(), purpose("cinepipe-stories", vec![]));
        let mut b = CatalogFragment::default();
        b.purposes.insert("text-structured-json".into(), purpose("cinepipe-director", vec![]));

        let err = merge_fragments(&[a, b]).unwrap_err();
        assert!(matches!(err, CatalogError::Conflict { .. }));
    }
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test catalog::`
Expected: FAIL to compile — `merge_fragments`, `MergedCatalog`, and the test helper's use of `profile()` (added in Task 3) don't exist yet. `profile()` is referenced here but implemented in Task 3 — for this step, temporarily stub it inline at the bottom of this task's test additions:
```rust
    fn profile(effective_vram_gb: f64) -> HardwareProfile {
        HardwareProfile {
            os: Os::Linux,
            gpu_vendor: GpuVendor::Nvidia,
            vram_gb: effective_vram_gb,
            effective_vram_gb,
            disk_free_gb: 100.0,
        }
    }
```
(This stub is genuinely needed by Task 2's tests and is not redundant with Task 3 — Task 3 doesn't redefine it, it reuses this one.)

- [x] **Step 3: Write the implementation**

Add to `crates/mlai-core/src/catalog.rs`, after `CatalogFragment`'s `impl` block:
```rust
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MergedCatalog {
    purposes: BTreeMap<String, Purpose>,
}

/// Merges catalog fragments from multiple independently-developed
/// sub-projects. A purpose declared identically (same owner, same tiers —
/// or one side merely referencing it with no tiers) by more than one
/// fragment is fine. A purpose declared with *different* tiers, or a
/// *different* owner, by more than one fragment is a hard error — this is
/// the mechanism that prevents the exact fragmentation bug this design
/// exists to stop (two products independently inventing different tier
/// tables for what should be one shared decision).
pub fn merge_fragments(fragments: &[CatalogFragment]) -> Result<MergedCatalog, CatalogError> {
    let mut merged: BTreeMap<String, Purpose> = BTreeMap::new();
    for fragment in fragments {
        for (name, purpose) in &fragment.purposes {
            match merged.get(name) {
                None => {
                    merged.insert(name.clone(), purpose.clone());
                }
                Some(existing) => {
                    if existing.owner != purpose.owner {
                        return Err(CatalogError::Conflict {
                            purpose: name.clone(),
                            owner_a: existing.owner.clone(),
                            owner_b: purpose.owner.clone(),
                        });
                    }
                    match (existing.tiers.is_empty(), purpose.tiers.is_empty()) {
                        (true, false) => {
                            merged.insert(name.clone(), purpose.clone());
                        }
                        (false, false) if existing.tiers != purpose.tiers => {
                            return Err(CatalogError::Conflict {
                                purpose: name.clone(),
                                owner_a: existing.owner.clone(),
                                owner_b: purpose.owner.clone(),
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    Ok(MergedCatalog { purposes: merged })
}
```

- [x] **Step 4: Run test to verify it passes**

Run: `cd crates/mlai-core && cargo test catalog::`
Expected: still FAILS to compile at this point — `MergedCatalog::resolve` (used by every test in this task) doesn't exist yet. That's expected; Task 3 implements it. Do not skip ahead — commit only after Task 3 makes the full suite pass, per Step 5 below being deferred.

- [x] **Step 5: Commit (after Task 3, not before)**

This task's commit is folded into Task 3's — `resolve` and `merge_fragments` are tested together and neither compiles alone. Proceed directly to Task 3.

---

### Task 3: Tier resolution

**Files:**
- Modify: `crates/mlai-core/src/catalog.rs`

**Interfaces:**
- Produces: `MergedCatalog::resolve(&self, purpose: &str, profile: &HardwareProfile, reserve_vram_gb: f64) -> Option<&str>`.

- [x] **Step 1: Add the `profile()` test helper and resolution-specific tests**

Add to `crates/mlai-core/src/catalog.rs`'s test module (the `profile()` helper from Task 2's Step 2 should already be present — if it isn't yet, add it now exactly as shown there):
```rust
    #[test]
    fn resolve_picks_the_highest_tier_the_profile_qualifies_for() {
        let mut a = CatalogFragment::default();
        a.purposes.insert(
            "text-structured-json".into(),
            purpose(
                "cinepipe-stories",
                vec![tier(24.0, "qwen3:32b"), tier(8.0, "qwen3:8b"), tier(0.0, "qwen3:4b")],
            ),
        );
        let merged = merge_fragments(&[a]).unwrap();

        assert_eq!(merged.resolve("text-structured-json", &profile(30.0), 0.0), Some("qwen3:32b"));
        assert_eq!(merged.resolve("text-structured-json", &profile(10.0), 0.0), Some("qwen3:8b"));
        assert_eq!(merged.resolve("text-structured-json", &profile(2.0), 0.0), Some("qwen3:4b"));
    }

    #[test]
    fn resolve_returns_none_for_an_unknown_purpose() {
        let merged = merge_fragments(&[]).unwrap();
        assert_eq!(merged.resolve("nonexistent", &profile(100.0), 0.0), None);
    }

    #[test]
    fn resolve_subtracts_the_reservation_before_matching_tiers() {
        let mut a = CatalogFragment::default();
        a.purposes.insert(
            "text-structured-json".into(),
            purpose("cinepipe-stories", vec![tier(24.0, "qwen3:32b"), tier(8.0, "qwen3:8b")]),
        );
        let merged = merge_fragments(&[a]).unwrap();

        // 30GB effective, but 24GB reserved for a co-resident heavy consumer
        // (e.g. Unreal Engine) -- only 6GB usable, below even the 8GB tier.
        assert_eq!(merged.resolve("text-structured-json", &profile(30.0), 24.0), None);
    }

    #[test]
    fn resolve_skips_a_tier_whose_vendor_constraint_is_not_met() {
        let mut a = CatalogFragment::default();
        let mut mlx_tier = tier(0.0, "parakeet-mlx");
        mlx_tier.requires_vendor = vec![GpuVendor::Apple];
        mlx_tier.requires_os = vec![Os::Macos];
        a.purposes.insert("voice-transcription".into(), purpose("trusted-autonomy", vec![mlx_tier]));
        let merged = merge_fragments(&[a]).unwrap();

        let nvidia_linux = HardwareProfile {
            os: Os::Linux,
            gpu_vendor: GpuVendor::Nvidia,
            vram_gb: 24.0,
            effective_vram_gb: 24.0,
            disk_free_gb: 100.0,
        };
        assert_eq!(merged.resolve("voice-transcription", &nvidia_linux, 0.0), None);

        let apple_macos = HardwareProfile {
            os: Os::Macos,
            gpu_vendor: GpuVendor::Apple,
            vram_gb: 16.0,
            effective_vram_gb: 12.0,
            disk_free_gb: 100.0,
        };
        assert_eq!(merged.resolve("voice-transcription", &apple_macos, 0.0), Some("parakeet-mlx"));
    }
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test catalog::`
Expected: FAIL to compile — `MergedCatalog::resolve` doesn't exist yet.

- [x] **Step 3: Write the implementation**

Add to `crates/mlai-core/src/catalog.rs`, after `merge_fragments`:
```rust
impl MergedCatalog {
    /// Resolves the best-fit model for `purpose` given `profile`, after
    /// subtracting `reserve_vram_gb` (headroom for a co-resident heavy GPU
    /// consumer, e.g. Unreal Engine) from `profile.effective_vram_gb`.
    /// Tiers are checked from highest `min_vram_gb` down; a tier is skipped
    /// if the profile doesn't meet its `min_vram_gb` after reservation, or
    /// if it declares `requires_vendor`/`requires_os` constraints the
    /// profile doesn't satisfy.
    pub fn resolve(&self, purpose: &str, profile: &HardwareProfile, reserve_vram_gb: f64) -> Option<&str> {
        let purpose = self.purposes.get(purpose)?;
        let usable_vram = (profile.effective_vram_gb - reserve_vram_gb).max(0.0);

        let mut sorted_tiers: Vec<&ModelTier> = purpose.tiers.iter().collect();
        sorted_tiers.sort_by(|a, b| b.min_vram_gb.partial_cmp(&a.min_vram_gb).unwrap());

        for tier in sorted_tiers {
            if tier.min_vram_gb > usable_vram {
                continue;
            }
            if !tier.requires_vendor.is_empty() && !tier.requires_vendor.contains(&profile.gpu_vendor) {
                continue;
            }
            if !tier.requires_os.is_empty() && !tier.requires_os.contains(&profile.os) {
                continue;
            }
            return Some(&tier.model);
        }
        None
    }
}
```

- [x] **Step 4: Run the full catalog test suite**

Run: `cd crates/mlai-core && cargo test catalog::`
Expected: PASS — all tests from Tasks 1–3 (14 tests).

- [x] **Step 5: Run the full mlai-core suite and commit**

Run: `cd crates/mlai-core && cargo test`
Expected: PASS — all modules green.

```bash
git add crates/mlai-core/src/catalog.rs
git commit -m "feat(mlai-core): add model catalog merge and tier resolution"
```

---

### Task 4: `mlai catalog resolve` CLI command

**Files:**
- Create: `crates/mlai-cli/src/commands/catalog.rs`
- Modify: `crates/mlai-cli/src/commands/mod.rs`
- Modify: `crates/mlai-cli/src/main.rs`
- Create: `crates/mlai-cli/tests/catalog.rs`

**Interfaces:**
- Consumes: `mlai_core::catalog::{CatalogFragment, merge_fragments, HardwareProfile, Os, GpuVendor}` (Tasks 1–3).
- Produces: `mlai catalog resolve --purpose <p> --catalog <path>... --os <os> --gpu-vendor <vendor> --vram-gb <f> --effective-vram-gb <f> --disk-free-gb <f> [--reserve-vram-gb <f>]` — prints the resolved model name to stdout, or exits non-zero with an actionable message if nothing resolves.

- [x] **Step 1: Write the failing integration test**

`crates/mlai-cli/tests/catalog.rs`:
```rust
use assert_cmd::Command;
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

    cmd.assert()
        .failure()
        .stderr(contains("no model in 'text-structured-json' fits this hardware profile"));
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test --workspace`
Expected: FAIL to compile — `crates/mlai-cli/src/commands/catalog.rs` doesn't exist yet, and the CLI has no `catalog` subcommand.

- [x] **Step 3: Write `commands/catalog.rs`**

`crates/mlai-cli/src/commands/catalog.rs`:
```rust
use anyhow::{bail, Context, Result};
use mlai_core::catalog::{merge_fragments, CatalogFragment, GpuVendor, HardwareProfile, Os};
use std::fs;
use std::path::PathBuf;

#[allow(clippy::too_many_arguments)]
pub fn resolve(
    purpose: &str,
    catalog_paths: &[PathBuf],
    os: Os,
    gpu_vendor: GpuVendor,
    vram_gb: f64,
    effective_vram_gb: f64,
    disk_free_gb: f64,
    reserve_vram_gb: f64,
) -> Result<()> {
    let mut fragments = Vec::new();
    for path in catalog_paths {
        let content = fs::read_to_string(path)
            .with_context(|| format!("reading catalog fragment at {}", path.display()))?;
        let fragment = CatalogFragment::parse(&content)
            .with_context(|| format!("parsing catalog fragment at {}", path.display()))?;
        fragments.push(fragment);
    }

    let merged = merge_fragments(&fragments).map_err(|e| anyhow::anyhow!("{e}"))?;
    let profile = HardwareProfile { os, gpu_vendor, vram_gb, effective_vram_gb, disk_free_gb };

    match merged.resolve(purpose, &profile, reserve_vram_gb) {
        Some(model) => {
            println!("{model}");
            Ok(())
        }
        None => bail!(
            "no model in '{purpose}' fits this hardware profile (effective {effective_vram_gb}GB VRAM, \
             {reserve_vram_gb}GB reserved, vendor {gpu_vendor:?}, os {os:?}) — check the catalog's tiers \
             for '{purpose}' and whether any qualify"
        ),
    }
}
```

Modify `crates/mlai-cli/src/commands/mod.rs`:
```rust
pub mod catalog;
pub mod install;
pub mod repair;
pub mod uninstall;
```

- [x] **Step 4: Wire the CLI subcommand**

In `crates/mlai-cli/src/main.rs`, add `Os`/`GpuVendor` to the `use` list (`use mlai_core::manifest::...` stays separate; add a new import), and extend the `Commands` enum:
```rust
use mlai_core::catalog::{GpuVendor, Os};
```
```rust
    /// Resolve the best-fit model for a purpose against a hardware profile
    Catalog {
        #[command(subcommand)]
        action: CatalogAction,
    },
```

```rust
#[derive(Subcommand)]
enum CatalogAction {
    Resolve {
        #[arg(long)]
        purpose: String,
        #[arg(long = "catalog")]
        catalog_paths: Vec<PathBuf>,
        #[arg(long, value_enum)]
        os: CliOs,
        #[arg(long = "gpu-vendor", value_enum)]
        gpu_vendor: CliGpuVendor,
        #[arg(long)]
        vram_gb: f64,
        #[arg(long)]
        effective_vram_gb: f64,
        #[arg(long)]
        disk_free_gb: f64,
        #[arg(long, default_value_t = 0.0)]
        reserve_vram_gb: f64,
    },
}

#[derive(Clone, clap::ValueEnum)]
enum CliOs {
    Windows,
    Macos,
    Linux,
}

impl From<CliOs> for Os {
    fn from(v: CliOs) -> Os {
        match v {
            CliOs::Windows => Os::Windows,
            CliOs::Macos => Os::Macos,
            CliOs::Linux => Os::Linux,
        }
    }
}

#[derive(Clone, clap::ValueEnum)]
enum CliGpuVendor {
    Nvidia,
    Amd,
    Apple,
    Intel,
    None,
}

impl From<CliGpuVendor> for GpuVendor {
    fn from(v: CliGpuVendor) -> GpuVendor {
        match v {
            CliGpuVendor::Nvidia => GpuVendor::Nvidia,
            CliGpuVendor::Amd => GpuVendor::Amd,
            CliGpuVendor::Apple => GpuVendor::Apple,
            CliGpuVendor::Intel => GpuVendor::Intel,
            CliGpuVendor::None => GpuVendor::None,
        }
    }
}
```

Update the `match cli.command` block:
```rust
        Commands::Catalog { action } => match action {
            CatalogAction::Resolve {
                purpose,
                catalog_paths,
                os,
                gpu_vendor,
                vram_gb,
                effective_vram_gb,
                disk_free_gb,
                reserve_vram_gb,
            } => commands::catalog::resolve(
                &purpose,
                &catalog_paths,
                os.into(),
                gpu_vendor.into(),
                vram_gb,
                effective_vram_gb,
                disk_free_gb,
                reserve_vram_gb,
            ),
        },
```

- [x] **Step 5: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS — including the 3 new `mlai-cli` catalog integration tests.

- [x] **Step 6: Commit**

```bash
git add crates/mlai-cli
git commit -m "feat(mlai-cli): add catalog resolve command"
```

---

### Task 5: Docs + final verification

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

Add a new section, after "Backend options protocol":
```markdown
## Model catalog

A component that needs a decision like "which local model fits this
machine" can defer to a shared catalog instead of inventing its own
hardware-tier table. Multiple sub-projects can each contribute a fragment
without a central authority — a purpose declares an `owner`; a fragment
that only *references* a purpose (no `[[tiers]]`) never conflicts, but two
fragments that *define* the same purpose differently is a hard error, not
a silent pick:

```toml
# fragment owned by cinepipe-stories
[purposes.text-structured-json]
owner = "cinepipe-stories"

[[purposes.text-structured-json.tiers]]
min_vram_gb = 24
model = "qwen3:32b"

[[purposes.text-structured-json.tiers]]
min_vram_gb = 8
model = "qwen3:8b"
```

```bash
mlai catalog resolve --purpose text-structured-json \
  --catalog fragment-a.toml --catalog fragment-b.toml \
  --os linux --gpu-vendor nvidia \
  --vram-gb 12 --effective-vram-gb 12 --disk-free-gb 200
```

Prints the resolved model name to stdout, or a clear error if nothing fits
or two catalogs disagree. `mlai` does not detect hardware itself — the
`--os`/`--gpu-vendor`/`--vram-gb`/`--effective-vram-gb`/`--disk-free-gb`
flags are the caller's (a component's own setup script) responsibility to
supply, the same as today.
```

- [x] **Step 3: Commit**

```bash
git add docs/USAGE.md
git commit -m "docs: document the model catalog and catalog resolve command"
```

- [x] **Step 4: Final full-workspace verification**

Run: `cargo test --workspace`
Expected: PASS on the local platform; CI verifies all three once pushed.

---

## Self-Review Notes

- **Spec coverage**: ownership-tagged fragments, merge-with-conflict-detection (both "different tiers" and "different owner" cases), structured hardware profile with vendor/OS tier constraints, and reservation subtraction are all covered per the design doc's Decisions 2–4 and 8. `mlai-package`/`mlai init` (which consume this) are separate, later plans, not gaps here.
- **Placeholder scan**: no TBD/TODO markers; every step has complete, runnable code.
- **Type consistency**: `CatalogFragment`, `Purpose`, `ModelTier`, `HardwareProfile`, `merge_fragments`, and `MergedCatalog::resolve` are each defined once (Tasks 1–3) and consumed identically in Task 4's CLI layer.
