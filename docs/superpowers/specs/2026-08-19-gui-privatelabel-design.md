# GUI Private-Labeling (No-Fork Distribution): Design

**Status**: Approved 2026-08-19 (user directed the fix directly; mechanism verified empirically against the real `cargo-packager` 0.11.8 binary, not guessed).
**Extends**: `docs/superpowers/specs/2026-08-15-distribution-packaging-framework-design.md`, `docs/superpowers/specs/2026-08-19-gui-theme-design.md`.

## Problem

`mlai-cli` is a true dependency today — an adopter supplies `--manifest`/`--install-root` at runtime, no source changes needed. `mlai-gui` is not: `crates/mlai-gui/src-tauri/tauri.conf.json` declares `manifest.toml` as a Tauri **build-time** bundled resource, and hardcodes the window title (`app.windows[0].title`). Since `mlai-package` wraps an *already-compiled* binary (it never runs Tauri's own bundler — confirmed by reading `crates/mlai-package/src/{build,packager_config}.rs`), neither of those can currently be changed per-adopter without maintaining a second checkout of the `mlai-gui` crate that swaps `manifest.toml` and edits `tauri.conf.json` — a fork in substance even though it requires zero UI/Rust code changes.

## Verified mechanism

`cargo-packager` builds the entire platform app bundle (`.app`/`Contents/MacOS`, `Contents/Resources`, Info.plist, etc.) from scratch around whatever raw binary its config points at — it does not require or reuse a Tauri-bundler-produced `.app`. Confirmed directly: a minimal `cargo packager` invocation with `"resources": ["some/local/manifest.toml"]` in its JSON config placed that file at `<App>.app/Contents/MacOS/../Resources/manifest.toml`. That is exactly the path Tauri's own `app.path().resource_dir()` API resolves to at runtime — the same API `mlai-gui`'s existing `find_resource()` function already checks first, before its dev-mode fallback. **This means the manifest-embedding gap closes with zero `mlai-gui` code changes** — only `mlai-package` needs to pass the adopter's manifest path through to `cargo-packager`'s own `resources` config field.

The window title is a different kind of gap: it's compiled into the `mlai-gui` binary itself (via `tauri::generate_context!()` reading `tauri.conf.json` at `mlai-gui`'s own build time), which `mlai-package` never touches. It can still be changed *after* the binary is already running, via Tauri's runtime window API (`getCurrentWindow().setTitle(...)`) — the same category of fix as the theme override already built (`docs/superpowers/specs/2026-08-19-gui-theme-design.md`): read a value out of the (now correctly per-adopter) `manifest.toml` at startup, apply it via a runtime API call, no recompilation needed.

## Scope

**In scope:**
- `mlai-package`: `build_packager_config` adds a `resources: [<distribution.manifest path>]` entry to the generated `cargo-packager` config, so `mlai package build` automatically bundles the adopter's own manifest.toml into the packaged app.
- `mlai-core::manifest::GuiConfig`: add an optional `app_name: Option<String>` field (alongside the existing `theme` field from the theme feature — same table, `[gui] app_name = "..."`).
- `mlai-gui::main.ts`: at the same startup point the theme override already runs, if `manifest.gui.app_name` is set, call Tauri's window API to retitle the window.

**Explicitly out of scope (documented limitation, not solved here):**
- Per-adopter icons. `cargo-packager`'s config plausibly supports its own `icons` field (not yet verified), and `DistributionProfile` has no icon field yet either — a real, separate, addable-later gap. Every adopter ships with MLAppInstaller's placeholder icon until this is built.
- The OS-level app bundle identifier/name shown in Finder/Explorer/Applications list — already fully per-adopter today via `mlai-package`'s existing `productName`/`identifier` config (verified: `cargo-packager` constructs the bundle's own `Info.plist`/equivalent from its config, independent of whatever Tauri itself compiled in), so no change needed there.

## Architecture

### 1. `mlai-package` — bundle the adopter's manifest as a packaging-time resource

`crates/mlai-package/src/packager_config.rs`'s `PackagerConfig` struct gets a new field:

```rust
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackagerConfig {
    // ...existing fields...
    #[serde(skip_serializing_if = "Vec::is_empty")]
    resources: Vec<String>,
}
```

`build_packager_config(profile, target, binary_path)` sets `resources: vec![profile.distribution.manifest.clone()]` — the `Distribution.manifest` field already exists (`pub manifest: String`), this is purely additive wiring, not a schema change to `DistributionProfile`.

### 2. `mlai-core::manifest` — `app_name` alongside `theme`

```rust
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
pub struct GuiConfig {
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub app_name: Option<String>,
}
```

TOML:
```toml
[gui]
theme = "dark"
app_name = "Example Studio Installer"
```

Absent `app_name` (every existing manifest) parses to `None` — the window keeps whatever title `mlai-gui`'s own `tauri.conf.json` compiled in, matching today's behavior exactly.

### 3. `mlai-gui` — retitle at startup

`main.ts`'s `loadComponents()`, right next to the existing theme-override line:

```typescript
if (manifest.gui.app_name) {
  getCurrentWindow().setTitle(manifest.gui.app_name);
}
```

Using `@tauri-apps/api/window`'s `getCurrentWindow()` — already available via the `@tauri-apps/api` dependency `mlai-gui` already has (no new npm package, unlike the theme feature's `tauri-plugin-dialog`).

## Decisions

1. **Zero `mlai-gui` Rust changes for manifest embedding** — the fix lives entirely in `mlai-package`, verified empirically against the real `cargo-packager` binary rather than assumed from documentation, consistent with this project's established practice of installing and testing external tools locally before designing against them.
2. **`app_name` extends the existing `[gui]` table**, not a new table — it's the same "adopter-configured presentation setting" category as `theme`.
3. **Icons are an explicit, documented follow-up**, not solved now — real gap, but additive later (a `DistributionProfile` field + a `cargo-packager` `icons` config entry), not blocking a first close of the "no fork needed" gap.
