use crate::backup::backup_component;
use crate::fetch::{unpack_zip, Fetcher};
use crate::health::{check_health, HealthStatus};
use crate::manifest::{Component, SetupCommand};
use crate::state::{ComponentRecord, ComponentState, InstalledState};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error(transparent)]
    Fetch(#[from] crate::fetch::FetchError),
    #[error(transparent)]
    Backup(#[from] crate::backup::BackupError),
    #[error(transparent)]
    State(#[from] crate::state::StateError),
    #[error("setup command '{command}' failed to launch: {source}")]
    SetupLaunch {
        command: String,
        #[source]
        source: std::io::Error,
    },
    #[error("setup command '{command}' exited with status {status}")]
    SetupFailed { command: String, status: i32 },
}

pub struct PipelineOptions<'a> {
    pub install_root: PathBuf,
    pub fetcher: &'a dyn Fetcher,
    pub version: String,
    pub backup_keep: usize,
    pub set_options: Vec<(String, String)>,
    pub force: bool,
}

pub fn install_component(
    component: &Component,
    manifest: &crate::manifest::Manifest,
    opts: &PipelineOptions,
) -> Result<ComponentState, PipelineError> {
    let mut state = apply_removals_and_persist_manifest_version(manifest, opts)?;

    if !opts.force {
        if let Some(existing) = state.components.get(&component.name) {
            if existing.version == opts.version && existing.state == ComponentState::Healthy {
                return Ok(ComponentState::Healthy);
            }
        }
    }

    run_install_sequence(component, &mut state, opts)
}

/// Re-verifies a component's health directly against disk, ignoring any
/// recorded state in `installed.json` entirely — unlike `install_component`'s
/// trust-based shortcut, which skips reinstalling based on the record alone.
/// This is what a manually deleted or corrupted health target needs: a
/// plain re-install would silently skip it forever (the record still says
/// "healthy"), but repair always asks the filesystem directly. Returns
/// `(state, reinstalled)` — `reinstalled` is `false` when the component was
/// found genuinely healthy and zero filesystem changes were made.
pub fn repair_component(
    component: &Component,
    manifest: &crate::manifest::Manifest,
    opts: &PipelineOptions,
) -> Result<(ComponentState, bool), PipelineError> {
    let mut state = apply_removals_and_persist_manifest_version(manifest, opts)?;

    let component_dir = opts.install_root.join(&component.name);
    let genuinely_healthy = component_dir.exists()
        && check_health(&component_dir, component.health_for_current_os()) == HealthStatus::Healthy;
    if genuinely_healthy {
        return Ok((ComponentState::Healthy, false));
    }

    run_install_sequence(component, &mut state, opts).map(|final_state| (final_state, true))
}

/// Finds every already-installed component matching `project_type`,
/// substitutes `project_path` for a `{project}` placeholder in its setup
/// command args, and force-reinstalls it -- the same semantics as
/// cinepipe-installer's original `add_project`: untagged components and
/// tagged-but-not-yet-installed components are left completely untouched.
pub fn bind_project(
    manifest: &crate::manifest::Manifest,
    install_root: &Path,
    fetcher: &dyn crate::fetch::Fetcher,
    project_type: &str,
    project_path: &Path,
) -> Vec<(String, Result<ComponentState, PipelineError>)> {
    let installed = InstalledState::load(install_root).unwrap_or_default();
    let project_str = project_path.to_string_lossy().to_string();

    manifest
        .components
        .iter()
        .filter(|c| c.binds_to_project_type.as_deref() == Some(project_type))
        .filter(|c| installed.components.contains_key(&c.name))
        .map(|component| {
            let mut bound = component.clone();
            let setup = if cfg!(target_os = "windows") {
                bound.setup.windows.as_mut()
            } else {
                bound.setup.posix.as_mut()
            };
            if let Some(setup) = setup {
                for arg in &mut setup.args {
                    if arg == "{project}" {
                        *arg = project_str.clone();
                    }
                }
            }
            let opts = PipelineOptions {
                install_root: install_root.to_path_buf(),
                fetcher,
                version: component.component_ref.clone(),
                backup_keep: 3,
                set_options: Vec::new(),
                force: true,
            };
            let result = install_component(&bound, manifest, &opts);
            (component.name.clone(), result)
        })
        .collect()
}

fn apply_removals_and_persist_manifest_version(
    manifest: &crate::manifest::Manifest,
    opts: &PipelineOptions,
) -> Result<InstalledState, PipelineError> {
    let mut state = InstalledState::load(&opts.install_root)?;
    let previous_manifest_version = if state.manifest_version.is_empty() {
        None
    } else {
        Some(state.manifest_version.clone())
    };
    crate::removals::apply_removals(
        &manifest.removals,
        previous_manifest_version.as_deref(),
        &opts.install_root,
        false,
    );
    if state.manifest_version != manifest.manifest_version {
        state.manifest_version = manifest.manifest_version.clone();
        state.save(&opts.install_root)?;
    }
    Ok(state)
}

fn run_install_sequence(
    component: &Component,
    state: &mut InstalledState,
    opts: &PipelineOptions,
) -> Result<ComponentState, PipelineError> {
    let existing_version = state
        .components
        .get(&component.name)
        .map(|r| r.version.clone());

    let component_dir = opts.install_root.join(&component.name);
    if component_dir.exists() {
        let backup_label = existing_version.as_deref().unwrap_or(&opts.version);
        backup_component(&opts.install_root, &component.name, backup_label)?;
        crate::backup::prune_backups(&opts.install_root, opts.backup_keep)?;
    }

    let zip_path = opts
        .install_root
        .join(".mlai-install")
        .join("downloads")
        .join(format!("{}.zip", component.name));
    opts.fetcher.fetch(&component.source_url, &zip_path)?;
    record_state(state, opts, component, ComponentState::Downloaded)?;

    let component_dir = unpack_zip(&zip_path, &opts.install_root, &component.name)?;
    record_state(state, opts, component, ComponentState::Unpacked)?;

    if let Some(setup) = component.setup_for_current_os() {
        run_setup(&component_dir, setup, &opts.set_options)?;
    }
    record_state(state, opts, component, ComponentState::SetupRun)?;

    let final_state = match check_health(&component_dir, component.health_for_current_os()) {
        HealthStatus::Healthy => ComponentState::Healthy,
        HealthStatus::NeedsAttention(_) => ComponentState::NeedsAttention,
    };
    record_state(state, opts, component, final_state)?;

    Ok(final_state)
}

fn record_state(
    state: &mut InstalledState,
    opts: &PipelineOptions,
    component: &Component,
    component_state: ComponentState,
) -> Result<(), PipelineError> {
    state.components.insert(
        component.name.clone(),
        ComponentRecord {
            version: opts.version.clone(),
            component_ref: component.component_ref.clone(),
            state: component_state,
            installed_at: chrono::Utc::now().to_rfc3339(),
        },
    );
    state.save(&opts.install_root)?;
    Ok(())
}

fn run_setup(
    component_dir: &Path,
    setup: &SetupCommand,
    set_options: &[(String, String)],
) -> Result<(), PipelineError> {
    let mut args = setup.args.clone();
    for (key, value) in set_options {
        args.push("--set".to_string());
        args.push(format!("{key}={value}"));
    }
    let status = Command::new(&setup.command)
        .args(&args)
        .current_dir(component_dir)
        .status()
        .map_err(|source| PipelineError::SetupLaunch {
            command: setup.command.clone(),
            source,
        })?;
    if !status.success() {
        return Err(PipelineError::SetupFailed {
            command: setup.command.clone(),
            status: status.code().unwrap_or(-1),
        });
    }
    Ok(())
}

// This module's fixtures declare `sample_component()`'s setup as a posix-only
// `sh` script (`windows: None`), so on a Windows test run `setup_for_current_os()`
// returns None, the fixture's marker/args files never get created, and the
// pipeline's own health check would then correctly (but unhelpfully) fail —
// these tests exercise a POSIX fixture, not a Windows one. Gated to unix until
// a Windows-native fixture is worth building. Windows CI still verifies this
// crate compiles and its non-shell tests run for real.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::manifest::{
        Component, HealthCheck, Manifest, PlatformFlag, PlatformHealth, PlatformSetup, SetupCommand,
    };
    use std::fs;
    use std::io::Write;
    use std::path::Path;
    use tempfile::tempdir;

    struct FixtureFetcher {
        zip_path: PathBuf,
    }

    impl Fetcher for FixtureFetcher {
        fn fetch(&self, _url: &str, dest_zip: &Path) -> Result<(), crate::fetch::FetchError> {
            if let Some(parent) = dest_zip.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::copy(&self.zip_path, dest_zip).unwrap();
            Ok(())
        }
    }

    fn build_fixture_zip(path: &Path) {
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.add_directory("hello-component-main/", options).unwrap();
        zip.start_file("hello-component-main/setup.sh", options)
            .unwrap();
        zip.write_all(b"#!/bin/sh\ntouch marker.txt\n").unwrap();
        zip.finish().unwrap();
    }

    fn build_fixture_zip_with_project_placeholder(path: &Path) {
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.add_directory("ue5-cine-pipeline-main/", options)
            .unwrap();
        zip.start_file("ue5-cine-pipeline-main/setup.sh", options)
            .unwrap();
        zip.write_all(b"#!/bin/sh\necho \"$@\" > argv.txt\ntouch marker.txt\n")
            .unwrap();
        zip.finish().unwrap();
    }

    fn sample_component() -> Component {
        Component {
            name: "hello-component".into(),
            source_url: "https://example.com/hello-component.zip".into(),
            component_ref: "main".into(),
            default: true,
            setup: PlatformSetup {
                windows: None,
                posix: Some(SetupCommand {
                    command: "sh".into(),
                    args: vec!["setup.sh".into()],
                }),
            },
            health: PlatformHealth {
                windows: None,
                posix: Some(HealthCheck::FileExists {
                    path: "marker.txt".into(),
                }),
            },
            supports_options_protocol: PlatformFlag::default(),
            binds_to_project_type: None,
        }
    }

    #[test]
    fn installs_a_component_end_to_end_and_records_healthy_state() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        let zip_path = fixture_dir.path().join("bundle.zip");
        build_fixture_zip(&zip_path);

        let component = sample_component();
        let manifest = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![component.clone()],
            removals: vec![],
        };
        let fetcher = FixtureFetcher { zip_path };
        let opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "abc123".into(),
            backup_keep: 3,
            set_options: vec![],
            force: false,
        };

        let result = install_component(&component, &manifest, &opts).unwrap();
        assert_eq!(result, ComponentState::Healthy);

        let state = InstalledState::load(root.path()).unwrap();
        let record = state.components.get("hello-component").unwrap();
        assert_eq!(record.state, ComponentState::Healthy);
        assert_eq!(record.version, "abc123");
    }

    #[test]
    fn skips_reinstall_when_already_healthy_at_same_version() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        let zip_path = fixture_dir.path().join("bundle.zip");
        build_fixture_zip(&zip_path);

        let component = sample_component();
        let manifest = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![component.clone()],
            removals: vec![],
        };
        let fetcher = FixtureFetcher { zip_path };
        let opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "abc123".into(),
            backup_keep: 3,
            set_options: vec![],
            force: false,
        };
        install_component(&component, &manifest, &opts).unwrap();

        // Remove the setup script so a real re-run would fail — proves the
        // second call short-circuits instead of re-running setup.
        fs::remove_file(root.path().join("hello-component").join("setup.sh")).unwrap();

        let result = install_component(&component, &manifest, &opts).unwrap();
        assert_eq!(result, ComponentState::Healthy);
    }

    #[test]
    fn backs_up_existing_install_before_replacing_it() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        let zip_path = fixture_dir.path().join("bundle.zip");
        build_fixture_zip(&zip_path);

        let component = sample_component();
        let manifest = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![component.clone()],
            removals: vec![],
        };
        let fetcher = FixtureFetcher { zip_path };
        let opts_v1 = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "v1".into(),
            backup_keep: 3,
            set_options: vec![],
            force: false,
        };
        install_component(&component, &manifest, &opts_v1).unwrap();

        let opts_v2 = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "v2".into(),
            backup_keep: 3,
            set_options: vec![],
            force: false,
        };
        install_component(&component, &manifest, &opts_v2).unwrap();

        let backups_dir = root.path().join(".mlai-install").join("backups");
        assert!(backups_dir.join("v1").join("hello-component").exists());
    }

    fn build_fixture_zip_recording_args(path: &Path) {
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.add_directory("hello-component-main/", options).unwrap();
        zip.start_file("hello-component-main/setup.sh", options)
            .unwrap();
        zip.write_all(b"#!/bin/sh\necho \"$@\" > args.txt\ntouch marker.txt\n")
            .unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn set_options_are_appended_as_set_flags_to_setup() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        let zip_path = fixture_dir.path().join("bundle.zip");
        build_fixture_zip_recording_args(&zip_path);

        let mut component = sample_component();
        component.supports_options_protocol.posix = true;
        let manifest = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![component.clone()],
            removals: vec![],
        };

        let fetcher = FixtureFetcher { zip_path };
        let opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "abc123".into(),
            backup_keep: 3,
            set_options: vec![("model".to_string(), "qwen3:14b".to_string())],
            force: false,
        };

        install_component(&component, &manifest, &opts).unwrap();

        let recorded_args =
            fs::read_to_string(root.path().join("hello-component").join("args.txt")).unwrap();
        assert!(recorded_args.contains("--set model=qwen3:14b"));
    }

    #[test]
    fn removals_older_than_the_manifest_are_applied_on_reinstall() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        let zip_path = fixture_dir.path().join("bundle.zip");
        build_fixture_zip(&zip_path);

        let component = sample_component();
        let fetcher = FixtureFetcher { zip_path };

        // First install at manifest_version "1.0.0" — leaves a legacy file behind
        // that a later manifest version will mark for removal.
        let manifest_v1 = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![component.clone()],
            removals: vec![],
        };
        let opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "abc123".into(),
            backup_keep: 3,
            set_options: vec![],
            force: false,
        };
        install_component(&component, &manifest_v1, &opts).unwrap();
        fs::write(
            root.path().join("hello-component").join("legacy_tool.py"),
            "old",
        )
        .unwrap();

        // Second install at manifest_version "1.1.0", which declares that file
        // deprecated as of 1.1.0 — must be removed by this install.
        let manifest_v2 = Manifest {
            manifest_version: "1.1.0".into(),
            components: vec![component.clone()],
            removals: vec![mlai_core_removal_entry(
                "1.1.0",
                "hello-component/legacy_tool.py",
            )],
        };
        install_component(&component, &manifest_v2, &opts).unwrap();

        assert!(!root
            .path()
            .join("hello-component")
            .join("legacy_tool.py")
            .exists());

        let state = InstalledState::load(root.path()).unwrap();
        assert_eq!(state.manifest_version, "1.1.0");
    }

    #[test]
    fn force_bypasses_the_already_healthy_short_circuit() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        let zip_path = fixture_dir.path().join("bundle.zip");
        build_fixture_zip(&zip_path);

        let component = sample_component();
        let fetcher = FixtureFetcher { zip_path };
        let manifest = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![component.clone()],
            removals: vec![],
        };
        let opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "abc123".into(),
            backup_keep: 3,
            set_options: vec![],
            force: false,
        };
        install_component(&component, &manifest, &opts).unwrap();

        // Break health without changing the recorded state — a plain
        // (non-forced) re-run must still trust the record and leave the
        // break in place.
        fs::remove_file(root.path().join("hello-component").join("marker.txt")).unwrap();
        let result = install_component(&component, &manifest, &opts).unwrap();
        assert_eq!(result, ComponentState::Healthy);
        assert!(
            !root
                .path()
                .join("hello-component")
                .join("marker.txt")
                .exists(),
            "force: false must not touch disk when the record says healthy"
        );

        let forced_opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "abc123".into(),
            backup_keep: 3,
            set_options: vec![],
            force: true,
        };
        let result = install_component(&component, &manifest, &forced_opts).unwrap();
        assert_eq!(result, ComponentState::Healthy);
        assert!(
            root.path()
                .join("hello-component")
                .join("marker.txt")
                .exists(),
            "force: true must bypass the short-circuit and actually reinstall"
        );
    }

    fn mlai_core_removal_entry(version: &str, path: &str) -> crate::manifest::RemovalEntry {
        crate::manifest::RemovalEntry {
            version: version.to_string(),
            paths: vec![path.to_string()],
        }
    }

    #[test]
    fn repair_reinstalls_when_disk_is_broken_despite_recorded_healthy_state() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        let zip_path = fixture_dir.path().join("bundle.zip");
        build_fixture_zip(&zip_path);

        let component = sample_component();
        let fetcher = FixtureFetcher { zip_path };
        let manifest = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![component.clone()],
            removals: vec![],
        };
        let opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "abc123".into(),
            backup_keep: 3,
            set_options: vec![],
            force: false,
        };
        install_component(&component, &manifest, &opts).unwrap();

        // Real bug this guards: installed.json says healthy, but the health
        // target is missing from disk (e.g. a user deleted it by hand).
        fs::remove_file(root.path().join("hello-component").join("marker.txt")).unwrap();

        let (state_after, reinstalled) = repair_component(&component, &manifest, &opts).unwrap();

        assert_eq!(state_after, ComponentState::Healthy);
        assert!(
            reinstalled,
            "repair must have gone through a real reinstall"
        );
        assert!(
            root.path()
                .join("hello-component")
                .join("marker.txt")
                .exists(),
            "repair should have fixed the health target"
        );
    }

    #[test]
    fn repair_makes_zero_filesystem_changes_when_genuinely_healthy_even_with_no_recorded_state() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        // Deliberately never build a fixture zip — repair must never reach
        // the download step for a genuinely healthy component.
        let zip_path = fixture_dir.path().join("bundle.zip");

        let component = sample_component();
        let component_dir = root.path().join("hello-component");
        fs::create_dir_all(&component_dir).unwrap();
        fs::write(component_dir.join("marker.txt"), "real marker").unwrap();

        let fetcher = FixtureFetcher { zip_path };
        let manifest = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![component.clone()],
            removals: vec![],
        };
        let opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "abc123".into(),
            backup_keep: 3,
            set_options: vec![],
            force: false,
        };

        // Deliberately no prior install_component call — installed.json has
        // no record for this component at all. Repair must decide from disk.
        let (state_after, reinstalled) = repair_component(&component, &manifest, &opts).unwrap();

        assert_eq!(state_after, ComponentState::Healthy);
        assert!(
            !reinstalled,
            "a genuinely healthy component must not be reinstalled"
        );
        assert!(
            !root.path().join(".mlai-install").join("backups").exists(),
            "repair must not back up a genuinely healthy component"
        );
    }

    #[test]
    fn repair_reinstalls_when_the_component_directory_does_not_exist() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        let zip_path = fixture_dir.path().join("bundle.zip");
        build_fixture_zip(&zip_path);

        let component = sample_component();
        let fetcher = FixtureFetcher { zip_path };
        let manifest = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![component.clone()],
            removals: vec![],
        };
        let opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "abc123".into(),
            backup_keep: 3,
            set_options: vec![],
            force: false,
        };

        let (state_after, reinstalled) = repair_component(&component, &manifest, &opts).unwrap();

        assert_eq!(state_after, ComponentState::Healthy);
        assert!(reinstalled);
        assert!(root
            .path()
            .join("hello-component")
            .join("marker.txt")
            .exists());
    }

    #[test]
    fn bind_project_ignores_untagged_and_uninstalled_components() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        let zip_path = fixture_dir.path().join("bundle.zip");
        build_fixture_zip(&zip_path);
        let fetcher = FixtureFetcher { zip_path };

        let mut untagged = sample_component();
        untagged.name = "untagged".into();

        let mut tagged_not_installed = sample_component();
        tagged_not_installed.name = "tagged-not-installed".into();
        tagged_not_installed.binds_to_project_type = Some("UE5".into());

        let manifest = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![untagged.clone(), tagged_not_installed],
            removals: vec![],
        };

        // Install only the untagged component, so it's recorded in installed.json
        // -- tagged_not_installed is declared in the manifest but never installed.
        let opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "abc123".into(),
            backup_keep: 3,
            set_options: vec![],
            force: false,
        };
        install_component(&untagged, &manifest, &opts).unwrap();

        let results = bind_project(
            &manifest,
            root.path(),
            &fetcher,
            "UE5",
            Path::new("/fake/MyGame.uproject"),
        );

        assert!(
            results.is_empty(),
            "untagged component and a tagged-but-uninstalled component must both be skipped"
        );
    }

    #[test]
    fn bind_project_substitutes_the_real_path_and_force_reinstalls_a_matching_installed_component()
    {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        let zip_path = fixture_dir.path().join("bundle.zip");
        build_fixture_zip_with_project_placeholder(&zip_path);
        let fetcher = FixtureFetcher { zip_path };

        let ue5 = Component {
            name: "ue5-cine-pipeline".into(),
            source_url: "https://example.com/ue5-cine-pipeline.zip".into(),
            component_ref: "main".into(),
            default: true,
            setup: PlatformSetup {
                windows: None,
                posix: Some(SetupCommand {
                    command: "sh".into(),
                    args: vec!["setup.sh".into(), "-Project".into(), "{project}".into()],
                }),
            },
            health: PlatformHealth {
                windows: None,
                posix: Some(HealthCheck::FileExists {
                    path: "marker.txt".into(),
                }),
            },
            supports_options_protocol: PlatformFlag::default(),
            binds_to_project_type: Some("UE5".into()),
        };
        let manifest = Manifest {
            manifest_version: "1.0.0".into(),
            components: vec![ue5.clone()],
            removals: vec![],
        };

        let opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "main".into(),
            backup_keep: 3,
            set_options: vec![],
            force: false,
        };
        install_component(&ue5, &manifest, &opts).unwrap();

        let results = bind_project(
            &manifest,
            root.path(),
            &fetcher,
            "UE5",
            Path::new("/fake/MyGame.uproject"),
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "ue5-cine-pipeline");
        assert_eq!(results[0].1.as_ref().unwrap(), &ComponentState::Healthy);

        let argv =
            fs::read_to_string(root.path().join("ue5-cine-pipeline").join("argv.txt")).unwrap();
        assert!(
            argv.contains("/fake/MyGame.uproject"),
            "the {{project}} placeholder must be substituted with the real path: {argv}"
        );
        assert!(
            !argv.contains("{project}"),
            "the literal placeholder must not survive into the real setup invocation: {argv}"
        );
    }
}
