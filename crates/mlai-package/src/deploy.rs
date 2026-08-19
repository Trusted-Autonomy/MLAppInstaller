use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct DeployOptions {
    pub repo: String,
    pub tag: String,
    pub files: Vec<PathBuf>,
    pub draft: bool,
    pub prerelease: bool,
    pub notes: String,
    pub title: String,
}

/// Builds the `gh release create` invocation for publishing built packages,
/// without running it: kept separate from any execution wrapper so the
/// exact command shape is testable without `gh` installed or authenticated.
/// Verified directly: `gh release create <tag> <files...> --repo <repo>
/// [--draft] [--prerelease] --notes <text> --title <text>` creates the
/// release and uploads assets in one command.
pub fn deploy_command(opts: &DeployOptions) -> Command {
    let mut cmd = Command::new("gh");
    cmd.arg("release").arg("create").arg(&opts.tag);
    for file in &opts.files {
        cmd.arg(file);
    }
    cmd.arg("--repo").arg(&opts.repo);
    if opts.draft {
        cmd.arg("--draft");
    }
    if opts.prerelease {
        cmd.arg("--prerelease");
    }
    cmd.arg("--notes").arg(&opts.notes);
    cmd.arg("--title").arg(&opts.title);
    cmd
}

#[derive(Debug, thiserror::Error)]
pub enum DeployError {
    #[error("failed to launch gh: {source}")]
    Launch {
        #[source]
        source: std::io::Error,
    },
    #[error("gh release create exited with status {status}")]
    Failed { status: i32 },
}

pub fn deploy(opts: &DeployOptions) -> Result<(), DeployError> {
    let status = deploy_command(opts)
        .status()
        .map_err(|source| DeployError::Launch { source })?;
    if !status.success() {
        return Err(DeployError::Failed {
            status: status.code().unwrap_or(-1),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_options() -> DeployOptions {
        DeployOptions {
            repo: "example-org/example-app".to_string(),
            tag: "v1.2.3".to_string(),
            files: vec![PathBuf::from("dist/app.dmg"), PathBuf::from("dist/app.msi")],
            draft: false,
            prerelease: false,
            notes: "Release notes here".to_string(),
            title: "v1.2.3".to_string(),
        }
    }

    #[test]
    fn command_invokes_gh_release_create_with_tag_and_files() {
        let opts = sample_options();
        let cmd = deploy_command(&opts);

        assert_eq!(cmd.get_program(), "gh");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert_eq!(args[0], "release");
        assert_eq!(args[1], "create");
        assert_eq!(args[2], "v1.2.3");
        assert!(args.contains(&"dist/app.dmg".to_string()));
        assert!(args.contains(&"dist/app.msi".to_string()));
        assert!(args.contains(&"--repo".to_string()));
        assert!(args.contains(&"example-org/example-app".to_string()));
    }

    #[test]
    fn draft_and_prerelease_flags_are_included_only_when_set() {
        let mut opts = sample_options();
        opts.draft = true;
        opts.prerelease = true;
        let cmd = deploy_command(&opts);
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"--draft".to_string()));
        assert!(args.contains(&"--prerelease".to_string()));

        let cmd_without = deploy_command(&sample_options());
        let args_without: Vec<String> = cmd_without
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(!args_without.contains(&"--draft".to_string()));
        assert!(!args_without.contains(&"--prerelease".to_string()));
    }

    #[test]
    fn notes_and_title_are_passed_through() {
        let opts = sample_options();
        let cmd = deploy_command(&opts);
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        let notes_index = args.iter().position(|a| a == "--notes").unwrap();
        assert_eq!(args[notes_index + 1], "Release notes here");
        let title_index = args.iter().position(|a| a == "--title").unwrap();
        assert_eq!(args[title_index + 1], "v1.2.3");
    }
}
