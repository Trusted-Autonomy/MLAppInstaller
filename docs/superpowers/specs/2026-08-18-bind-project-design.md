# Project Binding (`bind-project`): Design

**Status**: Approved 2026-08-18 (design confirmed directly by user; grounded against the real mechanism in `cinepipe-installer`'s `feat/unified-rust-installer` branch, read directly — `wizard/src-tauri/src/{manifest,components,lib}.rs`, `wizard/src/main.ts`).
**Extends**: `docs/superpowers/specs/2026-08-14-foundation-design.md` (manifest/pipeline) and `docs/superpowers/specs/2026-08-15-gui-wizard-design.md` (GUI).

## Problem

`docs/migration/cinepipe-installer-migration.md` names one real, not-yet-generalized gap blocking a clean cutover of `cinepipe-installer` onto MLAppInstaller: **project binding**. In the branch, a component can tag itself `BindsToProjectType = "UE5"`; once that component is installed, the GUI's "Add Project" action lets the end user pick a real `.uproject` file, and the wizard re-runs that component's setup command with the real project path substituted in for a `{Project}` placeholder. This is used in production today — `cinepipe-installer` is the installer currently shipping to customers — so this is not a hypothetical feature, it's a hard requirement for the cutover.

## Scope

**In scope:**
- `mlai-core`: a `binds_to_project_type: Option<String>` field on `Component`, and `{project}` placeholder substitution in a component's setup command args.
- `mlai-core::pipeline`: a `bind_project` function with the same semantics as the branch's `add_project` — find installed components matching a given project type, force-reinstall each with the real path substituted.
- `mlai-cli`: a `mlai bind-project --type <engine> --path <file>` subcommand.
- `mlai-gui`: an "Add Project" panel — engine-type dropdown sourced from installed components' `binds_to_project_type` values, a native file picker, a bind button — wired to a new Tauri command.

**Explicitly out of scope:**
- A general named-parameter mechanism for setup commands (considered and rejected for v1 — see Decisions). `{project}` is the only substitution this design adds.
- Any change to how `install`/`repair`/`uninstall` treat non-project-bound components — this is purely additive.

## Architecture

### 1. Manifest (`mlai-core::manifest`)

```rust
pub struct Component {
    // ...existing fields...
    #[serde(default)]
    pub binds_to_project_type: Option<String>,
}
```

TOML:
```toml
[[components]]
name = "ue5-cine-pipeline"
binds_to_project_type = "UE5"

[components.setup.windows]
command = "install\\Install-CinePipe-UE5.ps1"
args = ["-Project", "{project}", "-NoPause"]
```

Same optional field, same `windows`/`posix` per-platform setup shape already established. A component with no `binds_to_project_type` is completely unaffected — this mirrors the branch's own "untagged components are untouched" guarantee (verified in its own test suite: `add_project_leaves_untagged_and_uninstalled_components_untouched`).

### 2. Pipeline (`mlai-core::pipeline`)

```rust
pub fn bind_project(
    components: &[Component],
    installed: &mut InstalledState,
    options: &PipelineOptions,
    project_type: &str,
    project_path: &Path,
) -> Vec<ComponentResult>
```

Semantics, ported directly from the branch's `components::add_project`:
- Filter to components where `binds_to_project_type.as_deref() == Some(project_type)`.
- Of those, filter further to components already recorded as installed (`installed.json` — an uninstalled component can't be bound to a project).
- For each match, substitute `{project}` in the component's setup command args with `project_path`'s string form, then force-reinstall via the existing `install_component` path (same forced-reinstall mechanism `repair_component`/`force` already use — no new install codepath).
- Components with no match (wrong project type, or matching type but not installed) are left untouched — zero filesystem changes, matching this project's existing "no-op means literally no-op" guarantee used throughout `pipeline.rs`.

### 3. CLI (`mlai-cli`)

```
mlai bind-project --manifest <path> --install-root <path> --type <engine> --path <project-file>
```

Same flag-naming convention as `install`/`repair` (`--manifest`, `--install-root`). Errors when zero components match the given `--type` are loud and actionable per the Observability Mandate (name the type, name the manifest, suggest checking `binds_to_project_type` declarations) — not a silent no-op.

### 4. GUI (`mlai-gui`)

Ported from `wizard/src/main.ts`'s existing "Add Project" panel:
- Engine-type dropdown, populated from installed components' `binds_to_project_type` values (dedup'd) — only shown when at least one installed component declares the field.
- Native file-picker via `tauri-plugin-dialog` — verified not currently a dependency of `mlai-gui` (checked `crates/mlai-gui/src-tauri/Cargo.toml`), so it's a new dependency this plan adds, same pattern as any other Tauri plugin.
- "Add Project" button invokes a new Tauri command `bind_project(install_root, project_type, project_path)` wrapping `mlai_core::pipeline::bind_project`.

## Decisions

1. **Port near-verbatim, generalized only in naming/casing** (user-confirmed 2026-08-17): `binds_to_project_type`/`{project}`/`bind_project`, not a broader named-parameter mechanism. Rationale: the only known real use case today is UE5 project binding; a general mechanism has no second consumer to validate its shape against, and this project's own constitution (§1.6 Reuse Before Reinventing, and the broader YAGNI stance already applied throughout this session) argues against speculative generalization.
2. **`{project}` is the only placeholder**, not a general templating syntax — matches the branch's own scope exactly.
3. **Force-reinstall reuses the existing `install_component` force path** rather than a new "rebind" codepath — same mechanism `repair`/`force: bool` already exercises, no new pipeline primitive beyond the filter-and-substitute logic itself.
4. **CLI-first, GUI-second within this one plan** — both are in scope (the GUI is the actual production requirement), but the CLI subcommand is the interface `mlai-gui`'s Tauri command wraps, so it's built first within the same implementation plan, not a separate one.

## Relationship to the cinepipe-installer migration

This closes the project-binding gap named in `docs/migration/adopter-migration-guide.md`'s "What usually requires real changes" section. Once built, `mlai-gui` becomes a complete replacement for a bespoke wizard's own project-binding feature, not a subset of it.
