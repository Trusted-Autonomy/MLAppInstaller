use anyhow::{bail, Context, Result};
use mlai_core::fetch::HttpFetcher;
use mlai_core::manifest::Manifest;
use mlai_core::pipeline::{repair_component, PipelineOptions};
use mlai_core::state::ComponentState;
use std::fs;
use std::path::Path;

pub fn run(manifest_path: &Path, install_root: &Path, component_name: Option<&str>) -> Result<()> {
    let manifest_str = fs::read_to_string(manifest_path)
        .with_context(|| format!("reading manifest at {}", manifest_path.display()))?;
    let manifest = Manifest::parse(&manifest_str)
        .with_context(|| format!("parsing manifest at {}", manifest_path.display()))?;

    let components: Vec<_> = match component_name {
        Some(name) => match manifest.find_component(name) {
            Some(c) => vec![c],
            None => bail!("no component named '{name}' in {}", manifest_path.display()),
        },
        None => manifest.default_components(),
    };

    if components.is_empty() {
        bail!("no components to repair (manifest has no default components and none were named)");
    }

    let fetcher = HttpFetcher {
        token: std::env::var("MLAI_TOKEN").ok(),
    };

    for component in components {
        println!("Checking {}...", component.name);
        let opts = PipelineOptions {
            install_root: install_root.to_path_buf(),
            fetcher: &fetcher,
            version: component.component_ref.clone(),
            backup_keep: 3,
            set_options: Vec::new(),
            force: false,
        };
        let (state, reinstalled) = repair_component(component, &manifest, &opts)
            .with_context(|| format!("repairing component '{}'", component.name))?;
        match (state, reinstalled) {
            (ComponentState::Healthy, false) => {
                println!("  {} -> already healthy", component.name)
            }
            (ComponentState::Healthy, true) => println!("  {} -> repaired", component.name),
            (other, _) => println!("  {} -> {other:?} (NEEDS ATTENTION)", component.name),
        }
    }

    Ok(())
}
