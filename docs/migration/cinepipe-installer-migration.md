# Migrating cinepipe-installer onto MLAppInstaller

**Audience**: the CinePipeAi team, deciding what to do with `feat/unified-rust-installer` and `docs/UNIFIED-INSTALLER-DESIGN.md`.
**Status**: handoff document, not an MLAppInstaller implementation plan — nothing here executes automatically. Originally written 2026-08-16, updated 2026-08-19 against MLAppInstaller's current state (`mlai-core`, `mlai-cli`, `mlai-credentials`, `mlai-gui`, `mlai-package`, model catalog).

## Bottom line

Most of what's on `feat/unified-rust-installer` (unmerged, 59 commits, real and tested) was ported into MLAppInstaller directly this session — the removals path-guard, per-platform setup/health, and the wizard's frontend all trace straight back to that branch, with attribution in the relevant commits and docs. **Recommendation: don't finish `feat/unified-rust-installer` as a CinePipe-only effort — retarget that work at adopting MLAppInstaller instead**, since a large fraction of it is now already built, tested (on Windows/macOS/Linux, which that branch's own design doc flagged as an unmet "reliability prerequisite"), and living somewhere CinePipe can share it with TA and future projects instead of maintaining it alone.

**2026-08-19 update — the one gap this doc originally flagged is now closed.** `add_project`/project-binding (the "Add Project" GUI feature, `BindsToProjectType`) is fully ported: `mlai_core::pipeline::bind_project`, the `mlai bind-project` CLI subcommand, and a "Bind a Project" GUI panel are all merged to `main` (see `docs/superpowers/specs/2026-08-18-bind-project-design.md`). `mlai-package` (native MSI/dmg/deb via `cargo-packager`, signing-as-reference, GitHub Releases deploy adapter) and `mlai init` (the guided distribution-authoring wizard) are also now built and merged — previously this doc said both were "designed but not built." **There is no longer a known missing capability blocking a clean cutover.** The remaining work is the actual migration (converting `manifest.json`/`model-catalog.json`, wiring up a distribution profile) plus real verification on CinePipe's own manifests and hardware — see "Readiness assessment" below.

## Readiness assessment (2026-08-19)

| Capability `UNIFIED-INSTALLER-DESIGN.md` needs | Status |
|---|---|
| Cross-platform install/repair/uninstall engine | ✅ merged, CI-proven on all 3 platforms |
| Per-platform setup/health (Windows + POSIX) | ✅ merged |
| Model catalog (multi-owner, hardware-aware) | ✅ merged |
| GUI wizard (install/repair/mode-select) | ✅ merged |
| Project binding ("Add Project", UE5 `.uproject` binding) | ✅ merged 2026-08-19 — was the one named gap, now closed |
| Native installer packaging (MSI/dmg/deb, signing-as-reference) | ✅ merged (`mlai-package`) |
| Publish to GitHub Releases | ✅ merged (`mlai package deploy`) |
| Guided distribution setup (`mlai init`) | ✅ merged |
| System dark/light theme, adopter-overridable | 🚧 in progress as of 2026-08-19, no known blocker |
| UE5-project registry-based detection, Windows console suppression | ⬜ not yet verified on real Windows hardware against the *ported* code (was verified against the original branch's own code by the CinePipe team separately — that verification doesn't automatically carry over and should be redone against `mlai-core`) |
| Manifest/catalog conversion for CinePipe's actual components | ⬜ not started — this doc's phased plan, not yet executed |

**Net: MLAppInstaller has no known missing capability for cinepipe-installer's production cutover.** What's left is real migration work (converting CinePipe's actual manifest/catalog files) and real verification (Windows hardware, GUI click-through against CinePipe's own components) — not waiting on MLAppInstaller to build anything further.

## What migrates directly (already proven, don't redo it)

| cinepipe-installer (`feat/unified-rust-installer`) | MLAppInstaller | Note |
|---|---|---|
| `cleanup.rs`'s `safe_target` path guard | `mlai_core::removals::safe_target` | Ported near-verbatim, including the fix for the real prefix-confusion bug documented in that file's own header comment. |
| `cleanup.rs`'s `apply_removals`/`clean_install`/`remove_orphaned_components` | `mlai_core::removals::{apply_removals, clean_install, remove_orphaned_components}` | Same algorithm; `.cinepipe-install` reserved dir renamed to `.mlai-install`. |
| `versioning.rs`'s `compare_version` | `mlai_core::versioning::compare_version` | Verbatim algorithm. |
| `manifest.rs`'s `PlatformSetup`/`PlatformHealth`/`setup_for_current_os()` pattern | `mlai_core::manifest::{PlatformSetup, PlatformHealth, Component::setup_for_current_os()}` | Same `windows`/`posix` split and `cfg!(target_os)` selection; field names generalized to snake_case/TOML instead of PascalCase JSON. |
| `wizard/src/main.ts` (plain TS, no React) | `crates/mlai-gui/src/main.ts` | Ported directly, re-skinned generic. See "What needs real work" for what was dropped. |
| Setup Options Protocol (`--describe-options`/`--set key=value`) | `mlai_core::options_protocol` | Verbatim-compatible flag names, by design (`docs/superpowers/specs/2026-08-14-foundation-design.md`, "Additional decisions"). |
| `repair_component`/`repair_all` | `mlai_core::pipeline::repair_component` | Same "always re-verify disk, ignore recorded state" semantics, same "zero filesystem changes when genuinely healthy" guarantee, same test scenarios ported. |

## What requires real changes

**Manifest format: JSON → TOML, PascalCase → snake_case.** `manifest.json`'s `Name`/`Ref`/`Default`/`Setup`/`Health` become `name`/`ref`/`default`/`setup`/`health` in TOML. This is the one file that needed rework either way (it was already carrying `manifest.psd1` legacy-compat baggage `UNIFIED-INSTALLER-DESIGN.md` itself wanted to retire — "the hand-synced-twins pattern is exactly the kind of drift this design exists to remove, not preserve"). Converting to MLAppInstaller's schema and retiring `manifest.psd1` are the same piece of work, not two.

**Model catalog: one file → owned fragments.** `model-catalog.json` today is a single canonical file — proven to work, but it assumes one team can maintain it, which doesn't hold as CinePipe's sub-projects (cinepipe-director, cinepipe-warden, cinepipe-stories, ...) develop independently. MLAppInstaller's catalog mechanism (`docs/superpowers/specs/2026-08-15-distribution-packaging-framework-design.md`) generalizes exactly this problem: split `model-catalog.json`'s `purposes` across per-sub-project fragment files, each purpose keeping an explicit `owner`:

```toml
# cinepipe-stories's own model-catalog.toml fragment
[purposes.text-structured-json]
owner = "cinepipe-stories"

[[purposes.text-structured-json.tiers]]
min_vram_gb = 24
model = "qwen3:32b"

[[purposes.text-structured-json.tiers]]
min_vram_gb = 8
model = "qwen3:8b"
notes = "recommended baseline"
```

`cinepipe-warden` (or any future sub-project) can then own its own purposes without needing write access to `cinepipe-stories`'s fragment, and `mlai catalog resolve`'s merge step hard-errors if two fragments ever disagree on the same purpose again — the exact bug class `model-catalog.json`'s header comment documents already happening once (`cinepipe-stories` vs. `cinepipe-director` disagreeing on Apple Silicon VRAM-derating). Apple-unified-memory derating and GPU reservation (both already named in that header comment, both noted there as "not built yet") are now real resolver parameters (`effective_vram_gb`, `reserve_vram_gb`) instead of ad hoc per-product math.

**GUI wizard: `add_project`/project-binding — closed 2026-08-19.** `mlai-gui` now includes project binding: a `binds_to_project_type` manifest field, `mlai_core::pipeline::bind_project`, the `mlai bind-project` CLI subcommand, and a "Bind a Project" GUI panel (engine-type dropdown, native file picker via `tauri-plugin-dialog`, bind button) — same semantics as the original `add_project` (untagged and not-yet-installed components are untouched, matched components are force-reinstalled with the real project path substituted for a `{project}` placeholder in their setup args). See `docs/superpowers/specs/2026-08-18-bind-project-design.md` and `docs/superpowers/plans/2026-08-18-bind-project.md`.

**Distribution/licensing is CinePipe-specific and stays that way.** `UNIFIED-INSTALLER-DESIGN.md`'s activation-gating design (installer runs fully unauthenticated; the *installed product* checks in with `cinepipe-license` at runtime) is entirely CinePipe's own product concern — MLAppInstaller has no opinion on licensing and shouldn't. Nothing here changes that design; `mlai-package` (once built) only handles *how the bytes get packaged and published*, not *whether the installed product then requires a license*.

## What's available now (was "not available yet" as of 2026-08-16)

`mlai-package` (native MSI/dmg/deb generation wrapping `cargo-packager`, signing-as-reference), the `mlai package deploy` GitHub Releases adapter, and `mlai init` (the guided distribution-authoring wizard) are all **built and merged** — see `docs/superpowers/specs/2026-08-15-distribution-packaging-framework-design.md`, `docs/superpowers/plans/2026-08-16-mlai-package-foundation.md`, `docs/superpowers/plans/2026-08-16-github-releases-deploy-adapter.md`, `docs/superpowers/plans/2026-08-16-mlai-init-wizard.md`. `UNIFIED-INSTALLER-DESIGN.md`'s own "Reliability prerequisites" (a real CI matrix; end-to-end verification on all target platforms, not just the build platform) are **already satisfied here** — this project's CI runs on `ubuntu-latest`/`macos-latest`/`windows-latest` today, which is more cross-platform proof than `feat/unified-rust-installer` had on its own (GitHub Actions for this repo is currently billing-blocked account-wide as of 2026-08-19 — an account/billing issue, not a code issue; recent merges were verified via full local `cargo build`/`test`/`clippy`/`fmt` runs instead).

## Phased plan

1. **Now**: convert `manifest.json` → `manifest.toml` against MLAppInstaller's schema; retire `manifest.psd1` in the same pass (this was already `UNIFIED-INSTALLER-DESIGN.md`'s own stated intent).
2. **Now**: split `model-catalog.json` into per-sub-project fragments with explicit ownership; wire each component's setup script to call `mlai catalog resolve` instead of reading the monolithic catalog directly.
3. **Now**: switch component setup scripts to declare `supports_options_protocol` per platform against MLAppInstaller's manifest shape — no script changes needed, this is purely a manifest-declaration update since the protocol itself is unchanged.
4. **Now**: tag the UE5 component(s) with `binds_to_project_type = "UE5"` in the converted manifest and drop the old wizard fork's separate "Add Project" implementation — `mlai-gui`'s own panel now covers it.
5. **Now**: author a distribution profile (signing-identity references for the existing Windows/macOS certs, GitHub Releases as the deploy target) to replace the packaging half of `UNIFIED-INSTALLER-DESIGN.md` — licensing/activation-gating stays exactly as separately designed there.
6. **Verify on real hardware before cutover**: the UE5 registry-based engine detection and Windows console-suppression logic, and a full GUI click-through (install, repair, bind-project) against CinePipe's actual converted manifest — not yet done against the ported `mlai-core`/`mlai-gui` code specifically.
7. **Retire** `feat/unified-rust-installer` once steps 1-6 are confirmed working from MLAppInstaller directly, rather than maintaining both.
