use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentState {
    Downloaded,
    Unpacked,
    SetupRun,
    Healthy,
    NeedsAttention,
    /// Unpacked but deliberately left without its setup command run: the
    /// component declares `binds_to_project_type` and no project has been
    /// bound to it yet, so its setup args still contain an unsubstituted
    /// `{project}` placeholder. Resolved by `pipeline::bind_project`.
    AwaitingProjectBinding,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ComponentRecord {
    pub version: String,
    #[serde(rename = "ref")]
    pub component_ref: String,
    pub state: ComponentState,
    pub installed_at: String,
    /// Every project path this component has been bound to via
    /// `pipeline::bind_project`, e.g. a UE5 component bound to more than one
    /// `.uproject` on the same machine. `#[serde(default)]` so an
    /// `installed.json` written before this field existed still parses.
    #[serde(default)]
    pub bound_projects: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct InstalledState {
    #[serde(default)]
    pub manifest_version: String,
    #[serde(default)]
    pub components: BTreeMap<String, ComponentRecord>,
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("failed to read installed state at {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write installed state at {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse installed state JSON at {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

impl InstalledState {
    pub fn state_path(install_root: &Path) -> PathBuf {
        install_root.join(".mlai-install").join("installed.json")
    }

    pub fn load(install_root: &Path) -> Result<InstalledState, StateError> {
        let path = Self::state_path(install_root);
        if !path.exists() {
            return Ok(InstalledState::default());
        }
        let contents = std::fs::read_to_string(&path).map_err(|source| StateError::Read {
            path: path.clone(),
            source,
        })?;
        serde_json::from_str(&contents).map_err(|source| StateError::Parse { path, source })
    }

    pub fn save(&self, install_root: &Path) -> Result<(), StateError> {
        let path = Self::state_path(install_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| StateError::Write {
                path: path.clone(),
                source,
            })?;
        }
        let json = serde_json::to_string_pretty(self).expect("InstalledState always serializes");
        std::fs::write(&path, json).map_err(|source| StateError::Write { path, source })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_returns_default_when_no_file_exists() {
        let dir = tempdir().unwrap();
        let state = InstalledState::load(dir.path()).unwrap();
        assert_eq!(state, InstalledState::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempdir().unwrap();
        let mut state = InstalledState {
            manifest_version: "1.0.0".into(),
            ..Default::default()
        };
        state.components.insert(
            "hello-component".into(),
            ComponentRecord {
                version: "abc123".into(),
                component_ref: "main".into(),
                state: ComponentState::Healthy,
                installed_at: "2026-08-14T00:00:00Z".into(),
                bound_projects: Vec::new(),
            },
        );
        state.save(dir.path()).unwrap();

        let loaded = InstalledState::load(dir.path()).unwrap();
        assert_eq!(loaded, state);
    }

    #[test]
    fn component_record_without_bound_projects_field_still_parses() {
        // installed.json written before bound_projects existed.
        let json = r#"{
            "manifest_version": "1.0.0",
            "components": {
                "hello-component": {
                    "version": "abc123",
                    "ref": "main",
                    "state": "healthy",
                    "installed_at": "2026-08-14T00:00:00Z"
                }
            }
        }"#;
        let state: InstalledState = serde_json::from_str(json).unwrap();
        assert_eq!(
            state.components["hello-component"].bound_projects,
            Vec::<String>::new()
        );
    }

    #[test]
    fn state_path_is_under_dot_mlai_install() {
        let dir = tempdir().unwrap();
        let path = InstalledState::state_path(dir.path());
        assert_eq!(
            path,
            dir.path().join(".mlai-install").join("installed.json")
        );
    }
}
