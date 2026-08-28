# Per-Distribution App Icons: Design

**Status**: Approved 2026-08-28 (user directed the fix directly; mechanism verified empirically against the real `cargo-packager` binary — a source PNG placed in its `icons` config field was auto-converted into a valid macOS `.icns` and referenced from a generated `Info.plist`, with zero image-conversion work needed on MLAppInstaller's side).
**Extends**: `docs/superpowers/specs/2026-08-19-gui-privatelabel-design.md`, which explicitly deferred this as a documented, not-yet-built gap.

## Problem

`docs/superpowers/specs/2026-08-19-gui-privatelabel-design.md` closed the manifest/branding-text half of "no fork needed for a branded distribution," but explicitly left icons out of scope: no `DistributionProfile` field, no `cargo-packager` `icons` wiring. A real adopter (first case: CinePipe-installer, with a real brand kit and wordmark) needs an actual app icon, not just a retitled window.

## Verified mechanism

Identical in shape to the already-shipped `resources` fix (`docs/superpowers/specs/2026-08-19-gui-privatelabel-design.md`'s Task 1): `cargo-packager`'s own JSON config accepts an `icons` field (`Vec<String>`, source image paths) and handles all per-platform icon-format conversion itself — confirmed directly, not assumed. No new MLAppInstaller-side image processing is needed; this is purely a config pass-through, exactly like `resources`.

## Scope

**In scope:**
- `mlai-package::profile::Distribution` gains `icons: Vec<String>` (`#[serde(default)]`), alongside the existing `manifest: String` field — distribution-level, not per-target, matching how `manifest`/the already-shipped `resources` wiring both work (cargo-packager converts one source image set appropriately per platform itself, no per-target override needed).
- `mlai-package::packager_config::build_packager_config` adds `icons: Vec<String>` to the generated `cargo-packager` config, populated from `profile.distribution.icons.clone()`.
- `mlai init` wizard: an optional prompt for icon file path(s) (comma-separated, blank = none), written to the profile's new field.
- `docs/USAGE.md` documents the field.

**Explicitly out of scope:**
- Any image validation/format-checking on MLAppInstaller's own side — `cargo-packager` already handles missing/invalid files with its own error, no need to duplicate that.
- Multiple distinct icon sets per platform/target — cargo-packager's own single `icons` list already produces correct per-platform output from one source; a genuine per-target override need hasn't come up and isn't being spec'd speculatively.

## Architecture

### 1. `mlai-package::profile::Distribution`

```rust
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Distribution {
    pub name: String,
    pub manifest: String,
    #[serde(default)]
    pub components: Vec<String>,
    #[serde(default)]
    pub icons: Vec<String>,
}
```

TOML:
```toml
[distribution]
name = "example-app-suite"
manifest = "manifest.toml"
icons = ["icon.png"]
```

### 2. `mlai-package::packager_config`

`PackagerConfig` gains `icons: Vec<String>` (`#[serde(skip_serializing_if = "Vec::is_empty")]`, matching how `resources` was added); `build_packager_config` sets it from `profile.distribution.icons.clone()`.

### 3. `mlai init`

New optional prompt after the existing signing-identity/certificate-thumbprint prompts (same "blank = none" convention already used throughout the wizard): "Icon file(s) (comma-separated paths, blank = none)". Parsed into `Vec<String>`, written to `[distribution] icons = [...]` only when non-empty (an empty list should serialize as absent, not `icons = []`, matching how `components` already behaves when blank — verify against the existing wizard code's actual serialization approach for `components` and mirror it exactly).

## Decisions

1. **Pure config pass-through, no new MLAppInstaller-side logic** — mirrors the `resources` fix exactly, verified against the real tool rather than assumed.
2. **Distribution-level, not per-target** — cargo-packager already does correct per-platform conversion from one source list; no evidence a per-target override is needed yet.
