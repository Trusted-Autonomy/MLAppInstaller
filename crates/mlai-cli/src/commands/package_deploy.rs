use anyhow::{bail, Context, Result};
use mlai_package::deploy::{deploy, DeployOptions};
use mlai_package::profile::DistributionProfile;
use std::fs;
use std::path::{Path, PathBuf};

#[allow(clippy::too_many_arguments)]
pub fn run(
    profile_path: &Path,
    tag: &str,
    files: Vec<PathBuf>,
    draft: bool,
    prerelease: bool,
    notes: &str,
    title: &str,
) -> Result<()> {
    let profile_str = fs::read_to_string(profile_path)
        .with_context(|| format!("reading distribution profile at {}", profile_path.display()))?;
    let profile = DistributionProfile::parse(&profile_str)
        .with_context(|| format!("parsing distribution profile at {}", profile_path.display()))?;

    let Some(deploy_config) = profile.deploy else {
        bail!(
            "distribution profile '{}' has no [deploy] section: nothing to deploy to",
            profile.distribution.name
        );
    };
    let Some(repo) = deploy_config.repo else {
        bail!("distribution profile's [deploy] section has no repo configured");
    };
    if deploy_config.adapter != "github-releases" {
        bail!(
            "unsupported deploy adapter '{}': only 'github-releases' is implemented",
            deploy_config.adapter
        );
    }

    let opts = DeployOptions {
        repo,
        tag: tag.to_string(),
        files,
        draft,
        prerelease,
        notes: notes.to_string(),
        title: title.to_string(),
    };
    deploy(&opts).context("publishing to GitHub Releases")?;
    println!("Published {} to {}", opts.tag, opts.repo);
    Ok(())
}
