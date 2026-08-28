# Migrating TrustedAutonomy onto MLAppInstaller

**Audience**: the TA team, deciding what to actually do with `v0.18.2` (`Extract ta-package + Cross-Platform Installer`).
**Status**: handoff document, not an MLAppInstaller implementation plan — nothing here executes automatically. Originally written 2026-08-16, updated 2026-08-19 against MLAppInstaller's current state (`mlai-core`, `mlai-cli`, `mlai-credentials`, `mlai-gui`, `mlai-package`, model catalog — see `docs/superpowers/specs/2026-08-14-foundation-design.md` for the full architecture).

## Bottom line

TA's `PLAN.md` `v0.18.2` item was written to build exactly what now exists here. **Recommendation: close `v0.18.2` as "superseded by MLAppInstaller" and replace it with an adoption item**, not a from-scratch build. What follows is what that adoption actually involves.

**2026-08-19 update:** the item this doc originally flagged as the sole reason to wait — `mlai-package` not existing yet — is resolved. `mlai-package` (native MSI/dmg/deb via `cargo-packager`, signing-as-reference), `mlai package deploy` (GitHub Releases adapter), and `mlai init` (guided distribution setup) are all built and merged. Step 4 of the phased plan below no longer needs to wait for anything upstream.

## What TA has today

- `install.sh` — curl-pipe installer, fetches a prebuilt `ta`/`ta-daemon` binary pair from GitHub Releases per-platform. No component graph (TA ships as one thing), no manifest file, rejects Windows outright (tells the user to use `winget`/`scoop`/WSL2).
- `install_local.sh` — builds from source for dev use. Not a distribution path.
- `ta-credentials` (`crates/ta-credentials`) — age-encrypted vault, OS-keychain-first custody, chmod-0600 fallback. This is the **exact code MLAppInstaller's now-reverted `mlai-credentials` was ported from**.
- No model catalog today, but `PLAN.md` `v0.18.3` (voice-transcription plugin) already plans platform-conditional model selection by hand: `parakeet-mlx` on Apple Silicon, NeMo `parakeet-tdt-1.1b-v2` on CUDA/Linux, `faster-whisper` on Windows — this is a hardware-tier decision with vendor/OS constraints, currently undesigned as data.
- No GUI. TA is a CLI-first developer tool; this may stay true after migration (see "What doesn't migrate" below).

## What migrates directly

**Manifest.** TA ships essentially one component (`ta` + `ta-daemon` + channel plugins as one release artifact, not a multi-component model). A minimal `manifest.toml`:

```toml
manifest_version = "1.0.0"

[[components]]
name = "ta"
source_url = "https://github.com/trustedautonomy/ta/releases/latest/download/ta-{platform}.zip"
ref = "latest"
default = true

[components.setup.posix]
command = "./install.sh"
args = []

[components.health.posix]
type = "file_exists"
path = "bin/ta"
```

(Exact `source_url`/setup mapping needs a real per-platform release-asset URL scheme — TA's current `install.sh` already resolves this; that logic becomes this manifest entry's `setup` script, unchanged, per `docs/CONSTITUTION.md` §1.6 — `mlai` never reimplements a component's own setup, it orchestrates it.)

**Windows support**, currently explicitly rejected by `install.sh`, becomes free: `mlai-core`'s per-platform `[components.setup.windows]` lets TA add a real Windows path without touching the posix path at all, and this project's own CI already proves the pattern compiles and runs on `windows-latest`.

**Model catalog for `v0.18.3`.** This is close to a worked example already used to design the catalog mechanism:

```toml
[purposes.voice-transcription]
owner = "trusted-autonomy"

[[purposes.voice-transcription.tiers]]
min_vram_gb = 0
model = "parakeet-mlx"
requires_vendor = ["apple"]
requires_os = ["macos"]

[[purposes.voice-transcription.tiers]]
min_vram_gb = 4
model = "parakeet-tdt-1.1b-v2"
requires_vendor = ["nvidia"]

[[purposes.voice-transcription.tiers]]
min_vram_gb = 0
model = "faster-whisper-medium"
requires_os = ["windows"]
```

`ta vtt install`'s existing platform-detection logic (already hand-written per `PLAN.md`) becomes a call to `mlai catalog resolve --purpose voice-transcription --catalog ta-catalog.toml --os <detected> --gpu-vendor <detected> ...` instead of hardcoded if/else branches — same behavior, now data instead of code, and automatically consistent if a second TA feature ever needs the same kind of decision.

## What requires real changes, not just translation

**`ta-credentials` stays exactly as-is — this is not a migration target.** It's easy to assume the wrong thing here given the history: `mlai-credentials` was originally *ported from* `ta-credentials`, then reverted from MLAppInstaller entirely (see `docs/CONSTITUTION.md` §2.1) because an install-time tool storing secrets is the wrong shape — but TA's own use case (a long-running daemon brokering scoped, revocable credentials to agent processes) is exactly what `ta-credentials` is for, and that need doesn't go away. **Nothing about adopting MLAppInstaller should touch `ta-credentials`.**

**Distribution/packaging is now available.** `v0.18.2` also wanted signed, versioned installers across three platforms — that's `mlai-package` (wraps `cargo-packager`, signing-as-reference) plus `mlai package deploy` (GitHub Releases adapter), per `docs/superpowers/specs/2026-08-15-distribution-packaging-framework-design.md`. **Both are built and merged as of 2026-08-19** — `mlai init` (guided distribution-profile authoring) is too. TA's native-installer/signing work no longer needs to wait on anything upstream.

## What doesn't migrate

- The GUI (`mlai-gui`) is almost certainly not relevant to TA — it's an end-customer install-wizard shape (checkbox components, install-root picker), and TA is a CLI-first tool for developers who are comfortable with `install.sh`. Adopting the engine doesn't obligate adopting the GUI.
- `ta-package`'s actual scope in `PLAN.md` (`ReleaseAsset`, `InstallerConfig`, archive/checksum helpers, platform-detection) — some of this is genuinely `mlai-package`'s job once it exists; some (anything specific to TA's own release-asset naming/versioning conventions) stays TA's.

See `docs/migration/configuration-depot-architecture.md` for the concrete mechanics of how TA's repo pulls in MLAppInstaller (pinned binary vs. building from source) and a readiness checklist.

## Phased plan

1. **Now**: author `manifest.toml` wrapping TA's existing `install.sh`/`install_local.sh` logic as setup commands (near-zero risk — `mlai` orchestrates, doesn't replace, TA's own scripts keep running exactly as they do today).
2. **Now**: add Windows setup support via `[components.setup.windows]`, closing the gap `install.sh` currently punts to WSL2/winget.
3. **Now**: convert `v0.18.3`'s hand-written platform/model logic into a `ta`-owned model catalog fragment; swap `ta vtt install`'s detection branches for `mlai catalog resolve` calls.
4. **Now**: author a distribution profile for signed, native installers via `mlai init`, then `mlai package build`/`mlai package deploy` — replacing the packaging half of `v0.18.2` for real. No longer blocked on upstream work.
5. **Update `PLAN.md`**: close `v0.18.2` as superseded; if the team wants a paper trail, `v0.18.2`'s items map to steps 1, 2, and 4 above, not a from-scratch build.

## Where this slots into `PLAN.md`

This is a **replacement of `v0.18.2`'s content, not a new phase** — same version slot, same `Depends on` line, so nothing else in `PLAN.md` that references `v0.18.2` by number needs renumbering. `v0.18.3` (voice-to-text) already only depends on `v0.18.1`, not `v0.18.2`, so retargeting `v0.18.2` doesn't block it — but phased-plan step 3 above (the model-catalog fragment) should land *before* `v0.18.3`'s own implementation work reaches its platform-detection item, since that item is exactly what step 3 replaces. An agent picking this up should sequence `v0.18.2` before finishing `v0.18.3`, even though `PLAN.md`'s formal `Depends on` field doesn't enforce that ordering today.

Proposed replacement text for the `v0.18.2` entry in `PLAN.md` (drop this in verbatim, adjust item numbering to match whatever's already checked off if work has started):

```markdown
### v0.18.2 — Adopt MLAppInstaller as the Cross-Platform Installer
<!-- status: pending -->

**Goal**: Adopt MLAppInstaller (github.com/Trusted-Autonomy/MLAppInstaller — an external
shared foundation, not a TA-internal crate) as TA's install/packaging engine instead of
building a TA-only `ta-package`. Meridian and future plugin apps get cross-platform
installer support from the same shared engine other adopting projects use, rather than TA
reimplementing packaging logic independently. See MLAppInstaller's
`docs/migration/ta-migration.md` for the full mapping this phase is based on.

**Depends on**: v0.17.4 (release management stable), v0.18.1 (`ta-agent` standalone)

**Items**:

1. [ ] **Author `manifest.toml`**: wrap `install.sh`/`install_local.sh` as `mlai-core`
   setup commands — TA's own scripts keep running unchanged, `mlai` only orchestrates.
2. [ ] **Windows setup path**: add `[components.setup.windows]`, closing the gap
   `install.sh` currently punts to WSL2/winget.
3. [ ] **Model catalog for v0.18.3**: convert the voice-transcription plugin's
   hand-written platform/model logic into a `trusted-autonomy`-owned catalog fragment;
   swap `ta vtt install`'s detection branches for `mlai catalog resolve` calls. Land this
   before v0.18.3's platform-detection item is implemented, not after.
4. [ ] **Distribution profile**: `mlai-package`/`mlai init`/`mlai package deploy` are
   built and merged upstream as of 2026-08-19 (see MLAppInstaller's
   `docs/superpowers/specs/2026-08-15-distribution-packaging-framework-design.md`).
   Run `mlai init` to author a distribution profile, then `mlai package build`/
   `mlai package deploy` for signed Windows/Mac/Linux installers. This replaces the
   original scope's `ta-package` extraction, GH Actions template, and code-signing
   stubs items.
5. [ ] **Meridian CI integration**: point Meridian's release workflow at the adopted
   MLAppInstaller distribution profile instead of a TA-internal packaging template.
6. [ ] **Tests**: real end-to-end `mlai install`/`mlai repair` against TA's own manifest,
   on all three platforms (MLAppInstaller's own CI already proves the underlying engine
   works cross-platform — this item verifies TA's specific manifest/setup scripts do too).
7. [ ] **USAGE.md**: how Meridian and future plugin apps adopt MLAppInstaller as their
   installer.

#### Version: `0.18.2-alpha`
```
