use anyhow::{bail, Context, Result};
use mlai_core::fetch::HttpFetcher;
use mlai_core::manifest::Manifest;
use mlai_core::pipeline::bind_project as core_bind_project;
use mlai_core::state::ComponentState;
use std::fs;
use std::path::Path;

pub fn run(
    manifest_path: &Path,
    install_root: &Path,
    project_type: &str,
    project_path: &Path,
) -> Result<()> {
    let manifest_str = fs::read_to_string(manifest_path)
        .with_context(|| format!("reading manifest at {}", manifest_path.display()))?;
    let manifest = Manifest::parse(&manifest_str)
        .with_context(|| format!("parsing manifest at {}", manifest_path.display()))?;

    let fetcher = HttpFetcher {
        token: std::env::var("MLAI_TOKEN").ok(),
    };

    let results = core_bind_project(
        &manifest,
        install_root,
        &fetcher,
        project_type,
        project_path,
    );

    if results.is_empty() {
        bail!(
            "no installed component declares binds_to_project_type = \"{project_type}\" in {} \
             -- check the manifest's [[components]] entries and confirm the matching component \
             is already installed before binding a project to it",
            manifest_path.display()
        );
    }

    let mut any_failed = false;
    for (name, result) in results {
        match result {
            Ok(ComponentState::Healthy) => println!("  {name} -> bound"),
            Ok(other) => {
                println!("  {name} -> {other:?} (NEEDS ATTENTION)");
                any_failed = true;
            }
            Err(e) => {
                println!("  {name} -> failed: {e}");
                any_failed = true;
            }
        }
    }

    if any_failed {
        bail!("one or more components failed to bind to the project -- see output above");
    }

    Ok(())
}
