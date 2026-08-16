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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![list_components])
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
}
