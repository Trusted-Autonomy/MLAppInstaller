# Sample distribution

A real, working example distribution that exercises every install-time feature this project ships: install/repair, per-machine model selection via the model catalog, project binding, and private-labeling. It's what the root `manifest.toml` (the file `mlai-gui` reads in dev mode, and what `mlai install` in this repo's own README examples points at) actually is.

## What it demonstrates

- **Install/repair/health checks** — `sample-transcription-app` and `sample-ue5-plugin` are both real, fetchable components (published as GitHub Release assets — see "Rebuilding the sample components" below), not placeholders.
- **Per-target-machine model selection** (`docs/superpowers/specs/2026-08-15-distribution-packaging-framework-design.md`'s model catalog) — `sample-transcription-app`'s setup script supports the options protocol, and its `--describe-options` handler *actually shells out to `mlai catalog resolve`* against `catalog/trusted-autonomy.toml` (bundled inside its own zip), using an illustrative hardware profile that defaults differently per host OS (Apple Silicon → `parakeet-mlx`, everything else → the NVIDIA/CUDA tier by default; override via `MLAI_SAMPLE_OS`/`MLAI_SAMPLE_GPU_VENDOR`/`MLAI_SAMPLE_VRAM_GB` env vars to see other tiers resolve). This requires the `mlai` CLI to be on `PATH` — the same assumption any real adopter's own setup script makes.
- **Multi-owner catalog merge** — `catalog/trusted-autonomy.toml` (real data from `docs/migration/ta-migration.md`'s worked example) and `catalog/studio-a.toml` (a second, independent owner) declare different purposes with zero coordination between them, demonstrating the ownership-tagged-fragment mechanism directly:

  ```bash
  mlai catalog resolve --purpose voice-transcription \
    --catalog samples/catalog/trusted-autonomy.toml \
    --catalog samples/catalog/studio-a.toml \
    --os macos --gpu-vendor apple --vram-gb 0 --effective-vram-gb 0 --disk-free-gb 50
  ```

- **Project binding** — `sample-ue5-plugin` declares `binds_to_project_type = "UE5"`; after installing, `mlai bind-project --type UE5 --path /path/to/Some.uproject` (or the GUI's "Bind a Project" panel) substitutes the real path into its setup command.
- **Private-labeling** — the root `manifest.toml`'s `[gui]` table sets `app_name`, retitling the GUI window at startup with no rebuild.

## Try it

```bash
cargo build -p mlai-cli
target/debug/mlai install --manifest manifest.toml --install-root /tmp/mlai-demo
target/debug/mlai bind-project --manifest manifest.toml --install-root /tmp/mlai-demo --type UE5 --path /fake/MyGame.uproject
```

Or via the GUI: `cd crates/mlai-gui && npm run tauri dev` (dev mode falls back to the workspace root's `manifest.toml` — this file).

## Layout

- `catalog/` — the two model-catalog fragments referenced above (also duplicated inside `components/sample-transcription-app/catalog/` so it travels with the zip — see below).
- `components/` — the *source* for each sample component's setup scripts, before zipping. Not fetched directly; see below.

## Rebuilding the sample components

The actual `source_url`s in `manifest.toml` point at GitHub Release assets (tag `samples-v1`), not at `components/` directly — `mlai`'s fetcher downloads a real zip over HTTP, so the scripts need to be packaged and hosted somewhere fetchable. To rebuild and republish after editing anything under `components/`:

```bash
rm -rf /tmp/mlai-sample-build
mkdir -p /tmp/mlai-sample-build/sample-transcription-app-main/catalog
mkdir -p /tmp/mlai-sample-build/sample-ue5-plugin-main
cp samples/components/sample-transcription-app/setup.sh samples/components/sample-transcription-app/setup.ps1 \
  /tmp/mlai-sample-build/sample-transcription-app-main/
cp samples/catalog/trusted-autonomy.toml /tmp/mlai-sample-build/sample-transcription-app-main/catalog/
cp samples/components/sample-ue5-plugin/setup.sh samples/components/sample-ue5-plugin/setup.ps1 \
  /tmp/mlai-sample-build/sample-ue5-plugin-main/
chmod +x /tmp/mlai-sample-build/sample-transcription-app-main/setup.sh \
  /tmp/mlai-sample-build/sample-ue5-plugin-main/setup.sh

cd /tmp/mlai-sample-build
zip -r sample-transcription-app.zip sample-transcription-app-main
zip -r sample-ue5-plugin.zip sample-ue5-plugin-main

gh release upload samples-v1 sample-transcription-app.zip sample-ue5-plugin.zip \
  --repo Trusted-Autonomy/MLAppInstaller --clobber
```

(`--clobber` overwrites the existing assets in place rather than requiring a new release tag — fine for sample content that isn't itself semver-covered per `docs/VERSIONING.md`.)
