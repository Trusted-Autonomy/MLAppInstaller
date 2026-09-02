use anyhow::{bail, Context, Result};
use mlai_core::fetch::HttpFetcher;
use mlai_core::manifest::Manifest;
use mlai_core::pipeline::{install_component, PipelineOptions};
use mlai_core::state::ComponentState;
use std::fs;
use std::path::Path;

pub fn run(
    manifest_path: &Path,
    install_root: &Path,
    component_name: Option<&str>,
    set_options: &[(String, String)],
    force: bool,
) -> Result<()> {
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
        bail!("no components to install (manifest has no default components and none were named)");
    }

    fs::create_dir_all(install_root)
        .with_context(|| format!("creating install root at {}", install_root.display()))?;

    let fetcher = HttpFetcher {
        token: std::env::var("MLAI_TOKEN").ok(),
    };

    for component in components {
        if !set_options.is_empty() && !component.supports_options_protocol_for_current_os() {
            bail!(
                "--set was provided but component '{}' does not declare supports_options_protocol = true in the manifest",
                component.name
            );
        }
        println!("Installing {}...", component.name);
        let opts = PipelineOptions {
            install_root: install_root.to_path_buf(),
            fetcher: &fetcher,
            version: component.component_ref.clone(),
            backup_keep: 3,
            set_options: set_options.to_vec(),
            force,
        };
        let result = install_component(component, &manifest, &opts)
            .with_context(|| format!("installing component '{}'", component.name))?;
        match result {
            ComponentState::Healthy => println!("  {} -> healthy", component.name),
            ComponentState::AwaitingProjectBinding => println!(
                "  {} -> installed, awaiting project binding (run `mlai bind-project`)",
                component.name
            ),
            other => println!("  {} -> {other:?} (NEEDS ATTENTION)", component.name),
        }
    }

    Ok(())
}
