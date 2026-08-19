# GUI Wizard (`mlai-gui`): Design

**Status**: Approved 2026-08-15 (scope confirmed by user in conversation; ready for implementation planning)
**Supersedes**: The "GUI wizard" fast-follow phase named but not designed in `docs/superpowers/specs/2026-08-14-foundation-design.md`.

## Problem

The engine (`mlai-core`/`mlai-cli`) reached feature parity with a prior-art proven install/uninstall/repair capability this session, with real cross-platform CI. Per the original architecture decision ("CLI/engine first, GUI fast-follow"), this is the point that fast-follow was meant to trigger. A prior-art project's own unmerged branch already has a working, real (not aspirational) Tauri wizard — verified zero `tauri::` coupling in its actual logic modules, only its 6-command `lib.rs` layer touches Tauri APIs — but that wizard's command layer calls its own local copies of `backup`/`cleanup`/`manifest`/`components`/`health`/`versioning`/`python`, not this project's `mlai-core`.

## Scope

**In scope**: a new `mlai-gui` crate (Tauri 2), porting a prior-art project's actual frontend (`wizard/src/main.ts`, plain TypeScript + Vite, no React — 455 lines) re-skinned generic, with its 6 Tauri commands (`list_components`, `default_install_root`, `describe_component_options`, `read_install_status`, `add_project`, `run_install`) reimplemented against `mlai-core`/`mlai-cli` directly rather than porting their local module copies. Covers `mlai install`/`uninstall`/`repair` — the CLI surface that exists today.

**Explicitly out of scope**:
- `add_project` (a prior-art project's UE5 project-binding concept) — not yet generalized in `mlai-core`'s manifest (noted as a future generalization in the foundation spec); dropped from this port rather than carrying product-specific project-binding into a generic GUI.
- The distribution-packaging framework's own UI — that's a developer/adopter tool (`mlai init`, CLI), not an end-customer GUI concern.
- Code signing / notarization / auto-update UI, model-catalog visual selection — all future work, not v1.
- A real end-to-end GUI test harness — a prior-art project's own wizard has none either ("manual verification only," per their README); this port keeps the same posture. Rust-side command logic gets real unit tests; the TS frontend gets manual `tauri dev` verification.

## Architecture

`mlai-gui` is a Tauri 2 app. Its `src-tauri/src/lib.rs` exposes Tauri commands as thin wrappers calling into `mlai-core` (parsing, pipeline) directly (in-process, not shelling out to the `mlai` binary — matches how a prior-art project's own branch already evolved past shelling out to `install.ps1`/`install.sh`). Its `src/main.ts` is a close port of a prior-art project's actual frontend: a component-checkbox screen, an install-root picker (native file dialog via `@tauri-apps/plugin-dialog`), a live progress/log view streaming Tauri events, re-skinned to be product-agnostic (no source-project branding/copyright headers, generic component list sourced from whatever `manifest.toml` is bundled).

Command mapping (the source project's command → `mlai-gui`'s reimplementation):
- `list_components` → parses the bundled `manifest.toml` via `mlai_core::manifest::Manifest::parse`, returns it to the frontend as JSON (Tauri serializes `Manifest` directly since it already derives `Serialize`).
- `default_install_root` → a small platform-specific default path helper (new, since `mlai-core` doesn't currently have one — `mlai install`/`uninstall`/`repair` all take `--install-root` explicitly today).
- `describe_component_options` → calls `mlai_core::options_protocol::describe_options` for components with `supports_options_protocol_for_current_os()` true.
- `read_install_status` → calls `mlai_core::state::InstalledState::load` and returns it.
- `run_install` → calls `mlai_core::pipeline::install_component` per selected component, streaming progress as Tauri events (matching a prior-art project's own `progress: Option<Sender<String>>` pattern in `InstallContext`, adapted to `mlai-core`'s existing function signatures rather than adding a new progress-channel parameter to the pipeline itself — the GUI command spawns a thread and polls/logs around the existing synchronous call, not a `mlai-core` API change).
- `add_project` → dropped (out of scope, see above).

## Decisions

1. Reimplement commands against `mlai-core` directly; do not port the source project's local module copies (`backup.rs`/`cleanup.rs`/etc. in their `wizard/src-tauri/src/`) — this project already has tested equivalents.
2. Port their actual frontend code (plain TS, no React) as the starting point, re-skinned generic — not a rewrite from scratch.
3. In-process calls to `mlai-core`, not shelling out to the `mlai` binary.
4. `add_project`/project-binding dropped from this port; project-binding generalization (if ever needed) is separate future work.
5. No new GUI-specific test harness; matches a prior-art project's own already-accepted "manual verification only" posture for the frontend, real unit tests for the Rust command layer's logic.
