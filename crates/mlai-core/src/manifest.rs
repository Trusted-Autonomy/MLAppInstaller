use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Manifest {
    pub manifest_version: String,
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Component {
    pub name: String,
    pub source_url: String,
    #[serde(rename = "ref")]
    pub component_ref: String,
    #[serde(default)]
    pub default: bool,
    pub setup: Option<SetupCommand>,
    pub health: Option<HealthCheck>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SetupCommand {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HealthCheck {
    FileExists { path: String },
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("failed to parse manifest TOML: {0}")]
    Parse(#[from] toml::de::Error),
}

impl Manifest {
    pub fn parse(toml_str: &str) -> Result<Manifest, ManifestError> {
        toml::from_str(toml_str).map_err(ManifestError::from)
    }

    pub fn default_components(&self) -> Vec<&Component> {
        self.components.iter().filter(|c| c.default).collect()
    }

    pub fn find_component(&self, name: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
manifest_version = "1.0.0"

[[components]]
name = "hello-component"
source_url = "https://example.com/hello-component.zip"
ref = "main"
default = true

[components.setup]
command = "setup.sh"
args = []

[components.health]
type = "file_exists"
path = "marker.txt"
"#;

    #[test]
    fn parses_a_component_with_setup_and_health() {
        let manifest = Manifest::parse(SAMPLE).expect("valid manifest");
        assert_eq!(manifest.manifest_version, "1.0.0");
        assert_eq!(manifest.components.len(), 1);
        let c = &manifest.components[0];
        assert_eq!(c.name, "hello-component");
        assert!(c.default);
        assert_eq!(c.component_ref, "main");
        assert_eq!(c.setup.as_ref().unwrap().command, "setup.sh");
        assert_eq!(
            c.health.as_ref().unwrap(),
            &HealthCheck::FileExists {
                path: "marker.txt".into()
            }
        );
    }

    #[test]
    fn find_component_returns_none_for_unknown_name() {
        let manifest = Manifest::parse(SAMPLE).unwrap();
        assert!(manifest.find_component("nope").is_none());
    }

    #[test]
    fn default_components_filters_on_default_flag() {
        let manifest = Manifest::parse(SAMPLE).unwrap();
        assert_eq!(manifest.default_components().len(), 1);
    }

    #[test]
    fn rejects_invalid_toml() {
        let err = Manifest::parse("not valid toml [[[").unwrap_err();
        assert!(matches!(err, ManifestError::Parse(_)));
    }
}
