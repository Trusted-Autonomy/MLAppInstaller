# GUI White-Labeling (No-Fork Distribution) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An adopter can produce a fully branded `mlai-gui` installer (their own components, their own window title) using the stock, already-compiled `mlai-gui` binary via `mlai package build` — no second checkout/fork of the `mlai-gui` crate required.

**Architecture:** `mlai-package::packager_config::build_packager_config` gains a `resources` field in the generated `cargo-packager` JSON config, populated from `DistributionProfile.distribution.manifest` — `cargo-packager` places that file at the platform's standard app-resource path (verified directly: macOS lands it at `<App>.app/Contents/Resources/`), which is exactly what `mlai-gui`'s existing `find_resource()`/`resource_dir()` lookup already checks at runtime, so no `mlai-gui` Rust changes are needed for this half. Separately, `mlai-core::manifest::GuiConfig` gains an `app_name: Option<String>` field (same `[gui]` table the theme feature added), and `mlai-gui`'s frontend retitles the window at startup via Tauri's runtime window API when it's set.

**Tech Stack:** Rust (`mlai-package`, `mlai-core` — no new crate dependencies), TypeScript (`mlai-gui`'s `main.ts` — uses `@tauri-apps/api/window`, already available via the existing `@tauri-apps/api` dependency, no new npm package).

## Global Constraints

- Every step's exit criteria: `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` must all pass before commit (per `CLAUDE.md`).
- Every existing manifest (no `app_name` in `[gui]`) must behave identically to before this change — the window keeps whatever title `mlai-gui`'s own compiled-in `tauri.conf.json` provides.
- Every existing `DistributionProfile` (already has `distribution.manifest` — this plan doesn't add a new profile field) must produce the same packager invocation shape as before, plus the new `resources` entry.
- Design reference: `docs/superpowers/specs/2026-08-19-gui-whitelabel-design.md`.
- Grounding note: verified directly against the current contents of `crates/mlai-package/src/{packager_config,build,profile}.rs`, `crates/mlai-core/src/manifest.rs`, `crates/mlai-gui/src/main.ts`, and against the real locally-installed `cargo-packager` 0.11.8 binary (a `resources` config field was tested empirically, not assumed) on 2026-08-19.

---

### Task 1: `mlai-package` bundles the adopter's manifest automatically

**Files:**
- Modify: `crates/mlai-package/src/packager_config.rs`

**Interfaces:**
- Consumes: `mlai_package::profile::DistributionProfile` (existing — `Distribution.manifest: String`, already parsed, currently unused by `packager_config.rs`).
- Produces: no new public function — `build_packager_config`'s existing signature (`profile: &DistributionProfile, target: &Target, binary_path: &str) -> String`) is unchanged; only its output JSON gains a `resources` key. `build.rs`'s `packager_command`/`build_package` call this unchanged, so Task 1 requires no `build.rs` edit.

- [ ] **Step 1: Write the failing test**

Add to `crates/mlai-package/src/packager_config.rs`'s existing `#[cfg(test)] mod tests` block:

```rust
#[test]
fn includes_the_distribution_manifest_as_a_resource() {
    let profile = sample_profile();
    let target = &profile.targets[0];
    let json = build_packager_config(&profile, target, "bin/hello-app");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["resources"], serde_json::json!(["manifest.toml"]));
}
```

(`sample_profile()`'s existing fixture already parses `manifest = "manifest.toml"` in its `[distribution]` table — no fixture change needed.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mlai-package includes_the_distribution_manifest_as_a_resource`
Expected: FAIL — `value["resources"]` is `Value::Null` (the key doesn't exist yet), not equal to the expected array.

- [ ] **Step 3: Add the `resources` field**

In `crates/mlai-package/src/packager_config.rs`, add to the `PackagerConfig` struct (after `windows`):

```rust
    #[serde(skip_serializing_if = "Vec::is_empty")]
    resources: Vec<String>,
```

In `build_packager_config`, add to the `PackagerConfig { ... }` literal:

```rust
        resources: vec![profile.distribution.manifest.clone()],
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p mlai-package includes_the_distribution_manifest_as_a_resource`
Expected: PASS.

- [ ] **Step 5: Run the crate's full test suite (confirm no existing test broke)**

Run: `cargo test -p mlai-package`
Expected: all PASS — none of the existing tests in this file assert on the full JSON shape in a way `resources`'s addition would break (they assert on individual keys like `value["productName"]`, `value["macos"]["signingIdentity"]`, etc., not the whole object), but verify this rather than assume it.

- [ ] **Step 6: Lint, format, and commit**

Run: `cargo clippy -p mlai-package --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: both clean.

```bash
git add crates/mlai-package/src/packager_config.rs
git commit -m "feat(mlai-package): bundle the distribution's manifest.toml as a packaged resource"
```

---

### Task 2: `[gui] app_name` manifest field + runtime window retitle

**Files:**
- Modify: `crates/mlai-core/src/manifest.rs`
- Modify: `crates/mlai-gui/src/main.ts`

**Interfaces:**
- Consumes: nothing new from Task 1 (independent field addition to the same `GuiConfig` struct the theme feature already added).
- Produces: `GuiConfig.app_name: Option<String>` (new field, JSON key `app_name`, consumed only by `mlai-gui`'s frontend — no other Rust code in this plan reads it). No other task depends on this.

- [ ] **Step 1: Write the failing test**

Add to `crates/mlai-core/src/manifest.rs`'s existing `#[cfg(test)] mod tests` block:

```rust
#[test]
fn gui_app_name_parses_when_present() {
    let toml = r#"
        manifest_version = "1.0.0"

        [[components]]
        name = "hello-component"
        source_url = "https://example.com/hello-component.zip"
        ref = "main"

        [gui]
        app_name = "Example Studio Installer"
    "#;
    let manifest = Manifest::parse(toml).unwrap();
    assert_eq!(
        manifest.gui.app_name.as_deref(),
        Some("Example Studio Installer")
    );
}

#[test]
fn gui_app_name_defaults_to_none_when_absent() {
    let manifest = Manifest::parse(SAMPLE).unwrap();
    assert_eq!(manifest.gui.app_name, None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mlai-core gui_app_name_parses_when_present`
Expected: FAIL — compile error, `GuiConfig` has no field `app_name`.

- [ ] **Step 3: Add the field**

In `crates/mlai-core/src/manifest.rs`, add to `GuiConfig` (after `theme`):

```rust
    #[serde(default)]
    pub app_name: Option<String>,
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p mlai-core gui_app_name_parses_when_present gui_app_name_defaults_to_none_when_absent`
Expected: PASS.

- [ ] **Step 5: Run the full mlai-core suite (confirm existing `Manifest { ... }` fixtures still compile)**

Run: `cargo test -p mlai-core`
Expected: PASS — `app_name` is `Option<String>` inside `GuiConfig`, and every existing `Manifest { ... }` struct literal already constructs `gui: GuiConfig::default()` (added when the `theme` field landed), so `GuiConfig::default()` already covers the new field via `#[derive(Default)]` — no fixture updates needed this time, unlike the `theme` field's own introduction. Verify this is actually true by running the suite rather than assuming it.

- [ ] **Step 6: Lint, format, and commit**

Run: `cargo clippy -p mlai-core --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: both clean.

```bash
git add crates/mlai-core/src/manifest.rs
git commit -m "feat(mlai-core): add app_name to the [gui] manifest table"
```

- [ ] **Step 7: Extend the frontend `Manifest` interface**

In `crates/mlai-gui/src/main.ts`, change:

```typescript
interface Manifest {
  manifest_version: string;
  components: Component[];
  gui: {
    theme: "system" | "light" | "dark";
  };
}
```

to:

```typescript
interface Manifest {
  manifest_version: string;
  components: Component[];
  gui: {
    theme: "system" | "light" | "dark";
    app_name: string | null;
  };
}
```

- [ ] **Step 8: Apply the retitle at startup**

In `crates/mlai-gui/src/main.ts`, add the import alongside the existing `@tauri-apps/api/core` and `@tauri-apps/api/event` imports:

```typescript
import { getCurrentWindow } from "@tauri-apps/api/window";
```

In `loadComponents()`, change:

```typescript
    const manifest = await invoke<Manifest>("list_components");
    currentManifest = manifest;
    if (manifest.gui.theme !== "system") {
      document.documentElement.dataset.theme = manifest.gui.theme;
    }
    renderComponents(manifest);
```

to:

```typescript
    const manifest = await invoke<Manifest>("list_components");
    currentManifest = manifest;
    if (manifest.gui.theme !== "system") {
      document.documentElement.dataset.theme = manifest.gui.theme;
    }
    if (manifest.gui.app_name) {
      await getCurrentWindow().setTitle(manifest.gui.app_name);
    }
    renderComponents(manifest);
```

- [ ] **Step 9: Type-check the frontend**

Run: `cd crates/mlai-gui && npm install && ./node_modules/.bin/tsc --noEmit`
Expected: no output, exit code 0.

- [ ] **Step 10: Run the full workspace suite one more time and commit**

Run: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: all clean (Task 1 and Task 2 touch disjoint files but this confirms they coexist).

```bash
git add crates/mlai-gui/src/main.ts
git commit -m "feat(mlai-gui): retitle the window from manifest.toml's [gui] app_name"
```

- [ ] **Step 11: Update `docs/USAGE.md`**

Add to the existing "Theme (`[gui]` table)" section (from the theme feature) — rename/expand it to cover both fields, e.g. "Presentation (`[gui]` table)" — documenting `app_name`, that it's applied via a runtime window-title change (not a rebuild), and that `mlai package build` now automatically bundles the distribution's `manifest.toml` as a packaged resource so a stock `mlai-gui` binary picks up the adopter's real components/branding with no separate `mlai-gui` checkout needed.

```bash
git add docs/USAGE.md
git commit -m "docs: document [gui] app_name and manifest-as-packaged-resource"
```

- [ ] **Step 12: Mark this plan's checkboxes complete**

Update this file (`docs/superpowers/plans/2026-08-19-gui-whitelabel.md`), checking off every completed step, then commit:

```bash
git add docs/superpowers/plans/2026-08-19-gui-whitelabel.md
git commit -m "docs: mark gui-whitelabel plan complete"
```
