use anyhow::{bail, Context, Result};
use mlai_package::build::build_package;
use mlai_package::profile::DistributionProfile;
use std::fs;
use std::path::Path;

pub fn build(profile_path: &Path, target_index: usize, binary: &str, out_dir: &Path) -> Result<()> {
    let profile_str = fs::read_to_string(profile_path)
        .with_context(|| format!("reading distribution profile at {}", profile_path.display()))?;
    let profile = DistributionProfile::parse(&profile_str)
        .with_context(|| format!("parsing distribution profile at {}", profile_path.display()))?;

    let Some(target) = profile.targets.get(target_index) else {
        bail!(
            "target index {target_index} out of range - profile '{}' declares {} target(s)",
            profile.distribution.name,
            profile.targets.len()
        );
    };

    fs::create_dir_all(out_dir)
        .with_context(|| format!("creating output directory at {}", out_dir.display()))?;

    println!(
        "Packaging '{}' for {:?}/{:?}...",
        profile.distribution.name, target.platform, target.format
    );
    build_package(&profile, target, binary, out_dir)
        .with_context(|| format!("packaging target index {target_index}"))?;
    println!("Packaged to {}", out_dir.display());
    Ok(())
}
