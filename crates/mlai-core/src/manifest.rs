use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Manifest {
    pub manifest_version: String,
    pub components: Vec<Component>,
    #[serde(default)]
    pub removals: Vec<RemovalEntry>,
    #[serde(default)]
    pub gui: GuiConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
pub struct GuiConfig {
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub app_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
pub struct PlatformSetup {
    #[serde(default)]
    pub windows: Option<SetupCommand>,
    #[serde(default)]
    pub posix: Option<SetupCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
pub struct PlatformHealth {
    #[serde(default)]
    pub windows: Option<HealthCheck>,
    #[serde(default)]
    pub posix: Option<HealthCheck>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
pub struct PlatformFlag {
    #[serde(default)]
    pub windows: bool,
    #[serde(default)]
    pub posix: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Component {
    pub name: String,
    pub source_url: String,
    #[serde(rename = "ref")]
    pub component_ref: String,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub setup: PlatformSetup,
    #[serde(default)]
    pub health: PlatformHealth,
    #[serde(default)]
    pub supports_options_protocol: PlatformFlag,
    #[serde(default)]
    pub binds_to_project_type: Option<String>,
}

impl Component {
    /// This platform's setup command, or `None` if this component has no
    /// setup script for the OS `mlai` is running on.
    pub fn setup_for_current_os(&self) -> Option<&SetupCommand> {
        if cfg!(target_os = "windows") {
            self.setup.windows.as_ref()
        } else {
            self.setup.posix.as_ref()
        }
    }

    pub fn health_for_current_os(&self) -> Option<&HealthCheck> {
        if cfg!(target_os = "windows") {
            self.health.windows.as_ref()
        } else {
            self.health.posix.as_ref()
        }
    }

    pub fn supports_options_protocol_for_current_os(&self) -> bool {
        if cfg!(target_os = "windows") {
            self.supports_options_protocol.windows
        } else {
            self.supports_options_protocol.posix
        }
    }
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct RemovalEntry {
    pub version: String,
    pub paths: Vec<String>,
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

[components.setup.posix]
command = "setup.sh"
args = []

[components.health.posix]
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
        assert_eq!(c.setup.posix.as_ref().unwrap().command, "setup.sh");
        assert!(c.setup.windows.is_none());
        assert_eq!(
            c.health.posix.as_ref().unwrap(),
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

    #[test]
    fn supports_options_protocol_defaults_to_false_when_absent() {
        let manifest = Manifest::parse(SAMPLE).unwrap();
        assert!(!manifest.components[0].supports_options_protocol_for_current_os());
    }

    #[test]
    fn supports_options_protocol_parses_when_present() {
        // Both platforms declared true so this assertion holds regardless of
        // which OS actually runs the test — a single-platform value would
        // make this test's expected outcome depend on the CI runner's OS.
        let toml = r#"
manifest_version = "1.0.0"

[[components]]
name = "hello-component"
source_url = "https://example.com/hello-component.zip"
ref = "main"
default = true

[components.supports_options_protocol]
posix = true
windows = true
"#;
        let manifest = Manifest::parse(toml).unwrap();
        assert!(manifest.components[0].supports_options_protocol_for_current_os());
    }

    #[test]
    fn setup_for_current_os_selects_only_the_matching_platform_entry() {
        let toml = r#"
manifest_version = "1.0.0"

[[components]]
name = "hello-component"
source_url = "https://example.com/hello-component.zip"
ref = "main"
default = true

[components.setup.windows]
command = "powershell"
args = ["-File", "setup.ps1"]
"#;
        let manifest = Manifest::parse(toml).unwrap();
        let setup = manifest.components[0].setup_for_current_os();
        // The manifest declares ONLY a windows entry — on Windows it must be
        // selected, on any other OS it must not leak across the platform
        // boundary. Runs correctly on every CI platform, unlike a test that
        // hardcodes an assumption about which OS it happens to run on.
        if cfg!(target_os = "windows") {
            assert_eq!(setup.unwrap().command, "powershell");
        } else {
            assert!(setup.is_none());
        }
    }

    #[test]
    fn removals_default_to_empty_when_absent() {
        let manifest = Manifest::parse(SAMPLE).unwrap();
        assert!(manifest.removals.is_empty());
    }

    #[test]
    fn removals_parse_when_present() {
        let toml = r#"
manifest_version = "1.1.0"

[[components]]
name = "hello-component"
source_url = "https://example.com/hello-component.zip"
ref = "main"
default = true

[[removals]]
version = "1.1.0"
paths = ["hello-component/legacy_tool.py"]
"#;
        let manifest = Manifest::parse(toml).unwrap();
        assert_eq!(manifest.removals.len(), 1);
        assert_eq!(manifest.removals[0].version, "1.1.0");
        assert_eq!(
            manifest.removals[0].paths,
            vec!["hello-component/legacy_tool.py"]
        );
    }

    #[test]
    fn component_parses_binds_to_project_type_when_present() {
        let toml = r#"
        manifest_version = "1.0.0"

        [[components]]
        name = "ue5-plugin"
        source_url = "https://example.com/ue5-plugin.zip"
        ref = "main"
        binds_to_project_type = "UE5"

        [components.setup.posix]
        command = "./install.sh"
        args = ["-Project", "{project}"]
    "#;
        let manifest = Manifest::parse(toml).unwrap();
        assert_eq!(
            manifest.components[0].binds_to_project_type.as_deref(),
            Some("UE5")
        );
    }

    #[test]
    fn component_binds_to_project_type_defaults_to_none_when_absent() {
        let manifest = Manifest::parse(SAMPLE).unwrap();
        assert_eq!(manifest.components[0].binds_to_project_type, None);
    }

    #[test]
    fn gui_theme_parses_when_present() {
        let toml = r#"
        manifest_version = "1.0.0"

        [[components]]
        name = "hello-component"
        source_url = "https://example.com/hello-component.zip"
        ref = "main"

        [gui]
        theme = "dark"
    "#;
        let manifest = Manifest::parse(toml).unwrap();
        assert_eq!(manifest.gui.theme, Theme::Dark);
    }

    #[test]
    fn gui_theme_defaults_to_system_when_gui_table_is_absent() {
        let manifest = Manifest::parse(SAMPLE).unwrap();
        assert_eq!(manifest.gui.theme, Theme::System);
    }

    #[test]
    fn theme_serializes_to_snake_case_json() {
        assert_eq!(serde_json::to_string(&Theme::System).unwrap(), "\"system\"");
        assert_eq!(serde_json::to_string(&Theme::Light).unwrap(), "\"light\"");
        assert_eq!(serde_json::to_string(&Theme::Dark).unwrap(), "\"dark\"");
    }

    #[test]
    fn gui_app_name_parses_when_present() {
        let toml = r#"
        manifest_version = "1.0.0"

        [[components]]
        name = "hello-component"
        source_url = "https://example.com/hello-component.zip"
        ref = "main"

        [gui]
        app_name = "Example Studio Installer"
    "#;
        let manifest = Manifest::parse(toml).unwrap();
        assert_eq!(
            manifest.gui.app_name.as_deref(),
            Some("Example Studio Installer")
        );
    }

    #[test]
    fn gui_app_name_defaults_to_none_when_absent() {
        let manifest = Manifest::parse(SAMPLE).unwrap();
        assert_eq!(manifest.gui.app_name, None);
    }
}
