use anyhow::{bail, Context, Result};
use mlai_core::manifest::Manifest;
use mlai_core::removals::clean_install;
use std::fs;
use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

pub fn run(manifest_path: &Path, install_root: &Path, yes: bool, dry_run: bool) -> Result<()> {
    let manifest_str = fs::read_to_string(manifest_path)
        .with_context(|| format!("reading manifest at {}", manifest_path.display()))?;
    let manifest = Manifest::parse(&manifest_str)
        .with_context(|| format!("parsing manifest at {}", manifest_path.display()))?;
    let component_names: Vec<String> = manifest.components.iter().map(|c| c.name.clone()).collect();

    if !yes && !dry_run {
        confirm_or_bail(install_root)?;
    }

    let removed = clean_install(&component_names, install_root, dry_run);

    if dry_run {
        println!(
            "Would remove {removed} item(s) from {}",
            install_root.display()
        );
    } else {
        println!("Removed {removed} item(s) from {}", install_root.display());
    }
    Ok(())
}

fn confirm_or_bail(install_root: &Path) -> Result<()> {
    if !std::io::stdin().is_terminal() {
        bail!(
            "confirmation required to uninstall {} — pass --yes to proceed non-interactively",
            install_root.display()
        );
    }
    eprint!(
        "This will permanently remove all components under {}. Continue? [y/N] ",
        install_root.display()
    );
    std::io::stderr().flush().ok();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    if answer.trim().eq_ignore_ascii_case("y") {
        Ok(())
    } else {
        bail!("uninstall cancelled");
    }
}
