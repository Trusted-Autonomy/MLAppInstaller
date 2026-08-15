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

[components.setup]
command = "setup.sh"
args = []

[components.health]
type = "file_exists"
path = "marker.txt"
```

- `source_url` — a direct HTTPS URL to a zip archive. The archive's single
  top-level folder is renamed to the component's `name` after extraction.
- `ref` — recorded as the installed version; used to detect whether a
  re-run needs to reinstall.
- `setup` — optional command run inside the unpacked component directory
  after unpack.
- `health` — optional check run after setup; today supports `file_exists`.
  A component with no `health` block is always considered healthy once
  setup succeeds.

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

`repair`, `uninstall`, `update`, cloud config generation, and the
credential-source glue layer above are planned follow-ups — see
`docs/superpowers/specs/2026-08-14-foundation-design.md`.
