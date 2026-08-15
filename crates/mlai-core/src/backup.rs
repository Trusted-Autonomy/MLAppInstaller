use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("component directory not found at {0}")]
    ComponentMissing(PathBuf),
    #[error("backup I/O failure at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

fn backups_dir(install_root: &Path) -> PathBuf {
    install_root.join(".mlai-install").join("backups")
}

pub fn backup_component(
    install_root: &Path,
    component_name: &str,
    timestamp: &str,
) -> Result<PathBuf, BackupError> {
    let component_dir = install_root.join(component_name);
    if !component_dir.exists() {
        return Err(BackupError::ComponentMissing(component_dir));
    }
    let dest = backups_dir(install_root)
        .join(timestamp)
        .join(component_name);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|source| BackupError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    copy_dir_recursive(&component_dir, &dest)?;
    Ok(dest)
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), BackupError> {
    fs::create_dir_all(dest).map_err(|source| BackupError::Io {
        path: dest.to_path_buf(),
        source,
    })?;
    for entry in fs::read_dir(src).map_err(|source| BackupError::Io {
        path: src.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| BackupError::Io {
            path: src.to_path_buf(),
            source,
        })?;
        let file_type = entry.file_type().map_err(|source| BackupError::Io {
            path: entry.path(),
            source,
        })?;
        let dest_path = dest.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            fs::copy(entry.path(), &dest_path).map_err(|source| BackupError::Io {
                path: entry.path(),
                source,
            })?;
        }
    }
    Ok(())
}

pub fn prune_backups(install_root: &Path, keep: usize) -> Result<(), BackupError> {
    let dir = backups_dir(install_root);
    if !dir.exists() {
        return Ok(());
    }
    let mut timestamps: Vec<PathBuf> = fs::read_dir(&dir)
        .map_err(|source| BackupError::Io {
            path: dir.clone(),
            source,
        })?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    timestamps.sort();
    if timestamps.len() > keep {
        let to_remove = timestamps.len() - keep;
        for path in &timestamps[..to_remove] {
            fs::remove_dir_all(path).map_err(|source| BackupError::Io {
                path: path.clone(),
                source,
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn backup_component_copies_directory_contents() {
        let root = tempdir().unwrap();
        let comp_dir = root.path().join("hello-component");
        fs::create_dir_all(comp_dir.join("nested")).unwrap();
        fs::write(comp_dir.join("file.txt"), b"v1").unwrap();
        fs::write(comp_dir.join("nested/inner.txt"), b"inner").unwrap();

        let dest = backup_component(root.path(), "hello-component", "2026-08-14T00-00-00").unwrap();

        assert_eq!(fs::read_to_string(dest.join("file.txt")).unwrap(), "v1");
        assert_eq!(
            fs::read_to_string(dest.join("nested/inner.txt")).unwrap(),
            "inner"
        );
    }

    #[test]
    fn backup_component_errors_when_component_missing() {
        let root = tempdir().unwrap();
        let err = backup_component(root.path(), "missing", "ts").unwrap_err();
        assert!(matches!(err, BackupError::ComponentMissing(_)));
    }

    #[test]
    fn prune_backups_keeps_only_the_newest_n() {
        let root = tempdir().unwrap();
        for ts in [
            "2026-08-01T00-00-00",
            "2026-08-02T00-00-00",
            "2026-08-03T00-00-00",
            "2026-08-04T00-00-00",
        ] {
            fs::create_dir_all(root.path().join(".mlai-install").join("backups").join(ts)).unwrap();
        }
        prune_backups(root.path(), 3).unwrap();
        let remaining: Vec<_> = fs::read_dir(root.path().join(".mlai-install").join("backups"))
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().to_string()))
            .collect();
        assert_eq!(remaining.len(), 3);
        assert!(!remaining.contains(&"2026-08-01T00-00-00".to_string()));
    }
}
