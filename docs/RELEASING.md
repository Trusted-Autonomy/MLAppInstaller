# Releasing MLAppInstaller

How the `mlai` CLI binary itself gets released, so adopters can depend on a
pinned artifact instead of building from source. See `docs/VERSIONING.md`
for what a version number means; this document is the mechanics of cutting
one.

## What gets released

The `mlai` CLI binary (`mlai-cli`), per platform, as a compressed archive —
`tar.gz` on macOS/Linux, `.zip` on Windows. Not a native installer: `mlai`
is a developer-facing CLI tool, the same shape as `ripgrep`/`fd`/etc., not
an end-user GUI app. `mlai-gui` packaging (signed `.dmg`/`.msi`/`.deb` via
`mlai package build`) is a separate, GUI-specific concern — this repo
doesn't currently publish its own `mlai-gui` release; an adopter builds
their own branded one against their own `manifest.toml`/distribution
profile using the private-labeling mechanism (`docs/superpowers/specs/
2026-08-19-gui-privatelabel-design.md`).

## Automated path (`.github/workflows/release.yml`)

Pushing a tag matching `v*` (e.g. `v0.1.0`) triggers a 3-platform matrix
build of `mlai-cli` in release mode, packages each into the archive shape
above, and publishes them as a GitHub Release via `gh release create
--generate-notes`.

**Currently dormant**: GitHub Actions on this repo is billing-blocked (see
any recent PR's failed CI checks — "recent account payments have failed or
your spending limit needs to be increased"). The workflow is real and
ready; it just can't run until that's resolved. Don't assume a tag push
actually published anything until you've confirmed the workflow ran.

## Manual path (while Actions is blocked)

From a machine for each target platform you can build on:

```bash
cargo build --release -p mlai-cli
uname -m   # check the real architecture before naming the archive -- Apple
           # Silicon Macs (and GitHub's own macos-latest runners) are arm64,
           # not x86_64; a mislabeled archive silently ships the wrong binary
# macOS/Linux:
tar -C target/release -czf mlai-<platform>-<arch>.tar.gz mlai
# Windows (PowerShell):
Compress-Archive -Path target/release/mlai.exe -DestinationPath mlai-windows-x86_64.zip
```

Then publish (repeat `--file`/positional args for every platform's archive
you were able to build — a partial release, e.g. macOS-only, is fine and
should say so in `--notes`, not silently ship as if it were complete):

```bash
gh release create v0.1.0 mlai-macos-arm64.tar.gz \
  --repo Trusted-Autonomy/MLAppInstaller \
  --title "v0.1.0" \
  --notes "macOS (Apple Silicon) binary only -- Linux/Windows/Intel Mac pending CI (GitHub Actions billing-blocked, see docs/RELEASING.md)."
```

## Version bump checklist

1. Update `Cargo.toml`'s `[workspace.package].version` (the single source
   of truth per `CLAUDE.md`).
2. Update `CLAUDE.md`'s "Current State" section to match.
3. Run the full local verification (`cargo build --workspace && cargo test
   --workspace && cargo clippy --workspace --all-targets -- -D warnings &&
   cargo fmt --all -- --check`) — same bar as any other merge.
4. Commit the version bump, merge to `main`.
5. Tag: `git tag v<version> && git push origin v<version>`.
6. Once Actions is unblocked: confirm the release workflow actually ran
   and produced all 3 platform archives — don't assume from the tag push
   alone. Until then: follow the manual path above.
