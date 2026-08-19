// Guarded removals: apply per-manifest-version legacy cleanup and full
// uninstall, with a path guard that resolves a manifest-supplied relative
// path component-by-component so the result can never leave install_root's
// own subtree. This fixes a real prefix-confusion vulnerability class: a
// naive `path.starts_with(root)` check incorrectly accepts a sibling
// directory like "MyAppEvil" when root is "MyApp". This construction-based
// guard is a stronger fix, not just a different implementation of the same
// check.

use crate::manifest::RemovalEntry;
use crate::versioning::compare_version;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// Resolves `install_root` joined with `rel` (untrusted, manifest-supplied)
/// into an absolute path, constructing it component-by-component so the
/// result can never leave `install_root`'s own subtree: a `..` that would
/// pop above the install root is rejected outright, and an absolute path
/// smuggled into `rel` is rejected too.
pub fn safe_target(install_root: &Path, rel: &str) -> Option<PathBuf> {
    let root_canon = install_root.canonicalize().ok()?;
    let root_depth = root_canon.components().count();
    let mut result = root_canon.clone();

    for comp in Path::new(rel).components() {
        match comp {
            std::path::Component::Normal(seg) => result.push(seg),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if result.components().count() <= root_depth {
                    return None; // would escape above the install root
                }
                result.pop();
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => return None,
        }
    }

    if result == root_canon {
        return None; // an empty/no-op rel would target the root itself -- unsafe
    }
    Some(result)
}

fn remove_path(target: &Path) -> std::io::Result<()> {
    if target.is_dir() {
        std::fs::remove_dir_all(target)
    } else {
        std::fs::remove_file(target)
    }
}

/// Applies every `RemovalEntry` whose `version` is strictly newer than
/// `installed_version`. Skipped entirely when `installed_version` is `None`
/// (a fresh install has no legacy to clean). Returns the count of paths
/// actually removed (or that would be removed, under `dry_run`).
pub fn apply_removals(
    removals: &[RemovalEntry],
    installed_version: Option<&str>,
    install_root: &Path,
    dry_run: bool,
) -> usize {
    let Some(installed_version) = installed_version else {
        return 0;
    };
    let mut applied = 0;
    for entry in removals {
        if compare_version(&entry.version, installed_version) != Ordering::Greater {
            continue;
        }
        for rel in &entry.paths {
            let Some(target) = safe_target(install_root, rel) else {
                eprintln!("removal skipped (outside install root): {rel}");
                continue;
            };
            if !target.exists() {
                continue;
            }
            if dry_run {
                eprintln!(
                    "[dry-run] would remove legacy: {rel} (from {})",
                    entry.version
                );
            } else {
                eprintln!("removing legacy: {rel} (deprecated in {})", entry.version);
                let _ = remove_path(&target);
            }
            applied += 1;
        }
    }
    applied
}

/// Full uninstall: removes every named component folder plus
/// `.mlai-install` under `install_root`. Returns the count removed (or
/// that would be removed, under `dry_run`).
pub fn clean_install(component_names: &[String], install_root: &Path, dry_run: bool) -> usize {
    if !install_root.exists() {
        return 0;
    }
    let mut targets: Vec<String> = component_names.to_vec();
    targets.push(".mlai-install".to_string());

    let mut removed = 0;
    for name in &targets {
        let Some(target) = safe_target(install_root, name) else {
            eprintln!("clean skipped (unsafe target): {name}");
            continue;
        };
        if !target.exists() {
            continue;
        }
        if dry_run {
            eprintln!("[dry-run] would UNINSTALL: {name}");
        } else {
            eprintln!("uninstalling: {name}");
            let _ = remove_path(&target);
        }
        removed += 1;
    }
    removed
}

/// Scans `install_root`'s top-level entries and removes anything that is
/// neither a current manifest component name nor a reserved path
/// (`.mlai-install`, `venv`) — a component removed or renamed from the
/// manifest since it was installed. Independent of any particular run's
/// component selection: a whole-install-root reconciliation against what
/// the manifest currently names, not scoped to what's being installed now.
pub fn remove_orphaned_components(
    install_root: &Path,
    known_names: &[String],
    dry_run: bool,
) -> usize {
    if !install_root.exists() {
        return 0;
    }
    const RESERVED: [&str; 2] = [".mlai-install", "venv"];
    let Ok(entries) = std::fs::read_dir(install_root) else {
        return 0;
    };

    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if known_names.iter().any(|k| k == name_str.as_ref())
            || RESERVED.contains(&name_str.as_ref())
        {
            continue;
        }
        let Some(target) = safe_target(install_root, &name_str) else {
            eprintln!("orphan cleanup skipped (unsafe target): {name_str}");
            continue;
        };
        if dry_run {
            eprintln!("[dry-run] would remove orphaned component: {name_str}");
        } else {
            eprintln!("removing orphaned component: {name_str}");
            let _ = remove_path(&target);
        }
        removed += 1;
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::RemovalEntry;
    use std::path::PathBuf;

    fn temp_root(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mlai-removals-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn safe_target_rejects_parent_dir_escape() {
        let root = temp_root("escape");
        assert!(safe_target(&root, "../../etc/passwd").is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn safe_target_rejects_absolute_path() {
        let root = temp_root("absolute");
        assert!(safe_target(&root, "/etc/passwd").is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn safe_target_accepts_a_normal_relative_child() {
        let root = temp_root("normal-child");
        let result = safe_target(&root, "old-component").unwrap();
        assert_eq!(result, root.join("old-component"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn safe_target_accepts_a_nested_relative_child() {
        let root = temp_root("nested-child");
        let result = safe_target(&root, "hello-component/legacy_tool.py").unwrap();
        assert_eq!(result, root.join("hello-component").join("legacy_tool.py"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn safe_target_allows_an_internal_dotdot_that_stays_inside_root() {
        let root = temp_root("internal-dotdot");
        let result = safe_target(&root, "a/../b").unwrap();
        assert_eq!(result, root.join("b"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn safe_target_rejects_empty_rel_as_targeting_root_itself() {
        let root = temp_root("empty-rel");
        assert!(safe_target(&root, "").is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn apply_removals_skipped_entirely_on_fresh_install() {
        let root = temp_root("removals-fresh");
        let legacy = root.join("old-thing");
        std::fs::write(&legacy, "x").unwrap();
        let removals = vec![RemovalEntry {
            version: "1.1.0".to_string(),
            paths: vec!["old-thing".to_string()],
        }];

        let applied = apply_removals(&removals, None, &root, false);

        assert_eq!(applied, 0);
        assert!(legacy.exists(), "fresh install must not touch anything");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn apply_removals_only_applies_entries_newer_than_installed_version() {
        let root = temp_root("removals-versioned");
        std::fs::write(root.join("still-current"), "x").unwrap();
        std::fs::write(root.join("deprecated"), "x").unwrap();
        let removals = vec![
            RemovalEntry {
                version: "1.0.0".to_string(),
                paths: vec!["still-current".to_string()],
            },
            RemovalEntry {
                version: "1.5.0".to_string(),
                paths: vec!["deprecated".to_string()],
            },
        ];

        let applied = apply_removals(&removals, Some("1.2.0"), &root, false);

        assert_eq!(applied, 1);
        assert!(root.join("still-current").exists());
        assert!(!root.join("deprecated").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn apply_removals_dry_run_makes_no_filesystem_changes() {
        let root = temp_root("removals-dry-run");
        std::fs::write(root.join("deprecated"), "x").unwrap();
        let removals = vec![RemovalEntry {
            version: "1.5.0".to_string(),
            paths: vec!["deprecated".to_string()],
        }];

        let applied = apply_removals(&removals, Some("1.2.0"), &root, true);

        assert_eq!(applied, 1, "dry-run still reports what WOULD be removed");
        assert!(
            root.join("deprecated").exists(),
            "dry-run must not delete anything"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn apply_removals_traversal_attempt_is_rejected_not_deleted() {
        let root = temp_root("removals-traversal");
        let sibling_secret = root.parent().unwrap().join(format!(
            "{}-sibling-secret.txt",
            root.file_name().unwrap().to_string_lossy()
        ));
        std::fs::write(&sibling_secret, "do not delete me").unwrap();
        let escape_rel = format!(
            "../{}",
            sibling_secret.file_name().unwrap().to_string_lossy()
        );
        let removals = vec![RemovalEntry {
            version: "1.5.0".to_string(),
            paths: vec![escape_rel],
        }];

        let applied = apply_removals(&removals, Some("1.2.0"), &root, false);

        assert_eq!(applied, 0, "an out-of-root path must not count as applied");
        assert!(
            sibling_secret.exists(),
            "the traversal target must survive untouched"
        );
        std::fs::remove_file(&sibling_secret).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn clean_install_removes_named_components_and_state_dir() {
        let root = temp_root("clean-basic");
        std::fs::create_dir_all(root.join("hello-component")).unwrap();
        std::fs::create_dir_all(root.join("other-component")).unwrap();
        std::fs::create_dir_all(root.join(".mlai-install")).unwrap();
        std::fs::write(root.join("unrelated-file.txt"), "keep me").unwrap();

        let removed = clean_install(
            &["hello-component".to_string(), "other-component".to_string()],
            &root,
            false,
        );

        assert_eq!(removed, 3); // 2 components + .mlai-install
        assert!(!root.join("hello-component").exists());
        assert!(!root.join("other-component").exists());
        assert!(!root.join(".mlai-install").exists());
        assert!(
            root.join("unrelated-file.txt").exists(),
            "clean only touches known targets"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn clean_install_dry_run_makes_no_filesystem_changes() {
        let root = temp_root("clean-dry-run");
        std::fs::create_dir_all(root.join("hello-component")).unwrap();

        let removed = clean_install(&["hello-component".to_string()], &root, true);

        assert_eq!(removed, 1);
        assert!(
            root.join("hello-component").exists(),
            "dry-run must not delete anything"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn clean_install_on_nonexistent_root_is_a_no_op() {
        let root = std::env::temp_dir().join(format!(
            "mlai-removals-test-never-created-{}",
            std::process::id()
        ));
        let removed = clean_install(&["hello-component".to_string()], &root, false);
        assert_eq!(removed, 0);
    }

    #[test]
    fn remove_orphaned_components_removes_a_folder_matching_no_known_name() {
        let root = temp_root("orphan-basic");
        std::fs::create_dir_all(root.join("renamed-old-component")).unwrap();
        std::fs::write(root.join("renamed-old-component").join("data.txt"), "x").unwrap();

        let removed = remove_orphaned_components(&root, &["hello-component".to_string()], false);

        assert_eq!(removed, 1);
        assert!(!root.join("renamed-old-component").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn remove_orphaned_components_leaves_known_and_reserved_paths_alone() {
        let root = temp_root("orphan-known-reserved");
        std::fs::create_dir_all(root.join("hello-component")).unwrap();
        std::fs::create_dir_all(root.join(".mlai-install")).unwrap();
        std::fs::create_dir_all(root.join("venv")).unwrap();

        let removed = remove_orphaned_components(&root, &["hello-component".to_string()], false);

        assert_eq!(removed, 0);
        assert!(root.join("hello-component").exists());
        assert!(root.join(".mlai-install").exists());
        assert!(root.join("venv").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn remove_orphaned_components_dry_run_makes_no_filesystem_changes() {
        let root = temp_root("orphan-dry-run");
        std::fs::create_dir_all(root.join("renamed-old-component")).unwrap();

        let removed = remove_orphaned_components(&root, &["hello-component".to_string()], true);

        assert_eq!(removed, 1, "dry-run still reports what WOULD be removed");
        assert!(
            root.join("renamed-old-component").exists(),
            "dry-run must not delete anything"
        );
        std::fs::remove_dir_all(&root).ok();
    }
}
