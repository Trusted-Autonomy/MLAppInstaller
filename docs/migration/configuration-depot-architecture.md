# Configuration Depot Architecture

**Audience**: whoever implements TA's and CinePipe-installer's adoption of MLAppInstaller — this is the concrete "how" that `docs/migration/ta-migration.md` and `docs/migration/adopter-migration-guide.md` describe at the mapping/phased-plan level.
**Status**: current as of 2026-08-28. `mlai`'s own release pipeline (`docs/RELEASING.md`) and per-distribution icons (`docs/superpowers/specs/2026-08-28-package-icons-design.md`) both landed/are landing this week — this doc assumes both exist; check `git log` if reading this later than 2026-08-28 to confirm nothing since has changed the shape described here.

## The core idea: a "configuration depot," not a fork

An adopting project's own repository should contain **only data and its own real build scripts** — never a copy of MLAppInstaller's Rust/Tauri source. This has been true for the CLI engine since the beginning; the private-labeling work (`docs/superpowers/specs/2026-08-19-gui-privatelabel-design.md`) made it true for the GUI too. A depot repo holds:

- `manifest.toml` — the adopter's real component list, setup commands (their own existing scripts, unchanged), health checks.
- Model catalog fragment(s), if the adopter has per-machine model selection needs (`mlai catalog resolve`).
- `distribution-profile.toml` — `[gui] app_name`/`theme`, `icons`, signing identity references, deploy target. Written once via `mlai init`, hand-edited after.
- Their own real setup scripts (`install.sh`, `Install-*.ps1`, etc.) — orchestrated, never reimplemented, per `docs/CONSTITUTION.md` §1.6.

Nothing else. No `crates/mlai-gui` checkout, no `crates/mlai-core` fork.

## How a depot pulls in MLAppInstaller

Two independent halves, two different current states:

**CLI (`mlai`)** — ready today. `docs/RELEASING.md`'s pipeline publishes real, pinned binaries as of `v0.1.0` (currently macOS/arm64 only — Linux/Windows pending GitHub Actions billing being unblocked; see that doc before assuming full platform coverage). A depot's own CI:

```bash
curl -sL https://github.com/Trusted-Autonomy/MLAppInstaller/releases/download/v0.1.0/mlai-macos-arm64.tar.gz | tar -xz
./mlai install --manifest manifest.toml --install-root ...
```

Pin to an exact tag (not `main`) per `docs/VERSIONING.md` — the `0.x` series carries no stability guarantee.

**GUI (`mlai-gui`)** — no published binary of MLAppInstaller's own; each adopter builds their own branded one, since the whole point of the private-labeling work is that the binary is generic but the *packaging step* bundles the adopter's manifest as a resource (`docs/superpowers/specs/2026-08-19-gui-privatelabel-design.md`). A depot's CI:

```bash
git clone --depth 1 --branch v0.1.0 https://github.com/Trusted-Autonomy/MLAppInstaller
cd MLAppInstaller/crates/mlai-gui/src-tauri && cargo build --release
cd ../../../..
cargo run -p mlai-cli -- package build \
  --profile /path/to/depot/distribution-profile.toml \
  --target-index 0 \
  --binary MLAppInstaller/target/release/mlai-gui \
  --out-dir dist/
```

This is a `git clone` of MLAppInstaller at a pinned tag, not a fork — the depot never modifies anything inside it. Building `mlai-gui` from source is still required for the GUI half specifically because Tauri compiles the binary itself (window creation, IPC bindings); only the *data it's packaged with* is adopter-specific, and that's supplied entirely at the `mlai package build` step, not by editing MLAppInstaller's own source.

## Readiness checklist per depot

| | TA | CinePipe-installer |
|---|---|---|
| Manifest conversion | Not started — `docs/migration/ta-migration.md` phased plan, step 1 | Not started — `docs/migration/adopter-migration-guide.md` phased plan, step 1 |
| Model catalog fragment | `ta-migration.md`'s worked example is real, ready to convert directly (already used verbatim in `samples/catalog/trusted-autonomy.toml`) | Depends on the adopter's actual `model-catalog.json` — not converted yet |
| GUI relevance | Probably not needed — TA is CLI-first (`ta-migration.md`, "What doesn't migrate") | Needed — this is the whole reason private-labeling/icons exist |
| Branding | N/A (no GUI) | Brand kit available (`CinePipeAi_Brand_Kit_One_Pager_v3_Checked.pdf` — colors, wordmark, type system). **Do not commit any of this to MLAppInstaller's own repo** — it's public now; branding lives entirely in CinePipe-installer's own (private) depot repo's `distribution-profile.toml`/icon files. |
| Icon support | N/A | In progress as of 2026-08-28 — check whether `docs/superpowers/plans/2026-08-28-package-icons.md` is merged before relying on the `icons` field |
| Signing identity | Existing Windows cert + signing scripts (`scripts/sign-windows.ps1`) — convert to a `certificate_thumbprint` reference | Existing Windows/macOS certs per the (removed, CinePipe-specific) migration doc's original content — get the actual values from the CinePipe team directly, not from this repo |
| Engine gaps (mutable-ref version tracking / project-binding-until-bound / multi-project persistence) | Fixed upstream 2026-09-02 — benefits TA too, since `ta-migration.md`'s own worked manifest example uses `ref = "latest"` | Fixed upstream 2026-09-02, found via reviewing `feat/unified-rust-installer`'s real source against `mlai-core` — see `docs/migration/adopter-migration-guide.md`, "Engine gaps found and fixed" |

## Granting CinePipeAi access

The repo is public, so **read access needs no grant at all** — anyone can clone it, download releases, or open a PR from a fork with zero setup. That's sufficient for CinePipe-installer's depot repo to consume MLAppInstaller as described above.

What might genuinely need a grant is **write access**, if CinePipeAi engineers should be able to push branches directly (not fork) — e.g. for faster iteration while converting their manifest/catalog, or to contribute a generalization back (per `adopter-migration-guide.md`: "own the purpose you want to change... or contribute the change back"). Two options, in order of how much trust/setup they require:

1. **No grant needed (default, recommended to start)**: CinePipeAi engineers fork `Trusted-Autonomy/MLAppInstaller` and open PRs normally. Standard open-source flow, works today, nothing to configure.
2. **Direct write access**, if (1) proves too slow: an org admin (current admins: whoever has `admin:org` on `Trusted-Autonomy` — confirm who that is before doing this) invites specific CinePipeAi GitHub accounts as outside collaborators:

   ```bash
   gh api repos/Trusted-Autonomy/MLAppInstaller/collaborators/<github-username> \
     -X PUT -f permission=push
   ```

   Or, for more than one or two people, create a dedicated team scoped to just this repo rather than granting individual access:

   ```bash
   gh api orgs/Trusted-Autonomy/teams -X POST -f name="cinepipeai-contributors" -f privacy=closed
   gh api orgs/Trusted-Autonomy/teams/cinepipeai-contributors/repos/Trusted-Autonomy/MLAppInstaller \
     -X PUT -f permission=push
   # then add each CinePipeAi member:
   gh api orgs/Trusted-Autonomy/teams/cinepipeai-contributors/memberships/<github-username> -X PUT
   ```

   `permission=push` grants write (branch push, no force-push to `main` — branch protection still applies), not admin. Branch protection on `main` (`docs/VERSIONING.md`-adjacent context: PR review + passing CI required) already gates what a `push`-level collaborator can do to `main` directly, same as it gates everyone else.

Recommendation: start with (1). Only move to (2) if fork-based PRs genuinely become a bottleneck — granting org-adjacent access is easy to add later and harder to cleanly walk back once people are used to pushing directly.
