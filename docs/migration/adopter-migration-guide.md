# Migrating an Existing Installer onto MLAppInstaller

**Audience**: any team replacing a bespoke, in-house installer/wizard with MLAppInstaller as the shared engine.
**Status**: generic adoption guide — a template for the kind of migration mapping any adopting project should work through, not a plan for a specific project. If your team wants a mapping written against your own installer's actual source, work through each section below against your own codebase (or ask for help doing so) rather than trying to reverse-generalize from this template.

## Bottom line

If your project already has a working, in-house installer — even a rough one, even platform-specific shell/PowerShell twins — most of its actual logic (setup orchestration, health verification, backup-before-replace, versioned cleanup) is proven and shouldn't be thrown away. The migration is mostly a *translation*: your existing mechanisms map onto `mlai-core`'s manifest schema and pipeline, and the parts that don't map directly are usually the parts worth fixing anyway (format drift between platform-specific config twins, one-file bottlenecks that don't survive multiple independently-developed sub-projects, ad hoc hardware-tier logic hand-rolled per feature).

## What typically migrates directly

| A typical existing installer has... | Maps onto MLAppInstaller as... |
|---|---|
| A "don't let cleanup delete outside the install root" path guard | `mlai_core::removals::safe_target` |
| Versioned removal/cleanup-on-upgrade logic | `mlai_core::removals::{apply_removals, clean_install, remove_orphaned_components}` |
| Version-comparison logic for upgrade decisions | `mlai_core::versioning::compare_version` |
| Per-platform setup/health scripts (Windows vs. POSIX twins) | `mlai_core::manifest::{PlatformSetup, PlatformHealth, Component::setup_for_current_os()}` — same `windows`/`posix` split, selected via `cfg!(target_os)`. A POSIX setup script that's exec'd directly (`command: "install/setup.sh"`, not `command: "sh"` with the script as an `arg`) gets its executable bit restored automatically after unpack — GitHub's `codeload.github.com` zipballs never carry Unix permission bits for any file, so if your existing installer does its own `chmod +x` before invoking a setup script (as `install.sh` commonly does), that step is now redundant, not required, once converted. |
| A GUI wizard's component-selection/install-status frontend | `crates/mlai-gui/src/main.ts` as a starting point to re-skin, not rebuild from scratch |
| A "pass through backend-specific config without the installer understanding it" protocol | `mlai_core::options_protocol` (`--describe-options`/`--set key=value`) |
| A "verify against real disk state, not just the recorded install log" repair path | `mlai_core::pipeline::repair_component` |
| A "bind an already-installed component to a real target project/file after the fact" feature | `mlai_core::pipeline::bind_project` + the `mlai bind-project` CLI subcommand + `mlai-gui`'s "Bind a Project" panel |

## What usually requires real changes, not just translation

**Manifest format.** Whatever format your existing installer's manifest uses (JSON, a platform-specific config format, hand-synced twins across platforms), converting to MLAppInstaller's `manifest.toml` schema is close to unavoidable — but it's also usually the fix for a real, pre-existing pain point (format drift between hand-synced twins is exactly the kind of thing a single schema exists to remove).

**Model/hardware-tier catalogs.** A single canonical "which model fits which hardware" file works fine for one team, but breaks down once multiple independently-developed sub-projects need to contribute their own hardware-tier decisions without a central maintainer serializing every change. MLAppInstaller's catalog mechanism generalizes this: each sub-project ships its own fragment, explicitly owning the purposes it defines:

```toml
# your-subproject's own model-catalog.toml fragment
[purposes.some-purpose]
owner = "your-subproject"

[[purposes.some-purpose.tiers]]
min_vram_gb = 24
model = "some-large-model"

[[purposes.some-purpose.tiers]]
min_vram_gb = 8
model = "some-smaller-model"
notes = "recommended baseline"
```

`mlai catalog resolve`'s merge step hard-errors if two fragments ever disagree about the same purpose, rather than silently coalescing — the structured `HardwareProfile` (OS, GPU vendor, raw + effective VRAM, disk) also replaces whatever ad hoc VRAM-only logic your installer may have hand-rolled per feature.

**Project/target binding.** If your installer has a feature where an already-installed component gets bound to a real target file or project after the fact (a game-engine project file, a workspace path, anything supplied at bind-time rather than install-time), that generalizes to `manifest.toml`'s `binds_to_project_type` field plus `mlai bind-project` — a component declares what type it binds to, and a `{project}` placeholder in its setup command gets the real path substituted in on bind.

**Licensing/activation.** Whatever your product's own licensing or activation-gating model is, that stays entirely your own product's concern — MLAppInstaller has no opinion on it and shouldn't. It only handles *how the installer's bytes get packaged and published*, never *whether the installed product then requires a license*.

See `docs/migration/configuration-depot-architecture.md` for the concrete mechanics of pulling in MLAppInstaller as a "configuration depot" dependency (pinned CLI binary vs. building the GUI from source) and access-grant instructions if your team needs direct push access rather than fork-based PRs.

## Phased plan

1. Convert your existing manifest format to `manifest.toml` against MLAppInstaller's schema; retire any legacy hand-synced config twins in the same pass.
2. If you have a model/hardware-tier catalog, split it into per-sub-project fragments with explicit ownership; wire component setup scripts to call `mlai catalog resolve` instead of reading a monolithic catalog directly.
3. Declare `supports_options_protocol` per platform in the converted manifest for any component that already does backend-config passthrough — this is a manifest-declaration update, not a script rewrite, since the protocol itself is unchanged.
4. If you have a project-binding feature, tag the relevant component(s) with `binds_to_project_type` in the converted manifest and drop any separate bespoke "bind after install" implementation — `mlai-gui`'s panel and `mlai bind-project` cover it.
5. Author a distribution profile via `mlai init`, then use `mlai package build`/`mlai package deploy` for signed, native installers — replacing any bespoke packaging/signing pipeline.
6. Verify on real target-platform hardware before cutover, especially anything OS-specific your components do outside MLAppInstaller's own scope (native API calls, registry lookups, platform-specific detection) — that verification doesn't automatically carry over from your prior installer and should be redone against the migrated code specifically.
7. Retire the old installer once the above is confirmed working end-to-end, rather than maintaining both indefinitely.
