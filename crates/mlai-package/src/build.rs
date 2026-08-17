use crate::packager_config::{build_packager_config, packager_format_str};
use crate::profile::{DistributionProfile, Target};
use std::path::Path;
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("failed to launch cargo packager: {source}")]
    Launch {
        #[source]
        source: std::io::Error,
    },
    #[error("cargo packager exited with status {status}")]
    Failed { status: i32 },
}

/// Builds the `cargo packager` invocation for one target, without running
/// it — kept separate from `build_package` so the exact command shape is
/// testable (inspecting `Command::get_args()`) without needing
/// `cargo-packager` installed or a real binary to package.
pub fn packager_command(
    profile: &DistributionProfile,
    target: &Target,
    binary_path: &str,
    out_dir: &Path,
) -> Command {
    let config_json = build_packager_config(profile, target, binary_path);
    let mut cmd = Command::new("cargo");
    cmd.arg("packager")
        .arg("-c")
        .arg(config_json)
        .arg("-f")
        .arg(packager_format_str(&target.format))
        .arg("-o")
        .arg(out_dir)
        .arg("-r");
    cmd
}

pub fn build_package(
    profile: &DistributionProfile,
    target: &Target,
    binary_path: &str,
    out_dir: &Path,
) -> Result<(), BuildError> {
    let status = packager_command(profile, target, binary_path, out_dir)
        .status()
        .map_err(|source| BuildError::Launch { source })?;
    if !status.success() {
        return Err(BuildError::Failed {
            status: status.code().unwrap_or(-1),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::DistributionProfile;
    use std::path::Path;

    fn sample_profile() -> DistributionProfile {
        DistributionProfile::parse(
            r#"
[distribution]
name = "hello-app"
manifest = "manifest.toml"

[[targets]]
platform = "macos"
format = "dmg"
"#,
        )
        .unwrap()
    }

    #[test]
    fn command_invokes_cargo_packager_with_expected_flags() {
        let profile = sample_profile();
        let target = &profile.targets[0];
        let cmd = packager_command(
            &profile,
            target,
            "target/release/hello-app",
            Path::new("dist"),
        );

        assert_eq!(cmd.get_program(), "cargo");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert_eq!(args[0], "packager");
        assert!(args.contains(&"-c".to_string()));
        assert!(args.contains(&"-f".to_string()));
        assert!(args.contains(&"dmg".to_string()));
        assert!(args.contains(&"-o".to_string()));
        assert!(args.contains(&"dist".to_string()));
        assert!(
            args.contains(&"-r".to_string()),
            "release flag must always be passed"
        );
    }

    #[test]
    fn command_config_arg_is_valid_json_matching_the_target() {
        let profile = sample_profile();
        let target = &profile.targets[0];
        let cmd = packager_command(
            &profile,
            target,
            "target/release/hello-app",
            Path::new("dist"),
        );

        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        let config_index = args.iter().position(|a| a == "-c").unwrap();
        let config_json = &args[config_index + 1];
        let value: serde_json::Value =
            serde_json::from_str(config_json).expect("must be valid JSON");
        assert_eq!(value["productName"], "hello-app");
    }
}
