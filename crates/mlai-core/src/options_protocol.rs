use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crate::manifest::SetupCommand;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct OptionsDescriptor {
    pub schema_version: u32,
    pub options: Vec<OptionSpec>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OptionSpec {
    Choice {
        key: String,
        label: String,
        choices: Vec<ChoiceValue>,
        default: Option<String>,
    },
    Bool {
        key: String,
        label: String,
        default: bool,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ChoiceValue {
    pub value: String,
    pub label: String,
    #[serde(default)]
    pub recommended: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum OptionsError {
    #[error("failed to launch '{command} --describe-options': {source}")]
    Launch {
        command: String,
        #[source]
        source: std::io::Error,
    },
    #[error("'{command} --describe-options' timed out after {timeout_secs}s")]
    Timeout { command: String, timeout_secs: u64 },
    #[error("'{command} --describe-options' exited with status {status}")]
    NonZeroExit { command: String, status: i32 },
    #[error("'{command} --describe-options' produced unparseable JSON: {reason}")]
    UnparseableJson { command: String, reason: String },
}

/// Probes a component's setup command for the backend-options protocol.
///
/// Per the protocol's own safety rationale, a
/// caller MUST NOT call this unless the component's manifest entry
/// explicitly declares `supports_options_protocol = true` — an unpatched
/// setup script could silently run its real, side-effecting setup if
/// handed an unrecognized flag instead of erroring.
///
/// Known limitation: on timeout, the spawned child process is not killed —
/// this thread simply stops waiting for it. Acceptable for a probe that's
/// documented to print one line of JSON and exit; a hung/misbehaving
/// script leaks a background wait, not a resource leak in this process.
pub fn describe_options(
    setup: &SetupCommand,
    component_dir: &Path,
    timeout: Duration,
) -> Result<OptionsDescriptor, OptionsError> {
    let mut cmd = Command::new(&setup.command);
    cmd.args(&setup.args)
        .arg("--describe-options")
        .current_dir(component_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let child = cmd.spawn().map_err(|source| OptionsError::Launch {
        command: setup.command.clone(),
        source,
    })?;

    let (tx, rx) = mpsc::channel();
    let command_name = setup.command.clone();
    std::thread::spawn(move || {
        let result = child.wait_with_output();
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => {
            if !output.status.success() {
                return Err(OptionsError::NonZeroExit {
                    command: command_name,
                    status: output.status.code().unwrap_or(-1),
                });
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_descriptor(&command_name, &stdout)
        }
        Ok(Err(source)) => Err(OptionsError::Launch {
            command: command_name,
            source,
        }),
        Err(_timeout) => Err(OptionsError::Timeout {
            command: command_name,
            timeout_secs: timeout.as_secs(),
        }),
    }
}

fn parse_descriptor(command: &str, output: &str) -> Result<OptionsDescriptor, OptionsError> {
    let value: serde_json::Value =
        serde_json::from_str(output.trim()).map_err(|e| OptionsError::UnparseableJson {
            command: command.to_string(),
            reason: e.to_string(),
        })?;
    let schema_version = value
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| OptionsError::UnparseableJson {
            command: command.to_string(),
            reason: "missing or non-numeric schema_version".to_string(),
        })? as u32;
    let options = value
        .get("options")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| serde_json::from_value::<OptionSpec>(item.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    Ok(OptionsDescriptor {
        schema_version,
        options,
    })
}

// This module's tests spawn `sh` against `#!/bin/sh` fixture scripts, so
// they only make sense on unix — a faithful Windows equivalent would need
// its own cmd/PowerShell fixtures, which is real work deferred until a
// component actually needs Windows setup-command testing. Windows CI still
// verifies this crate compiles and its non-shell tests (manifest, state,
// backup, health, fetch, removals, versioning) run for real.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::manifest::SetupCommand;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    fn write_fixture_script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("setup.sh");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    fn parses_a_valid_descriptor() {
        let dir = tempdir().unwrap();
        write_fixture_script(
            dir.path(),
            r#"echo '{"schema_version":1,"options":[{"key":"model","label":"Local model","type":"choice","choices":[{"value":"a","label":"A","recommended":true}],"default":"a"},{"key":"cloud_only","label":"Cloud only","type":"bool","default":false}]}'"#,
        );
        let setup = SetupCommand {
            command: "sh".into(),
            args: vec!["setup.sh".into()],
        };

        let descriptor = describe_options(&setup, dir.path(), Duration::from_secs(5)).unwrap();

        assert_eq!(descriptor.schema_version, 1);
        assert_eq!(descriptor.options.len(), 2);
    }

    #[test]
    fn unknown_option_type_is_skipped_not_errored() {
        let dir = tempdir().unwrap();
        write_fixture_script(
            dir.path(),
            r#"echo '{"schema_version":1,"options":[{"key":"x","label":"X","type":"slider","min":0,"max":10},{"key":"cloud_only","label":"Cloud only","type":"bool","default":false}]}'"#,
        );
        let setup = SetupCommand {
            command: "sh".into(),
            args: vec!["setup.sh".into()],
        };

        let descriptor = describe_options(&setup, dir.path(), Duration::from_secs(5)).unwrap();

        assert_eq!(descriptor.options.len(), 1);
    }

    #[test]
    fn non_zero_exit_produces_actionable_error() {
        let dir = tempdir().unwrap();
        write_fixture_script(dir.path(), "exit 1");
        let setup = SetupCommand {
            command: "sh".into(),
            args: vec!["setup.sh".into()],
        };

        let err = describe_options(&setup, dir.path(), Duration::from_secs(5)).unwrap_err();
        assert!(matches!(err, OptionsError::NonZeroExit { status: 1, .. }));
    }

    #[test]
    fn unparseable_output_produces_actionable_error() {
        let dir = tempdir().unwrap();
        write_fixture_script(dir.path(), "echo 'not json'");
        let setup = SetupCommand {
            command: "sh".into(),
            args: vec!["setup.sh".into()],
        };

        let err = describe_options(&setup, dir.path(), Duration::from_secs(5)).unwrap_err();
        assert!(matches!(err, OptionsError::UnparseableJson { .. }));
    }

    #[test]
    fn slow_script_times_out() {
        let dir = tempdir().unwrap();
        write_fixture_script(dir.path(), "sleep 2");
        let setup = SetupCommand {
            command: "sh".into(),
            args: vec!["setup.sh".into()],
        };

        let err = describe_options(&setup, dir.path(), Duration::from_millis(200)).unwrap_err();
        assert!(matches!(err, OptionsError::Timeout { .. }));
    }
}
