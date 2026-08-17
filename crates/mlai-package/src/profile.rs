use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct DistributionProfile {
    pub distribution: Distribution,
    pub targets: Vec<Target>,
    #[serde(default)]
    pub deploy: Option<DeployConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Distribution {
    pub name: String,
    pub manifest: String,
    #[serde(default)]
    pub components: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Macos,
    Windows,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageFormat {
    Dmg,
    App,
    Msi,
    Nsis,
    Deb,
    Appimage,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Target {
    pub platform: Platform,
    pub format: PackageFormat,
    #[serde(default)]
    pub signing_identity: Option<String>,
    #[serde(default)]
    pub certificate_thumbprint: Option<String>,
    #[serde(default)]
    pub notarize: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct DeployConfig {
    pub adapter: String,
    #[serde(default)]
    pub repo: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("failed to parse distribution profile TOML: {0}")]
    Parse(#[from] toml::de::Error),
}

impl DistributionProfile {
    pub fn parse(toml_str: &str) -> Result<DistributionProfile, ProfileError> {
        toml::from_str(toml_str).map_err(ProfileError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[distribution]
name = "cinepipe-director-suite"
manifest = "manifest.toml"
components = ["cinepipe-director", "ue5-cine-pipeline"]

[[targets]]
platform = "macos"
format = "dmg"
signing_identity = "keychain:Developer ID Application: CinePipeAi, Inc."

[[targets]]
platform = "windows"
format = "msi"
certificate_thumbprint = "AB12CD34EF56"

[deploy]
adapter = "github-releases"
repo = "CinePipeAi/cinepipe-director"
"#;

    #[test]
    fn parses_a_full_profile() {
        let profile = DistributionProfile::parse(SAMPLE).expect("valid profile");
        assert_eq!(profile.distribution.name, "cinepipe-director-suite");
        assert_eq!(profile.distribution.components.len(), 2);
        assert_eq!(profile.targets.len(), 2);

        let macos = &profile.targets[0];
        assert_eq!(macos.platform, Platform::Macos);
        assert_eq!(macos.format, PackageFormat::Dmg);
        assert_eq!(
            macos.signing_identity.as_deref(),
            Some("keychain:Developer ID Application: CinePipeAi, Inc.")
        );
        assert!(macos.certificate_thumbprint.is_none());

        let windows = &profile.targets[1];
        assert_eq!(windows.platform, Platform::Windows);
        assert_eq!(
            windows.certificate_thumbprint.as_deref(),
            Some("AB12CD34EF56")
        );

        let deploy = profile.deploy.expect("deploy config present");
        assert_eq!(deploy.adapter, "github-releases");
        assert_eq!(deploy.repo.as_deref(), Some("CinePipeAi/cinepipe-director"));
    }

    #[test]
    fn deploy_and_components_are_optional() {
        let toml = r#"
[distribution]
name = "minimal"
manifest = "manifest.toml"

[[targets]]
platform = "linux"
format = "deb"
"#;
        let profile = DistributionProfile::parse(toml).unwrap();
        assert!(profile.distribution.components.is_empty());
        assert!(profile.deploy.is_none());
        assert!(profile.targets[0].signing_identity.is_none());
    }

    #[test]
    fn rejects_invalid_toml() {
        let err = DistributionProfile::parse("not valid toml [[[").unwrap_err();
        assert!(matches!(err, ProfileError::Parse(_)));
    }
}
