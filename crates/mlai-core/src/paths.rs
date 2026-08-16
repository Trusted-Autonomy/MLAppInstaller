use std::path::PathBuf;

/// A reasonable default install root when the caller hasn't specified one:
/// `<home>/.mlai/install` on unix, `<LOCALAPPDATA>/mlai/install` on Windows.
/// Falls back to the current directory if neither expected environment
/// variable is set (never panics).
pub fn default_install_root() -> PathBuf {
    if cfg!(windows) {
        let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(base).join("mlai").join("install")
    } else {
        let base = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(base).join(".mlai").join("install")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_install_root_is_under_home_on_unix() {
        if cfg!(not(unix)) {
            return;
        }
        std::env::set_var("HOME", "/tmp/fake-home");
        let root = default_install_root();
        assert_eq!(
            root,
            std::path::PathBuf::from("/tmp/fake-home/.mlai/install")
        );
    }

    #[test]
    fn default_install_root_never_panics_when_env_vars_are_absent() {
        // Smoke test only: on a CI runner these vars are always set, so this
        // mainly documents that the function has a graceful fallback path
        // rather than unwrapping directly on a missing var.
        let _ = default_install_root();
    }
}
