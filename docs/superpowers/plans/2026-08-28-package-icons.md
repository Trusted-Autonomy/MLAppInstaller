# Per-Distribution App Icons Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An adopter can specify real app icon source file(s) in their distribution profile, and `mlai package build` produces a properly-iconed native installer with no image-conversion work of MLAppInstaller's own.

**Architecture:** `mlai-package::profile::Distribution` gains `icons: Vec<String>` (mirroring the existing `components: Vec<String>` field's `#[serde(default)]`-only pattern exactly, for consistency — not a new serialization convention). `packager_config::build_packager_config` passes it straight through to `cargo-packager`'s own `icons` config field, which handles all per-platform icon-format conversion itself (verified directly against the real binary: one source PNG became a valid `.icns`). `mlai init` gains one new optional prompt.

**Tech Stack:** Rust only — no new dependencies.

## Global Constraints

- Every step's exit criteria: `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` must all pass before commit (per `CLAUDE.md`).
- Every existing `DistributionProfile` TOML (no `icons` key) must parse identically to before this change.
- Design reference: `docs/superpowers/specs/2026-08-28-package-icons-design.md`.
- Grounding note: verified directly against the current contents of `crates/mlai-package/src/{profile,packager_config}.rs` and `crates/mlai-cli/src/commands/init.rs`, and against the real locally-installed `cargo-packager` binary (its `icons` config field was tested empirically — a source PNG placed there was auto-converted to a valid `.icns`), on 2026-08-28.

---

### Task 1: `icons` field on `Distribution` + packager config wiring

**Files:**
- Modify: `crates/mlai-package/src/profile.rs`
- Modify: `crates/mlai-package/src/packager_config.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `Distribution.icons: Vec<String>` (new field, consumed by Task 2's `mlai init` and by `build_packager_config`, which this task also updates).

- [ ] **Step 1: Write the failing test for parsing**

Add to `crates/mlai-package/src/profile.rs`'s existing `#[cfg(test)] mod tests` block:

```rust
#[test]
fn icons_parse_when_present() {
    let toml = r#"
[distribution]
name = "example-app"
manifest = "manifest.toml"
icons = ["icon.png"]

[[targets]]
platform = "macos"
format = "dmg"
"#;
    let profile = DistributionProfile::parse(toml).unwrap();
    assert_eq!(profile.distribution.icons, vec!["icon.png".to_string()]);
}

#[test]
fn icons_default_to_empty_when_absent() {
    let profile = sample_profile_for_icons_test();
    assert!(profile.distribution.icons.is_empty());
}
```

Note: `deploy_and_components_are_optional`'s existing `toml` fixture in this file already has no `icons` key and already asserts `profile.distribution.components.is_empty()` — add `assert!(profile.distribution.icons.is_empty());` to that SAME existing test instead of writing a new `sample_profile_for_icons_test()` helper (there is no such helper today; use the existing `deploy_and_components_are_optional` test's fixture directly, either by extending that test or by copying its exact TOML string into `icons_default_to_empty_when_absent` — prefer extending the existing test with one more assertion, since it's already proving "absent fields default correctly" for `components`/`deploy` and `icons` belongs in that same assertion group).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mlai-package icons_parse_when_present`
Expected: FAIL — compile error, `Distribution` has no field `icons`.

- [ ] **Step 3: Add the field**

In `crates/mlai-package/src/profile.rs`, add to `Distribution` (after `components`):

```rust
    #[serde(default)]
    pub icons: Vec<String>,
```

- [ ] **Step 4: Run tests, fix the two known existing struct-literal/fixture sites, verify full suite passes**

Run: `cargo test -p mlai-package`
Expected: initial FAIL on any `Distribution { ... }` struct literal missing the new field (check `crates/mlai-cli/src/commands/init.rs`'s `run_wizard` — it constructs `Distribution { name, manifest, components }` directly and will need `icons` added to that literal too, even though that's Task 2's file — fixing it now, in this step, is fine and necessary to keep the workspace compiling; Task 2 below covers actually *wiring* the wizard's new prompt, this step just keeps the build green by adding `icons: vec![]` as a placeholder value here first). Add `icons: vec![],` to that literal now. Re-run until PASS.

- [ ] **Step 5: Write the failing test for packager config wiring**

Add to `crates/mlai-package/src/packager_config.rs`'s existing `#[cfg(test)] mod tests` block:

```rust
#[test]
fn includes_icons_when_present() {
    let mut profile = sample_profile();
    profile.distribution.icons = vec!["icon.png".to_string()];
    let target = &profile.targets[0];
    let json = build_packager_config(&profile, target, "bin/hello-app");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["icons"], serde_json::json!(["icon.png"]));
}

#[test]
fn omits_icons_key_when_none_configured() {
    let profile = sample_profile();
    let target = &profile.targets[0];
    let json = build_packager_config(&profile, target, "bin/hello-app");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert!(value.get("icons").is_none());
}
```

- [ ] **Step 6: Run tests to verify they fail**

Run: `cargo test -p mlai-package includes_icons_when_present omits_icons_key_when_none_configured`
Expected: `includes_icons_when_present` FAILs (`value["icons"]` is `Value::Null`); `omits_icons_key_when_none_configured` currently PASSes trivially (there's no `icons` key emitted at all yet) — that's fine, it becomes a real regression guard once Step 7 adds the field with `skip_serializing_if`.

- [ ] **Step 7: Add the field to `PackagerConfig`**

In `crates/mlai-package/src/packager_config.rs`, add to the `PackagerConfig` struct (after `resources`):

```rust
    #[serde(skip_serializing_if = "Vec::is_empty")]
    icons: Vec<String>,
```

In `build_packager_config`, add to the `PackagerConfig { ... }` literal:

```rust
        icons: profile.distribution.icons.clone(),
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p mlai-package includes_icons_when_present omits_icons_key_when_none_configured`
Expected: both PASS.

- [ ] **Step 9: Run the crate's full suite, lint, format, and commit**

Run: `cargo test -p mlai-package && cargo clippy -p mlai-package --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: all clean.

```bash
git add crates/mlai-package/src/profile.rs crates/mlai-package/src/packager_config.rs
git commit -m "feat(mlai-package): pass distribution icons through to cargo-packager"
```

---

### Task 2: `mlai init` icon prompt

**Files:**
- Modify: `crates/mlai-cli/src/commands/init.rs`

**Interfaces:**
- Consumes: `Distribution.icons: Vec<String>` (Task 1).
- Produces: nothing new for other tasks — this is the plan's final task.

- [ ] **Step 1: Write the failing test**

Add to `crates/mlai-cli/src/commands/init.rs`'s existing `#[cfg(test)] mod tests` block:

```rust
#[test]
fn icons_are_comma_split_like_components() {
    let input = "\
my-app\n\
\n\
\n\
linux\n\
\n\
\n\
\n\
icon.png,icon@2x.png\n\
n\n\
";
    let profile = run_with(input).expect("wizard should succeed");
    assert_eq!(
        profile.distribution.icons,
        vec!["icon.png".to_string(), "icon@2x.png".to_string()]
    );
}

#[test]
fn blank_icons_answer_produces_empty_vec() {
    let input = "\
my-app\n\
\n\
\n\
linux\n\
\n\
\n\
\n\
\n\
n\n\
";
    let profile = run_with(input).expect("wizard should succeed");
    assert!(profile.distribution.icons.is_empty());
}
```

Every existing test in this file that answers the wizard's full prompt sequence (e.g. `blank_distribution_name_reprompts_and_uses_second_answer`, `components_with_empty_entries_are_filtered_out`, `blank_deploy_repo_reprompts_and_uses_second_answer`) will need one additional blank-line answer inserted at the point the new icon prompt falls in the sequence (after certificate thumbprint, before "Configure a deploy target?") — fix each of those existing tests' `input` strings in this same step, not as an afterthought, since they'll fail to reach their own assertions otherwise (the deploy-target prompt would consume what used to be the answer meant for it).

- [ ] **Step 2: Run tests to verify the new ones fail and existing ones that need fixing do fail**

Run: `cargo test -p mlai-cli icons_are_comma_split_like_components blank_icons_answer_produces_empty_vec`
Expected: FAIL (no icon prompt exists yet, so the extra blank-line answers in every fixed test's input are currently being consumed by the wrong prompt — new tests fail outright; run the full `cargo test -p mlai-cli init::` module too and confirm exactly which pre-existing tests are now broken by the input-string edits from Step 1, so Step 3's fix is verified against a known-bad baseline).

- [ ] **Step 3: Add the prompt**

In `crates/mlai-cli/src/commands/init.rs`'s `run_wizard`, insert after the certificate-thumbprint block and before the `"Configure a deploy target? [y/N]: "` prompt:

```rust
    prompt(
        writer,
        "Icon file(s) (comma-separated paths, blank = none): ",
    )?;
    let icons_answer = read_answer(reader)?;
    let icons: Vec<String> = if icons_answer.is_empty() {
        vec![]
    } else {
        icons_answer
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };
```

Replace the placeholder `icons: vec![],` in the final `Distribution { ... }` literal (added in Task 1 Step 4 just to keep the build green) with `icons,`.

- [ ] **Step 4: Run the full init test module to verify everything passes**

Run: `cargo test -p mlai-cli init::`
Expected: all PASS, including every pre-existing test whose input string was extended in Step 1.

- [ ] **Step 5: Run the full mlai-cli suite, lint, format, and commit**

Run: `cargo test -p mlai-cli && cargo clippy -p mlai-cli --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: all clean.

```bash
git add crates/mlai-cli/src/commands/init.rs
git commit -m "feat(mlai-cli): prompt for icon file(s) in mlai init"
```

- [ ] **Step 6: Update `docs/USAGE.md`**

Add a short note to the existing `mlai init`/distribution-profile documentation section about the new `icons` field: what it's for, that it's a comma-separated list of source image paths, and that `cargo-packager` (not MLAppInstaller) handles per-platform format conversion.

```bash
git add docs/USAGE.md
git commit -m "docs: document distribution profile icons field"
```

- [ ] **Step 7: Run the full workspace suite one more time**

Run: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: all clean.

- [ ] **Step 8: Mark this plan's checkboxes complete**

Update this file (`docs/superpowers/plans/2026-08-28-package-icons.md`), checking off every completed step, then commit:

```bash
git add docs/superpowers/plans/2026-08-28-package-icons.md
git commit -m "docs: mark package-icons plan complete"
```
