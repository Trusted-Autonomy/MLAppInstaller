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
}

pub fn install_component(
    component: &Component,
    manifest: &crate::manifest::Manifest,
    opts: &PipelineOptions,
) -> Result<ComponentState, PipelineError> {
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

    let existing_version = state
        .components
        .get(&component.name)
        .map(|r| r.version.clone());
    if let Some(existing) = state.components.get(&component.name) {
        if existing.version == opts.version && existing.state == ComponentState::Healthy {
            return Ok(ComponentState::Healthy);
        }
    }

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
    record_state(&mut state, opts, component, ComponentState::Downloaded)?;

    let component_dir = unpack_zip(&zip_path, &opts.install_root, &component.name)?;
    record_state(&mut state, opts, component, ComponentState::Unpacked)?;

    if let Some(setup) = component.setup_for_current_os() {
        run_setup(&component_dir, setup, &opts.set_options)?;
    }
    record_state(&mut state, opts, component, ComponentState::SetupRun)?;

    let final_state = match check_health(&component_dir, component.health_for_current_os()) {
        HealthStatus::Healthy => ComponentState::Healthy,
        HealthStatus::NeedsAttention(_) => ComponentState::NeedsAttention,
    };
    record_state(&mut state, opts, component, final_state)?;

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

#[cfg(test)]
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
        };
        install_component(&component, &manifest, &opts_v1).unwrap();

        let opts_v2 = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "v2".into(),
            backup_keep: 3,
            set_options: vec![],
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

    fn mlai_core_removal_entry(version: &str, path: &str) -> crate::manifest::RemovalEntry {
        crate::manifest::RemovalEntry {
            version: version.to_string(),
            paths: vec![path.to_string()],
        }
    }
}
