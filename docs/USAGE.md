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

## Credentials

`mlai` does not store, manage, or ever see hosted-model API keys or other
secrets — that's out of scope for an installer by design. See
`docs/superpowers/specs/2026-08-15-credential-source-glue-design.md` for the
planned (not yet implemented) approach: a component declares what
credential it needs, `--set <key>_source=...` tells it *where* to find that
credential (OS keychain, 1Password, Vault, an env var, ...), and the
component's own setup command is entirely responsible for resolving and
using it. This design is on hold pending further exploration.

## Not yet implemented

`repair`, `update`, cloud config generation, and the credential-source glue
layer are planned follow-ups — see
`docs/superpowers/specs/2026-08-14-foundation-design.md`.
