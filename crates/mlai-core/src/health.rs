use crate::manifest::HealthCheck;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
    Healthy,
    NeedsAttention(String),
}

pub fn check_health(component_dir: &Path, health: Option<&HealthCheck>) -> HealthStatus {
    let Some(health) = health else {
        return HealthStatus::Healthy;
    };
    match health {
        HealthCheck::FileExists { path } => {
            let target = component_dir.join(path);
            if target.exists() {
                HealthStatus::Healthy
            } else {
                HealthStatus::NeedsAttention(format!(
                    "expected file not found: {}",
                    target.display()
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::HealthCheck;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn no_health_check_declared_is_healthy() {
        let dir = tempdir().unwrap();
        assert_eq!(check_health(dir.path(), None), HealthStatus::Healthy);
    }

    #[test]
    fn file_exists_passes_when_file_present() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("marker.txt"), b"ok").unwrap();
        let health = HealthCheck::FileExists {
            path: "marker.txt".into(),
        };
        assert_eq!(
            check_health(dir.path(), Some(&health)),
            HealthStatus::Healthy
        );
    }

    #[test]
    fn file_exists_fails_when_file_missing() {
        let dir = tempdir().unwrap();
        let health = HealthCheck::FileExists {
            path: "marker.txt".into(),
        };
        let status = check_health(dir.path(), Some(&health));
        assert!(matches!(status, HealthStatus::NeedsAttention(_)));
    }
}
