# Versioning & Compatibility Policy

MLAppInstaller is a dependency for multiple, independently-released adopting projects (TrustedAutonomy and others). This document is the contract those adopters can rely on when deciding how to pin and upgrade.

## Version scheme

All crates in this workspace share one version, set in `Cargo.toml`'s `[workspace.package].version` — that field is the single source of truth (`CLAUDE.md`'s "Current State" section is kept in sync with it on every bump). MLAppInstaller follows [Semantic Versioning](https://semver.org/): `MAJOR.MINOR.PATCH`.

**While the version is `0.x`** (current state): per semver's own convention, `0.x` carries no stability guarantee — a `0.x` → `0.(x+1)` bump may include breaking changes. This is the expected state for a foundation still absorbing its first real adopters (TA, and others working through their own migration docs). Pin to an exact git tag or commit, not a branch, if you depend on this before `1.0.0`.

**At and after `1.0.0`**: standard semver — `PATCH` for fixes with no interface change, `MINOR` for additive/backward-compatible changes, `MAJOR` for anything breaking.

## What counts as the public interface (subject to semver)

- **`manifest.toml` schema** — field names, table names (`[[components]]`, `[gui]`, `[[removals]]`, etc.), and their defaults.
- **`distribution-profile.toml` schema** — `[distribution]`, `[[targets]]`, `[deploy]` and their fields.
- **Model catalog fragment schema** — `[purposes.<name>]`, `[[purposes.<name>.tiers]]`, and the merge/conflict-detection behavior.
- **CLI surface** — `mlai` subcommands and their flags (`install`, `repair`, `uninstall`, `catalog resolve`, `package build`, `package deploy`, `init`, `bind-project`).
- **Public Rust items** in `mlai-core`, `mlai-package`, and `mlai-credentials` when consumed as library dependencies (not internal/private items, and not `mlai-cli`'s or `mlai-gui`'s own internals, which are binaries, not libraries other crates depend on).

## What is NOT covered by semver (may change without a major bump)

- Internal module layout / private functions within any crate.
- `mlai-gui`'s frontend (`main.ts`/`styles.css`/`index.html`) internals — the Tauri commands it calls (`list_components`, `run_install`, `bind_project`, etc.) are part of the public interface above; the markup/JS wiring around them is not.
- Test fixtures, CI configuration, and this repository's own `docs/superpowers/` planning artifacts.

## Backward compatibility within a MINOR/PATCH bump

New optional manifest/profile fields are added with `#[serde(default)]` (or the TOML-table equivalent) so an existing, unmodified `manifest.toml`/`distribution-profile.toml` continues to parse and behave identically — this is already the pattern every schema addition in this project has followed (`supports_options_protocol`, `binds_to_project_type`, `[gui] theme`/`app_name`) and is a hard requirement, not a convention to revisit case-by-case.

## Deprecation

A field or CLI flag being removed is a breaking (`MAJOR`) change. Where practical, a field scheduled for removal is documented as deprecated (with a pointer to its replacement) for at least one `MINOR` release before actually being removed in the next `MAJOR`.

## Where changes are announced

GitHub Releases (once packaging/publishing is fully wired for this repo itself, not just for adopters' own distributions) is the release channel. Until then: the merged PR history on `main` and this document are the record — there is no separate, hand-maintained `CHANGELOG.md` today; adding one is a reasonable future addition if adopter count/release cadence grows enough to warrant it, not required now.
