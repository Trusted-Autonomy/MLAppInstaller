# Using MLAppInstaller

## Installing components

`mlai install` reads a TOML manifest and installs every component marked
`default = true` (or a single named component via `--component`).

```bash
mlai install --manifest manifest.toml --install-root ~/my-app
```

## Manifest format

```toml
manifest_version = "1.0.0"

[[components]]
name = "hello-component"
source_url = "https://example.com/hello-component.zip"
ref = "main"
default = true

[components.setup.posix]
command = "setup.sh"
args = []

[components.health.posix]
type = "file_exists"
path = "marker.txt"
```

- `source_url` — a direct HTTPS URL to a zip archive. The archive's single
  top-level folder is renamed to the component's `name` after extraction.
- `ref` — recorded as the installed version; used to detect whether a
  re-run needs to reinstall.
- `setup`/`health`/`supports_options_protocol` are per-platform (`posix`/
  `windows`) — `mlai` picks the entry matching the OS it's running on. A
  component with no entry for the current OS simply has no setup/health/
  options step on that platform. A component with no `health` block at all
  is always considered healthy once setup succeeds.

## Removals (legacy cleanup)

A manifest can declare `[[removals]]` entries — paths to delete once an
install crosses a given `manifest_version`:

```toml
[[removals]]
version = "1.1.0"
paths = ["hello-component/legacy_tool.py"]
```

Applied automatically during `mlai install` when the previously-recorded
`manifest_version` is older than an entry's `version`. Every path is
validated to resolve inside the install root before removal — a malformed
or malicious manifest can never delete anything outside it.

## Uninstalling

```bash
mlai uninstall --manifest manifest.toml --install-root ~/my-app
```

Prompts for confirmation unless `--yes` is passed (never prompts when
`--dry-run` is also given — dry-run is always safe to run non-interactively).
Removes every component named in the manifest plus `<install-root>/.mlai-install`.

## Repairing

`mlai repair` re-verifies every component directly against disk, ignoring
whatever `installed.json` has recorded — the fix for a component a plain
re-run of `install` would silently keep trusting even after something on
disk broke it by hand:

```bash
mlai repair --manifest manifest.toml --install-root ~/my-app
```

A genuinely healthy component is left completely untouched (no download, no
setup re-run). A broken one goes through the same backup-then-reinstall
sequence `install` uses.

## Forcing a reinstall

```bash
mlai install --manifest manifest.toml --install-root ~/my-app --force
```

Reinstalls every selected component from `source_url` regardless of its
recorded state — the same backup-before-overwrite safety as a normal
install, just without the "already healthy, skip" shortcut. This is the
generic form of "get whatever is currently being served" — detecting that
a specific *newer* version exists upstream (vs. blindly re-pulling
`source_url`) isn't implemented yet; see
`docs/superpowers/specs/2026-08-14-foundation-design.md` for status.

## Install state

State is written to `<install-root>/.mlai-install/installed.json` after
every pipeline stage (`downloaded`, `unpacked`, `setup_run`, `healthy`, or
`needs_attention`), so a crashed install resumes rather than restarts.
Re-running `mlai install` against a component already `healthy` at the
manifest's `ref` is a no-op.

## Backups

Before a component directory is replaced, its previous contents are copied
to `<install-root>/.mlai-install/backups/<ref>/<component-name>`. The
newest 3 backups are kept; older ones are pruned automatically.

## Private sources

Set `MLAI_TOKEN` in the environment to send a bearer token with the
component download request (for private/authenticated source URLs).

## Backend options protocol

A component can declare `supports_options_protocol = true` in the manifest
to expose local-vs-hosted choices. `mlai` never probes or passes options to
a component that hasn't declared this — an unpatched setup script could
otherwise silently run its real setup instead of erroring on an unknown
flag.

```bash
mlai install --manifest manifest.toml --install-root ~/my-app --set model=qwen3:14b
```

`--set key=value` is repeatable and passed straight through to the
component's setup command, verbatim compatible with cinepipe-installer's
existing `--set key=value` convention (see
`docs/superpowers/specs/2026-08-14-foundation-design.md`).

## Model catalog

A component that needs a decision like "which local model fits this
machine" can defer to a shared catalog instead of inventing its own
hardware-tier table. Multiple sub-projects can each contribute a fragment
without a central authority — a purpose declares an `owner`; a fragment
that only *references* a purpose (no `[[tiers]]`) never conflicts, but two
fragments that *define* the same purpose differently is a hard error, not
a silent pick:

```toml
# fragment owned by cinepipe-stories
[purposes.text-structured-json]
owner = "cinepipe-stories"

[[purposes.text-structured-json.tiers]]
min_vram_gb = 24
model = "qwen3:32b"

[[purposes.text-structured-json.tiers]]
min_vram_gb = 8
model = "qwen3:8b"
```

```bash
mlai catalog resolve --purpose text-structured-json \
  --catalog fragment-a.toml --catalog fragment-b.toml \
  --os linux --gpu-vendor nvidia \
  --vram-gb 12 --effective-vram-gb 12 --disk-free-gb 200
```

Prints the resolved model name to stdout, or a clear error if nothing fits
or two catalogs disagree. `mlai` does not detect hardware itself — the
`--os`/`--gpu-vendor`/`--vram-gb`/`--effective-vram-gb`/`--disk-free-gb`
flags are the caller's (a component's own setup script) responsibility to
supply, the same as today.

## GUI wizard

A Tauri-based GUI wraps `mlai-core` for users who'd rather not use the CLI:

```bash
cd crates/mlai-gui && npm install && npm run tauri dev
```

It reads a `manifest.toml` bundled next to the app (or, in dev mode, at the
repository root) and supports the same three operations as the CLI --
Install, Force Reinstall, Repair -- with live per-component progress in the
log view. It has no test harness of its own (matching the prior art it was
ported from); verify changes by running it and exercising the flow
manually. Building a distributable app from this GUI is what the
distribution-packaging framework (`docs/superpowers/specs/2026-08-15-distribution-packaging-framework-design.md`)
is for -- not covered here.

## Credentials

`mlai` does not store, manage, or ever see hosted-model API keys or other
secrets — that's out of scope for an installer by design. See
`docs/superpowers/specs/2026-08-15-credential-source-glue-design.md` for the
planned (not yet implemented) approach: a component declares what
credential it needs, `--set <key>_source=...` tells it *where* to find that
credential (OS keychain, 1Password, Vault, an env var, ...), and the
component's own setup command is entirely responsible for resolving and
using it. This design is on hold pending further exploration.

## Publishing a distribution

```bash
mlai package deploy --profile distribution-profile.toml --tag v1.2.3 \
  --file dist/my-app.dmg --file dist/my-app.msi \
  --notes "Release notes" --title "v1.2.3"
```

Requires the profile's `[deploy]` section (`adapter = "github-releases"`,
`repo = "owner/name"`) and a `gh` CLI already authenticated in the
environment: `mlai` never manages GitHub credentials itself. `--draft`/
`--prerelease` are passed through unchanged. Only `github-releases` is
implemented; other deploy destinations are future, separately-designed
work.

## Not yet implemented

Remote-version detection (upgrade because something changed upstream, not
just `--force`), cloud config generation, and the credential-source glue
layer are planned follow-ups — see
`docs/superpowers/specs/2026-08-14-foundation-design.md`.
