use mlai_core::manifest::Manifest;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// Reads and parses a manifest.toml at an exact path. Separated from
/// `list_components` so the parsing logic is testable without a real Tauri
/// AppHandle (which needs a running app context to construct).
fn read_manifest_at(path: &Path) -> Result<Manifest, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("reading manifest at {}: {e}", path.display()))?;
    Manifest::parse(&content).map_err(|e| format!("parsing manifest at {}: {e}", path.display()))
}

/// Locates a bundled resource, preferring the production resource dir
/// (declared in tauri.conf.json's `bundle.resources`) and falling back to
/// the workspace root for `tauri dev` (CARGO_MANIFEST_DIR is
/// crates/mlai-gui/src-tauri; the repo root with manifest.toml is three
/// levels up).
fn find_resource(app: &AppHandle, relative: &str) -> Option<PathBuf> {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let p = resource_dir.join(relative);
        if p.exists() {
            return Some(p);
        }
    }
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join(relative);
    if dev_path.exists() {
        return Some(dev_path);
    }
    None
}

#[tauri::command]
fn list_components(app: AppHandle) -> Result<Manifest, String> {
    let manifest_path = find_resource(&app, "manifest.toml")
        .ok_or_else(|| "manifest.toml not found".to_string())?;
    read_manifest_at(&manifest_path)
}

#[tauri::command]
fn default_install_root() -> String {
    mlai_core::paths::default_install_root()
        .to_string_lossy()
        .to_string()
}

#[tauri::command]
fn read_install_status(
    install_root: Option<String>,
) -> Result<mlai_core::state::InstalledState, String> {
    let root = install_root
        .filter(|r| !r.trim().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(mlai_core::paths::default_install_root);
    mlai_core::state::InstalledState::load(&root).map_err(|e| e.to_string())
}

/// Probes a named component's setup command for the backend-options
/// protocol, gated the same way mlai-cli gates it: only when the manifest
/// declares support for the current OS. Returns `None` (not an error) for
/// an unknown component name, a component that doesn't support the
/// protocol, or a probe that fails — all graceful-degradation cases the
/// frontend already renders as "no options for this component."
fn describe_options_for(
    manifest: &Manifest,
    component_name: &str,
    component_dir: &Path,
) -> Option<mlai_core::options_protocol::OptionsDescriptor> {
    let component = manifest.find_component(component_name)?;
    if !component.supports_options_protocol_for_current_os() {
        return None;
    }
    let setup = component.setup_for_current_os()?;
    mlai_core::options_protocol::describe_options(
        setup,
        component_dir,
        std::time::Duration::from_secs(10),
    )
    .ok()
}

#[tauri::command]
fn describe_component_options(
    app: AppHandle,
    component: String,
    install_root: Option<String>,
) -> Result<Option<mlai_core::options_protocol::OptionsDescriptor>, String> {
    let manifest_path = find_resource(&app, "manifest.toml")
        .ok_or_else(|| "manifest.toml not found".to_string())?;
    let manifest = read_manifest_at(&manifest_path)?;
    let root = install_root
        .filter(|r| !r.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(mlai_core::paths::default_install_root);
    let component_dir = root.join(&component);
    Ok(describe_options_for(&manifest, &component, &component_dir))
}

use mlai_core::fetch::HttpFetcher;
use mlai_core::pipeline::{install_component, repair_component, PipelineOptions};
use mlai_core::state::ComponentState;
use serde::Serialize;
use std::collections::HashMap;
use tauri::Emitter;

#[derive(Debug, Serialize, Clone, PartialEq)]
struct ComponentResult {
    name: String,
    outcome: String,
    message: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
struct InstallDone {
    success: bool,
    message: String,
}

fn summarize_results(results: &[ComponentResult]) -> InstallDone {
    let failures: Vec<String> = results
        .iter()
        .filter(|r| r.outcome == "failed")
        .map(|r| {
            format!(
                "{}: {}",
                r.name,
                r.message.as_deref().unwrap_or("unknown error")
            )
        })
        .collect();
    if failures.is_empty() {
        InstallDone {
            success: true,
            message: "Install finished successfully.".to_string(),
        }
    } else {
        InstallDone {
            success: false,
            message: format!("Finished with warnings -- {}", failures.join("; ")),
        }
    }
}

fn run_install_inner(
    app: &AppHandle,
    components: Vec<String>,
    install_root: Option<String>,
    mode: &str,
    options: HashMap<String, HashMap<String, String>>,
) -> Result<Vec<ComponentResult>, String> {
    let manifest_path =
        find_resource(app, "manifest.toml").ok_or_else(|| "manifest.toml not found".to_string())?;
    let manifest = read_manifest_at(&manifest_path)?;
    let root = install_root
        .filter(|r| !r.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(mlai_core::paths::default_install_root);
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;

    let fetcher = HttpFetcher {
        token: std::env::var("MLAI_TOKEN").ok(),
    };
    let mut results = Vec::new();

    for name in components {
        let Some(component) = manifest.find_component(&name) else {
            results.push(ComponentResult {
                name: name.clone(),
                outcome: "failed".to_string(),
                message: Some("no component with this name in the manifest".to_string()),
            });
            continue;
        };
        let _ = app.emit("install-log", format!("{}: starting ({mode})", name));

        let set_options: Vec<(String, String)> = options
            .get(&name)
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        let opts = PipelineOptions {
            install_root: root.clone(),
            fetcher: &fetcher,
            version: component.component_ref.clone(),
            backup_keep: 3,
            set_options,
            force: mode == "force",
        };

        let result = if mode == "repair" {
            repair_component(component, &manifest, &opts).map(|(state, reinstalled)| {
                let outcome = match (state, reinstalled) {
                    (ComponentState::Healthy, false) => "already_healthy",
                    (ComponentState::Healthy, true) => "repaired",
                    (_, _) => "needs_attention",
                };
                ComponentResult {
                    name: name.clone(),
                    outcome: outcome.to_string(),
                    message: None,
                }
            })
        } else {
            install_component(component, &manifest, &opts).map(|state| {
                let outcome = match state {
                    ComponentState::Healthy => "healthy",
                    _ => "needs_attention",
                };
                ComponentResult {
                    name: name.clone(),
                    outcome: outcome.to_string(),
                    message: None,
                }
            })
        };

        let component_result = result.unwrap_or_else(|e| ComponentResult {
            name: name.clone(),
            outcome: "failed".to_string(),
            message: Some(e.to_string()),
        });
        let _ = app.emit(
            "install-log",
            format!("{}: {}", component_result.name, component_result.outcome),
        );
        results.push(component_result);
    }

    Ok(results)
}

#[tauri::command]
fn run_install(
    app: AppHandle,
    components: Vec<String>,
    install_root: Option<String>,
    mode: String,
    options: HashMap<String, HashMap<String, String>>,
) -> Result<(), String> {
    std::thread::spawn(move || {
        let done = match run_install_inner(&app, components, install_root, &mode, options) {
            Ok(results) => summarize_results(&results),
            Err(e) => InstallDone {
                success: false,
                message: e,
            },
        };
        let _ = app.emit("install-done", done);
    });
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            list_components,
            default_install_root,
            read_install_status,
            describe_component_options,
            run_install
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn list_components_reads_and_parses_a_bundled_manifest() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("manifest.toml");
        fs::write(
            &manifest_path,
            r#"
manifest_version = "1.0.0"

[[components]]
name = "hello-component"
source_url = "https://example.com/hello-component.zip"
ref = "main"
default = true
"#,
        )
        .unwrap();

        let manifest = read_manifest_at(&manifest_path).unwrap();
        assert_eq!(manifest.components.len(), 1);
        assert_eq!(manifest.components[0].name, "hello-component");
    }

    #[test]
    fn read_manifest_at_reports_a_clear_error_for_a_missing_file() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("nope.toml");
        let err = read_manifest_at(&missing).unwrap_err();
        assert!(
            err.contains("nope.toml"),
            "error should name the missing path: {err}"
        );
    }

    #[test]
    fn options_for_a_component_are_none_when_the_component_declares_no_support() {
        use mlai_core::manifest::{
            Component, Manifest, PlatformFlag, PlatformHealth, PlatformSetup,
        };

        let manifest = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![Component {
                name: "hello-component".into(),
                source_url: "https://example.com/hello-component.zip".into(),
                component_ref: "main".into(),
                default: true,
                setup: PlatformSetup::default(),
                health: PlatformHealth::default(),
                supports_options_protocol: PlatformFlag::default(),
            }],
            removals: vec![],
        };

        let result = describe_options_for(&manifest, "hello-component", Path::new("."));
        assert_eq!(result, None);
    }

    #[test]
    fn options_for_an_unknown_component_name_is_none_not_an_error() {
        let manifest = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![],
            removals: vec![],
        };
        let result = describe_options_for(&manifest, "nonexistent", Path::new("."));
        assert_eq!(result, None);
    }

    #[test]
    fn summarize_results_reports_success_when_everything_is_healthy() {
        let results = vec![
            ComponentResult {
                name: "a".into(),
                outcome: "healthy".into(),
                message: None,
            },
            ComponentResult {
                name: "b".into(),
                outcome: "already_healthy".into(),
                message: None,
            },
        ];
        let done = summarize_results(&results);
        assert!(done.success);
        assert_eq!(done.message, "Install finished successfully.");
    }

    #[test]
    fn summarize_results_reports_failure_with_component_names() {
        let results = vec![
            ComponentResult {
                name: "a".into(),
                outcome: "healthy".into(),
                message: None,
            },
            ComponentResult {
                name: "b".into(),
                outcome: "failed".into(),
                message: Some("network error".into()),
            },
        ];
        let done = summarize_results(&results);
        assert!(!done.success);
        assert!(done.message.contains("b: network error"));
    }
}
