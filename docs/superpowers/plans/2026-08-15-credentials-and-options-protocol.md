# Credentials Vault + Backend Options Protocol Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add secure credential storage (`mlai-credentials`, generalized from TA's `ta-credentials`) and the backend-options protocol (`mlai-core::options_protocol`, generalized from cinepipe-installer's Setup Options Protocol) so a component can declare local-vs-hosted choices, and `mlai install --set key=value` / `mlai credential set` can act on them.

**Architecture:** A new `mlai-credentials` crate (age encryption, OS-keychain-first custody, chmod-0600 fallback — ported from `ta-credentials/src/encryption.rs`, generalized so the keyring namespace is a caller parameter) backing a flat encrypted key-value `Vault`. A new `mlai-core::options_protocol` module probes a component's setup command for `--describe-options` (cinepipe's exact protocol shape) with a timeout and forward-compatible unknown-type handling. The existing pipeline (`crates/mlai-core/src/pipeline.rs`) gains a `set_options` field threaded through to `--set key=value` args on the setup invocation. `mlai-cli` gains `install --set key=value` (gated on the manifest declaring `supports_options_protocol = true`) and a new `credential set <key>` subcommand.

**Tech Stack:** Adds `age = "0.12"` and `keyring = "4"` (exact versions TA already uses in production) to a new `mlai-credentials` crate. No new dependencies in `mlai-core` or `mlai-cli` beyond what Plan A already added.

## Global Constraints

- Secrets are never written to disk in plaintext — always through `mlai-credentials`'s age-encrypted vault (`docs/CONSTITUTION.md` §2.1).
- When OS keychain custody is unavailable and the vault falls back to a chmod-0600 file, that must be surfaced to the user, not silently logged (`docs/CONSTITUTION.md` §2.2) — this plan uses `eprintln!` for that disclosure; a full `tracing` setup is out of scope.
- A component that doesn't implement the options protocol behaves exactly as it does today — probing or `--set` are only ever attempted when the manifest explicitly declares `supports_options_protocol = true` (`docs/superpowers/specs/2026-08-14-foundation-design.md`, "Local vs. cloud backend selection").
- `mlai-core`'s options-protocol flag names are verbatim cinepipe-compatible: `--describe-options`, `--set key=value` (spec's "Additional decisions").
- Unknown `OptionSpec` `type` values are skipped, not treated as an error (forward-compatible, per `docs/SETUP-OPTIONS-PROTOCOL.md`'s own protocol spec).
- Before every commit: `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` all pass (`docs/CONSTITUTION.md` §5).
- All filesystem-touching tests use `tempfile::tempdir()`. All tests that force key custody use `use_keychain: false` to force the deterministic file-fallback path — OS keychain interaction is not reliably testable in CI (matches `ta-credentials`'s own test suite, which never exercises the real keychain either).

## Out of scope for this plan

- Interactive/hidden secret entry (`mlai credential set` reads plaintext from stdin with terminal echo — a real "hidden input" UX needs a password-input crate, deferred).
- `repair`/`uninstall`/`update` CLI commands and versioned `removals` — Plan C.
- `mlai-cloud` (config generation + provider adapters) — Plan D.
- Automatically prompting for a hosted API key during `mlai install` when a component's descriptor requests one — this plan wires the mechanisms (vault + protocol + `--set`) but the interactive "ask, then store, then pass" orchestration is a follow-up once this foundation exists.

---

### Task 1: `mlai-credentials` scaffold + encryption (identity custody, encrypt/decrypt)

**Files:**
- Modify: `Cargo.toml` (add workspace member)
- Create: `crates/mlai-credentials/Cargo.toml`
- Create: `crates/mlai-credentials/src/lib.rs`
- Create: `crates/mlai-credentials/src/error.rs`
- Create: `crates/mlai-credentials/src/encryption.rs`

**Interfaces:**
- Produces: `mlai_credentials::error::VaultError` (`Io`, `Serialization`, `KeyUnreadable { path, reason }`, `DecryptionFailed { path, reason }`, `EncryptionFailed(String)`). `mlai_credentials::encryption::{KeyCustody, FALLBACK_KEY_FILENAME, load_or_create_identity, encrypt, decrypt}`. `KeyCustody` variants: `Keychain`, `FallbackFile(PathBuf)`. `load_or_create_identity(vault_dir: &Path, keyring_service: &str, keyring_user: &str, use_keychain: bool) -> Result<(age::x25519::Identity, KeyCustody), VaultError>`. `encrypt(identity: &Identity, plaintext: &[u8]) -> Result<Vec<u8>, VaultError>`. `decrypt(identity: &Identity, vault_path: &Path, ciphertext: &[u8]) -> Result<Vec<u8>, VaultError>`.

- [x] **Step 1: Add the workspace member and create the crate skeleton**

Modify `Cargo.toml` (repo root) — change:
```toml
members = ["crates/mlai-core", "crates/mlai-cli"]
```
to:
```toml
members = ["crates/mlai-core", "crates/mlai-cli", "crates/mlai-credentials"]
```

`crates/mlai-credentials/Cargo.toml`:
```toml
[package]
name = "mlai-credentials"
version.workspace = true
edition.workspace = true

[dependencies]
age = "0.12"
keyring = "4"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"

[dev-dependencies]
tempfile = "3"
```

`crates/mlai-credentials/src/lib.rs`:
```rust
pub mod error;
```

Verify the crate compiles standalone:
```bash
cd crates/mlai-credentials && cargo build
```
Expected: succeeds (empty `error` module referenced next step, `encryption` not yet declared).

- [x] **Step 2: Write `error.rs` (no test needed — pure type definitions consumed by the next step's tests)**

`crates/mlai-credentials/src/error.rs`:
```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error(
        "vault encryption key at {path} is missing or corrupt: {reason}. \
         Delete the file to generate a fresh key (existing encrypted credentials \
         will become unreadable and must be re-added), or restore the original \
         key file from backup."
    )]
    KeyUnreadable {
        path: std::path::PathBuf,
        reason: String,
    },

    #[error(
        "failed to decrypt vault at {path}: {reason}. This usually means the \
         encryption key was lost, rotated, or replaced with a mismatched one — \
         existing credentials cannot be recovered without the original key."
    )]
    DecryptionFailed {
        path: std::path::PathBuf,
        reason: String,
    },

    #[error("failed to encrypt vault: {0}")]
    EncryptionFailed(String),
}
```

Add to `crates/mlai-credentials/src/lib.rs` (already present from Step 1 — no change needed here; `error` is already declared).

- [x] **Step 3: Write the failing test**

`crates/mlai-credentials/src/encryption.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn fallback_custody_generates_and_persists_identity() {
        let dir = tempdir().unwrap();
        let (identity1, custody1) =
            load_or_create_identity(dir.path(), "mlai-test-service", "test-user", false).unwrap();
        assert!(matches!(custody1, KeyCustody::FallbackFile(_)));

        let (identity2, _custody2) =
            load_or_create_identity(dir.path(), "mlai-test-service", "test-user", false).unwrap();

        assert_eq!(
            identity1.to_public().to_string(),
            identity2.to_public().to_string()
        );
    }

    #[test]
    #[cfg(unix)]
    fn fallback_key_file_is_chmod_0600() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        load_or_create_identity(dir.path(), "mlai-test-service", "test-user", false).unwrap();

        let key_path = dir.path().join(FALLBACK_KEY_FILENAME);
        let mode = fs::metadata(&key_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn corrupt_fallback_key_produces_actionable_error() {
        use std::fs;

        let dir = tempdir().unwrap();
        let key_path = dir.path().join(FALLBACK_KEY_FILENAME);
        fs::write(&key_path, "not-a-valid-age-identity").unwrap();

        let err = load_or_create_identity(dir.path(), "mlai-test-service", "test-user", false)
            .unwrap_err();
        assert!(matches!(err, VaultError::KeyUnreadable { .. }));
    }

    #[test]
    fn encrypt_decrypt_round_trips() {
        let identity = age::x25519::Identity::generate();
        let plaintext = b"{\"api_key\":\"sk-test\"}";

        let ciphertext = encrypt(&identity, plaintext).unwrap();
        assert_ne!(ciphertext, plaintext);

        let decrypted = decrypt(&identity, Path::new("/tmp/vault.age"), &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypt_with_wrong_identity_produces_actionable_error() {
        let identity = age::x25519::Identity::generate();
        let other = age::x25519::Identity::generate();
        let ciphertext = encrypt(&identity, b"secret-data").unwrap();

        let err = decrypt(&other, Path::new("/tmp/vault.age"), &ciphertext).unwrap_err();
        assert!(matches!(err, VaultError::DecryptionFailed { .. }));
    }
}
```

- [x] **Step 4: Run test to verify it fails**

Run: `cd crates/mlai-credentials && cargo test`
Expected: FAIL to compile — `load_or_create_identity`, `KeyCustody`, `FALLBACK_KEY_FILENAME`, `encrypt`, `decrypt` are not defined.

- [x] **Step 5: Write the implementation**

Prepend this to the top of `crates/mlai-credentials/src/encryption.rs`, above the `#[cfg(test)]` module. This is adapted directly from TA's proven `ta-credentials/src/encryption.rs`, generalized so the keyring service/user are caller parameters instead of hardcoded to `trusted-autonomy-vault`:
```rust
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use age::secrecy::ExposeSecret;
use age::x25519::Identity;

use crate::error::VaultError;

/// Filename of the fallback key file, stored next to the vault file itself.
pub const FALLBACK_KEY_FILENAME: &str = "credentials.key";

/// Where the vault's age identity is actually stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyCustody {
    /// Stored in the OS-native keychain/credential manager.
    Keychain,
    /// Stored in a chmod-0600 file — used when no keychain backend is
    /// reachable, or when the caller opts out of keychain use (tests,
    /// headless servers).
    FallbackFile(PathBuf),
}

/// Load the vault's age identity, generating and persisting one on first use.
///
/// When `use_keychain` is true, tries the OS keychain first (namespaced by
/// `keyring_service`/`keyring_user`); on any failure falls back to a
/// chmod-0600 file at `<vault_dir>/credentials.key`, printing a warning to
/// stderr so the weaker guarantee is never silent (docs/CONSTITUTION.md
/// §2.2). When `use_keychain` is false, the keychain is never touched.
pub fn load_or_create_identity(
    vault_dir: &Path,
    keyring_service: &str,
    keyring_user: &str,
    use_keychain: bool,
) -> Result<(Identity, KeyCustody), VaultError> {
    if use_keychain {
        match keyring_load_or_create(keyring_service, keyring_user) {
            Ok(identity) => return Ok((identity, KeyCustody::Keychain)),
            Err(reason) => {
                eprintln!(
                    "warning: OS keychain unavailable for vault encryption key ({reason}); \
                     falling back to a chmod-0600 key file."
                );
            }
        }
    }
    let key_path = vault_dir.join(FALLBACK_KEY_FILENAME);
    let identity = file_load_or_create(&key_path)?;
    Ok((identity, KeyCustody::FallbackFile(key_path)))
}

fn keyring_load_or_create(service: &str, user: &str) -> Result<Identity, String> {
    let entry =
        keyring::Entry::new(service, user).map_err(|e| format!("keyring entry error: {e}"))?;
    match entry.get_password() {
        Ok(secret) => Identity::from_str(&secret)
            .map_err(|e| format!("stored age identity is corrupt: {e}")),
        Err(keyring::Error::NoEntry) => {
            let identity = Identity::generate();
            entry
                .set_password(identity.to_string().expose_secret())
                .map_err(|e| format!("keyring write error: {e}"))?;
            Ok(identity)
        }
        Err(e) => Err(format!("keyring read error: {e}")),
    }
}

fn file_load_or_create(key_path: &Path) -> Result<Identity, VaultError> {
    if key_path.exists() {
        let content = fs::read_to_string(key_path)?;
        Identity::from_str(content.trim()).map_err(|e| VaultError::KeyUnreadable {
            path: key_path.to_path_buf(),
            reason: e.to_string(),
        })
    } else {
        if let Some(parent) = key_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let identity = Identity::generate();
        fs::write(key_path, identity.to_string().expose_secret())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(key_path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(identity)
    }
}

/// Encrypt `plaintext` to the given identity's recipient.
pub fn encrypt(identity: &Identity, plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
    let recipient = identity.to_public();
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
            .map_err(|e| VaultError::EncryptionFailed(e.to_string()))?;

    let mut encrypted = Vec::new();
    let mut writer = encryptor
        .wrap_output(&mut encrypted)
        .map_err(|e| VaultError::EncryptionFailed(e.to_string()))?;
    writer
        .write_all(plaintext)
        .map_err(|e| VaultError::EncryptionFailed(e.to_string()))?;
    writer
        .finish()
        .map_err(|e| VaultError::EncryptionFailed(e.to_string()))?;
    Ok(encrypted)
}

/// Decrypt `ciphertext` (produced by [`encrypt`]) with the given identity.
pub fn decrypt(
    identity: &Identity,
    vault_path: &Path,
    ciphertext: &[u8],
) -> Result<Vec<u8>, VaultError> {
    let fail = |reason: String| VaultError::DecryptionFailed {
        path: vault_path.to_path_buf(),
        reason,
    };

    let decryptor = age::Decryptor::new(ciphertext).map_err(|e| fail(e.to_string()))?;
    let mut reader = decryptor
        .decrypt(std::iter::once(identity as &dyn age::Identity))
        .map_err(|e| fail(e.to_string()))?;
    let mut decrypted = Vec::new();
    reader
        .read_to_end(&mut decrypted)
        .map_err(|e| fail(e.to_string()))?;
    Ok(decrypted)
}
```

Add to `crates/mlai-credentials/src/lib.rs`:
```rust
pub mod encryption;
```

- [x] **Step 6: Run test to verify it passes**

Run: `cd crates/mlai-credentials && cargo test`
Expected: PASS — 5 tests.

- [x] **Step 7: Commit**

```bash
git add Cargo.toml crates/mlai-credentials
git commit -m "feat(mlai-credentials): add age-encrypted identity custody (keychain + fallback)"
```

---

### Task 2: Credential vault (flat encrypted key-value store)

**Files:**
- Create: `crates/mlai-credentials/src/vault.rs`
- Modify: `crates/mlai-credentials/src/lib.rs`

**Interfaces:**
- Consumes: `mlai_credentials::encryption::{load_or_create_identity, encrypt, decrypt, KeyCustody}` (Task 1), `mlai_credentials::error::VaultError` (Task 1).
- Produces: `mlai_credentials::vault::{VaultConfig, Vault}`. `VaultConfig { vault_dir: PathBuf, keyring_service: String, keyring_user: String }`. `Vault::open(config: VaultConfig, use_keychain: bool) -> Result<Vault, VaultError>`. `Vault::get(&self, key: &str) -> Option<&str>`. `Vault::set(&mut self, key: &str, value: &str) -> Result<(), VaultError>`. `Vault::custody(&self) -> &KeyCustody`.

- [x] **Step 1: Write the failing test**

`crates/mlai-credentials/src/vault.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::tempdir;

    fn test_config(dir: &Path) -> VaultConfig {
        VaultConfig {
            vault_dir: dir.to_path_buf(),
            keyring_service: "mlai-test-service".to_string(),
            keyring_user: "test-user".to_string(),
        }
    }

    #[test]
    fn set_then_get_round_trips() {
        let dir = tempdir().unwrap();
        let mut vault = Vault::open(test_config(dir.path()), false).unwrap();
        vault.set("api_key", "sk-test-123").unwrap();
        assert_eq!(vault.get("api_key"), Some("sk-test-123"));
    }

    #[test]
    fn get_returns_none_for_unknown_key() {
        let dir = tempdir().unwrap();
        let vault = Vault::open(test_config(dir.path()), false).unwrap();
        assert_eq!(vault.get("nope"), None);
    }

    #[test]
    fn reopening_the_vault_persists_entries() {
        let dir = tempdir().unwrap();
        {
            let mut vault = Vault::open(test_config(dir.path()), false).unwrap();
            vault.set("api_key", "sk-persisted").unwrap();
        }
        let reopened = Vault::open(test_config(dir.path()), false).unwrap();
        assert_eq!(reopened.get("api_key"), Some("sk-persisted"));
    }

    #[test]
    fn vault_file_on_disk_is_not_plaintext() {
        let dir = tempdir().unwrap();
        let mut vault = Vault::open(test_config(dir.path()), false).unwrap();
        vault
            .set("api_key", "sk-should-not-appear-in-plaintext")
            .unwrap();

        let raw = std::fs::read(dir.path().join("credentials.age")).unwrap();
        let raw_str = String::from_utf8_lossy(&raw);
        assert!(!raw_str.contains("sk-should-not-appear-in-plaintext"));
    }
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-credentials && cargo test vault::`
Expected: FAIL to compile — module `vault` doesn't exist yet.

- [x] **Step 3: Write the implementation**

Prepend to the top of `crates/mlai-credentials/src/vault.rs`:
```rust
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use age::x25519::Identity;

use crate::encryption::{decrypt, encrypt, load_or_create_identity, KeyCustody};
use crate::error::VaultError;

pub struct VaultConfig {
    pub vault_dir: PathBuf,
    pub keyring_service: String,
    pub keyring_user: String,
}

pub struct Vault {
    identity: Identity,
    custody: KeyCustody,
    vault_path: PathBuf,
    entries: BTreeMap<String, String>,
}

impl Vault {
    pub fn open(config: VaultConfig, use_keychain: bool) -> Result<Vault, VaultError> {
        fs::create_dir_all(&config.vault_dir)?;
        let (identity, custody) = load_or_create_identity(
            &config.vault_dir,
            &config.keyring_service,
            &config.keyring_user,
            use_keychain,
        )?;
        let vault_path = config.vault_dir.join("credentials.age");
        let entries = if vault_path.exists() {
            let ciphertext = fs::read(&vault_path)?;
            let plaintext = decrypt(&identity, &vault_path, &ciphertext)?;
            serde_json::from_slice(&plaintext)?
        } else {
            BTreeMap::new()
        };
        Ok(Vault {
            identity,
            custody,
            vault_path,
            entries,
        })
    }

    pub fn custody(&self) -> &KeyCustody {
        &self.custody
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<(), VaultError> {
        self.entries.insert(key.to_string(), value.to_string());
        self.persist()
    }

    fn persist(&self) -> Result<(), VaultError> {
        let plaintext = serde_json::to_vec(&self.entries)?;
        let ciphertext = encrypt(&self.identity, &plaintext)?;
        fs::write(&self.vault_path, ciphertext)?;
        Ok(())
    }
}
```

Add to `crates/mlai-credentials/src/lib.rs`:
```rust
pub mod vault;
```

- [x] **Step 4: Run test to verify it passes**

Run: `cd crates/mlai-credentials && cargo test`
Expected: PASS — 9 tests total (5 from Task 1 + 4 new).

- [x] **Step 5: Commit**

```bash
git add crates/mlai-credentials/src/vault.rs crates/mlai-credentials/src/lib.rs
git commit -m "feat(mlai-credentials): add flat encrypted key-value vault"
```

---

### Task 3: Manifest flag + options-protocol probe

**Files:**
- Modify: `crates/mlai-core/src/manifest.rs`
- Create: `crates/mlai-core/src/options_protocol.rs`
- Modify: `crates/mlai-core/src/lib.rs`

**Interfaces:**
- Consumes: `mlai_core::manifest::SetupCommand` (Plan A, Task 1).
- Produces: `Component.supports_options_protocol: bool` (new field, `#[serde(default)]`, so existing manifests without it still parse). `mlai_core::options_protocol::{OptionsDescriptor, OptionSpec, ChoiceValue, OptionsError, describe_options}`. `describe_options(setup: &SetupCommand, component_dir: &Path, timeout: Duration) -> Result<OptionsDescriptor, OptionsError>`.

- [x] **Step 1: Write the failing test for the manifest field**

In `crates/mlai-core/src/manifest.rs`, add this test to the existing `#[cfg(test)] mod tests` block (append after `rejects_invalid_toml`):
```rust
    #[test]
    fn supports_options_protocol_defaults_to_false_when_absent() {
        let manifest = Manifest::parse(SAMPLE).unwrap();
        assert!(!manifest.components[0].supports_options_protocol);
    }

    #[test]
    fn supports_options_protocol_parses_when_present() {
        let toml = r#"
manifest_version = "1.0.0"

[[components]]
name = "hello-component"
source_url = "https://example.com/hello-component.zip"
ref = "main"
default = true
supports_options_protocol = true
"#;
        let manifest = Manifest::parse(toml).unwrap();
        assert!(manifest.components[0].supports_options_protocol);
    }
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test manifest::`
Expected: FAIL to compile — `Component` has no field `supports_options_protocol`.

- [x] **Step 3: Add the field**

In `crates/mlai-core/src/manifest.rs`, modify the `Component` struct (add the new field at the end):
```rust
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Component {
    pub name: String,
    pub source_url: String,
    #[serde(rename = "ref")]
    pub component_ref: String,
    #[serde(default)]
    pub default: bool,
    pub setup: Option<SetupCommand>,
    pub health: Option<HealthCheck>,
    #[serde(default)]
    pub supports_options_protocol: bool,
}
```

- [x] **Step 4: Run test to verify it passes**

Run: `cd crates/mlai-core && cargo test manifest::`
Expected: PASS — 6 tests (4 from Plan A + 2 new). Note: this will also break `crates/mlai-core/src/pipeline.rs`'s tests (they construct `Component` literals without the new field) and `crates/mlai-cli`'s build — fixed in the next two steps before committing, so the workspace is never left in a broken state between commits.

- [x] **Step 5: Fix the now-broken `Component` literal in pipeline.rs**

In `crates/mlai-core/src/pipeline.rs`, modify `sample_component()` (used by all pipeline tests) — add the new field:
```rust
    fn sample_component() -> Component {
        Component {
            name: "hello-component".into(),
            source_url: "https://example.com/hello-component.zip".into(),
            component_ref: "main".into(),
            default: true,
            setup: Some(SetupCommand { command: "sh".into(), args: vec!["setup.sh".into()] }),
            health: Some(HealthCheck::FileExists { path: "marker.txt".into() }),
            supports_options_protocol: false,
        }
    }
```

Run: `cargo build --workspace`
Expected: succeeds (mlai-cli doesn't construct `Component` literals directly — it goes through `Manifest::parse`, which already handles the new `#[serde(default)]` field transparently).

- [x] **Step 6: Commit the manifest field**

```bash
git add crates/mlai-core/src/manifest.rs crates/mlai-core/src/pipeline.rs
git commit -m "feat(mlai-core): add supports_options_protocol manifest flag"
```

- [x] **Step 7: Write the failing test for the options-protocol probe**

`crates/mlai-core/src/options_protocol.rs`:
```rust
#[cfg(test)]
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
        let setup = SetupCommand { command: "sh".into(), args: vec!["setup.sh".into()] };

        let descriptor =
            describe_options(&setup, dir.path(), Duration::from_secs(5)).unwrap();

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
        let setup = SetupCommand { command: "sh".into(), args: vec!["setup.sh".into()] };

        let descriptor =
            describe_options(&setup, dir.path(), Duration::from_secs(5)).unwrap();

        assert_eq!(descriptor.options.len(), 1);
    }

    #[test]
    fn non_zero_exit_produces_actionable_error() {
        let dir = tempdir().unwrap();
        write_fixture_script(dir.path(), "exit 1");
        let setup = SetupCommand { command: "sh".into(), args: vec!["setup.sh".into()] };

        let err =
            describe_options(&setup, dir.path(), Duration::from_secs(5)).unwrap_err();
        assert!(matches!(err, OptionsError::NonZeroExit { status: 1, .. }));
    }

    #[test]
    fn unparseable_output_produces_actionable_error() {
        let dir = tempdir().unwrap();
        write_fixture_script(dir.path(), "echo 'not json'");
        let setup = SetupCommand { command: "sh".into(), args: vec!["setup.sh".into()] };

        let err =
            describe_options(&setup, dir.path(), Duration::from_secs(5)).unwrap_err();
        assert!(matches!(err, OptionsError::UnparseableJson { .. }));
    }

    #[test]
    fn slow_script_times_out() {
        let dir = tempdir().unwrap();
        write_fixture_script(dir.path(), "sleep 2");
        let setup = SetupCommand { command: "sh".into(), args: vec!["setup.sh".into()] };

        let err =
            describe_options(&setup, dir.path(), Duration::from_millis(200)).unwrap_err();
        assert!(matches!(err, OptionsError::Timeout { .. }));
    }
}
```

- [x] **Step 8: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test options_protocol::`
Expected: FAIL to compile — module `options_protocol` doesn't exist yet.

- [x] **Step 9: Write the implementation**

Prepend to the top of `crates/mlai-core/src/options_protocol.rs`:
```rust
use serde::Deserialize;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crate::manifest::SetupCommand;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct OptionsDescriptor {
    pub schema_version: u32,
    pub options: Vec<OptionSpec>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Deserialize, PartialEq)]
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
/// Per the protocol (and cinepipe-installer's own safety rationale), a
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

    let child = cmd
        .spawn()
        .map_err(|source| OptionsError::Launch { command: setup.command.clone(), source })?;

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
        Ok(Err(source)) => Err(OptionsError::Launch { command: command_name, source }),
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
    Ok(OptionsDescriptor { schema_version, options })
}
```

Add to `crates/mlai-core/src/lib.rs`:
```rust
pub mod options_protocol;
```

- [x] **Step 10: Run test to verify it passes**

Run: `cd crates/mlai-core && cargo test options_protocol::`
Expected: PASS — 5 tests.

- [x] **Step 11: Run the full mlai-core suite**

Run: `cd crates/mlai-core && cargo test`
Expected: PASS — all tests across manifest (6), state (3), backup (3), health (3), fetch (3), pipeline (3), options_protocol (5) = 26 tests.

- [x] **Step 12: Commit**

```bash
git add crates/mlai-core/src/options_protocol.rs crates/mlai-core/src/lib.rs
git commit -m "feat(mlai-core): add backend-options protocol probe (describe-options)"
```

---

### Task 4: Wire `--set key=value` through the pipeline

**Files:**
- Modify: `crates/mlai-core/src/pipeline.rs`
- Modify: `crates/mlai-cli/src/commands/install.rs`

**Interfaces:**
- Consumes: none new.
- Produces: `PipelineOptions.set_options: Vec<(String, String)>` (new field). `install_component` now appends `--set key=value` per pair to the setup command's args before running it.

- [x] **Step 1: Write the failing test**

In `crates/mlai-core/src/pipeline.rs`, add this to the existing `#[cfg(test)] mod tests` block (append after `backs_up_existing_install_before_replacing_it`), and add a second fixture-zip builder alongside the existing `build_fixture_zip`:
```rust
    fn build_fixture_zip_recording_args(path: &Path) {
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.add_directory("hello-component-main/", options).unwrap();
        zip.start_file("hello-component-main/setup.sh", options).unwrap();
        zip.write_all(b"#!/bin/sh\necho \"$@\" > args.txt\ntouch marker.txt\n").unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn set_options_are_appended_as_set_flags_to_setup() {
        let root = tempdir().unwrap();
        let fixture_dir = tempdir().unwrap();
        let zip_path = fixture_dir.path().join("bundle.zip");
        build_fixture_zip_recording_args(&zip_path);

        let mut component = sample_component();
        component.supports_options_protocol = true;

        let fetcher = FixtureFetcher { zip_path };
        let opts = PipelineOptions {
            install_root: root.path().to_path_buf(),
            fetcher: &fetcher,
            version: "abc123".into(),
            backup_keep: 3,
            set_options: vec![("model".to_string(), "qwen3:14b".to_string())],
        };

        install_component(&component, &opts).unwrap();

        let recorded_args =
            fs::read_to_string(root.path().join("hello-component").join("args.txt")).unwrap();
        assert!(recorded_args.contains("--set model=qwen3:14b"));
    }
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd crates/mlai-core && cargo test pipeline::`
Expected: FAIL to compile — `PipelineOptions` has no field `set_options`, and all existing `PipelineOptions { .. }` literals in this file's other tests are now missing a required field too.

- [x] **Step 3: Write the implementation**

In `crates/mlai-core/src/pipeline.rs`, modify the `PipelineOptions` struct:
```rust
pub struct PipelineOptions<'a> {
    pub install_root: PathBuf,
    pub fetcher: &'a dyn Fetcher,
    pub version: String,
    pub backup_keep: usize,
    pub set_options: Vec<(String, String)>,
}
```

Modify the call site inside `install_component` (find the line `run_setup(&component_dir, setup)?;`) to:
```rust
        run_setup(&component_dir, setup, &opts.set_options)?;
```

Modify `run_setup`'s signature and body:
```rust
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
        .map_err(|source| PipelineError::SetupLaunch { command: setup.command.clone(), source })?;
    if !status.success() {
        return Err(PipelineError::SetupFailed {
            command: setup.command.clone(),
            status: status.code().unwrap_or(-1),
        });
    }
    Ok(())
}
```

Fix the three existing `PipelineOptions { .. }` literals in this same file's test module (`installs_a_component_end_to_end_and_records_healthy_state`, `skips_reinstall_when_already_healthy_at_same_version`, `backs_up_existing_install_before_replacing_it`) by adding `set_options: vec![],` to each.

- [x] **Step 4: Fix the now-broken PipelineOptions construction in mlai-cli**

In `crates/mlai-cli/src/commands/install.rs`, modify the `PipelineOptions { .. }` literal inside the `for component in components` loop — add the new field:
```rust
        let opts = PipelineOptions {
            install_root: install_root.to_path_buf(),
            fetcher: &fetcher,
            version: component.component_ref.clone(),
            backup_keep: 3,
            set_options: Vec::new(),
        };
```
(Task 5 replaces `Vec::new()` with the real CLI-supplied value.)

- [x] **Step 5: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS — mlai-core's pipeline suite now has 4 tests (3 from Plan A + 1 new); full workspace suite green.

- [x] **Step 6: Commit**

```bash
git add crates/mlai-core/src/pipeline.rs crates/mlai-cli/src/commands/install.rs
git commit -m "feat(mlai-core): thread set_options through to --set key=value on setup"
```

---

### Task 5: `mlai-cli` — `install --set` and `credential set`

**Files:**
- Modify: `crates/mlai-cli/src/main.rs`
- Modify: `crates/mlai-cli/src/commands/install.rs`
- Modify: `crates/mlai-cli/src/commands/mod.rs`
- Create: `crates/mlai-cli/src/commands/credential.rs`
- Modify: `crates/mlai-cli/Cargo.toml`
- Create: `crates/mlai-cli/tests/credential.rs`

**Interfaces:**
- Consumes: `mlai_core::pipeline::PipelineOptions` (Task 4), `mlai_credentials::vault::{Vault, VaultConfig}` (Task 2).
- Produces: `mlai install --manifest <path> --install-root <dir> [--component <name>] [--set key=value]...` (repeatable). `mlai credential set <key> [--vault-dir <dir>]` (reads the secret value from stdin).

- [x] **Step 1: Add the mlai-credentials dependency**

Modify `crates/mlai-cli/Cargo.toml` — add to `[dependencies]`:
```toml
mlai-credentials = { path = "../mlai-credentials" }
```

- [x] **Step 2: Write the failing integration test for `install --set`**

In `crates/mlai-cli/tests/install.rs`, add this test (append after `install_command_fails_clearly_for_unknown_named_component`):
```rust
#[test]
fn install_command_rejects_set_for_a_component_without_protocol_support() {
    let manifest_dir = tempdir().unwrap();
    let manifest_path = manifest_dir.path().join("manifest.toml");
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
    let install_root = tempdir().unwrap();

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("install")
        .arg("--manifest")
        .arg(&manifest_path)
        .arg("--install-root")
        .arg(install_root.path())
        .arg("--set")
        .arg("model=qwen3:14b");

    cmd.assert()
        .failure()
        .stderr(contains("does not declare supports_options_protocol"));
}
```

- [x] **Step 3: Write the failing integration test for `credential set`**

`crates/mlai-cli/tests/credential.rs`:
```rust
use assert_cmd::Command;
use predicates::str::contains;
use tempfile::tempdir;

#[test]
fn credential_set_stores_a_secret_read_from_stdin() {
    let vault_dir = tempdir().unwrap();

    let mut cmd = Command::cargo_bin("mlai").unwrap();
    cmd.arg("credential")
        .arg("set")
        .arg("test-api-key")
        .arg("--vault-dir")
        .arg(vault_dir.path())
        .write_stdin("sk-secret-value\n");

    cmd.assert()
        .success()
        .stdout(contains("Stored credential 'test-api-key'"));

    let raw = std::fs::read(vault_dir.path().join("credentials.age")).unwrap();
    let raw_str = String::from_utf8_lossy(&raw);
    assert!(!raw_str.contains("sk-secret-value"));
}
```

- [x] **Step 4: Run tests to verify they fail**

Run: `cargo test --workspace`
Expected: FAIL to compile — `crates/mlai-cli/src/commands/credential.rs` doesn't exist yet, and `install`'s CLI doesn't accept `--set`.

- [x] **Step 5: Write `commands/credential.rs`**

`crates/mlai-cli/src/commands/credential.rs`:
```rust
use anyhow::{Context, Result};
use mlai_credentials::vault::{Vault, VaultConfig};
use std::path::PathBuf;

pub fn set(key: &str, vault_dir: Option<PathBuf>) -> Result<()> {
    let vault_dir = vault_dir.unwrap_or_else(default_vault_dir);
    eprintln!(
        "Enter value for '{key}' (input is not hidden in this version — avoid a shared terminal):"
    );
    let mut value = String::new();
    std::io::stdin()
        .read_line(&mut value)
        .context("reading secret value from stdin")?;
    let value = value.trim_end_matches(['\n', '\r']);

    let config = VaultConfig {
        vault_dir,
        keyring_service: "mlai-installer-vault".to_string(),
        keyring_user: "credential-vault-age-identity".to_string(),
    };
    let mut vault = Vault::open(config, true).context("opening credential vault")?;
    vault.set(key, value).context("writing credential to vault")?;
    println!("Stored credential '{key}'.");
    Ok(())
}

fn default_vault_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".mlai").join("credentials")
}
```

Modify `crates/mlai-cli/src/commands/mod.rs`:
```rust
pub mod credential;
pub mod install;
```

- [x] **Step 6: Wire up the CLI surface**

Modify `crates/mlai-cli/src/main.rs`:
```rust
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod commands;

#[derive(Parser)]
#[command(name = "mlai", version, about = "MLAppInstaller: cross-platform installer engine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install components from a manifest
    Install {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        install_root: PathBuf,
        #[arg(long)]
        component: Option<String>,
        /// Backend option to pass to a component's setup (key=value, repeatable).
        /// Only valid for components with supports_options_protocol = true.
        #[arg(long = "set", value_parser = parse_set_option)]
        set: Vec<(String, String)>,
    },
    /// Manage stored credentials (hosted-model API keys, etc.)
    Credential {
        #[command(subcommand)]
        action: CredentialAction,
    },
}

#[derive(Subcommand)]
enum CredentialAction {
    /// Store a secret value (read from stdin) under the given key
    Set {
        key: String,
        #[arg(long)]
        vault_dir: Option<PathBuf>,
    },
}

fn parse_set_option(s: &str) -> Result<(String, String), String> {
    match s.split_once('=') {
        Some((k, v)) => Ok((k.to_string(), v.to_string())),
        None => Err(format!("invalid --set value '{s}': expected key=value")),
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Install { manifest, install_root, component, set } => {
            commands::install::run(&manifest, &install_root, component.as_deref(), &set)
        }
        Commands::Credential { action } => match action {
            CredentialAction::Set { key, vault_dir } => {
                commands::credential::set(&key, vault_dir)
            }
        },
    }
}
```

- [x] **Step 7: Wire `--set` through `commands/install.rs`**

Modify `crates/mlai-cli/src/commands/install.rs` — change the function signature and add the protocol-support gate:
```rust
pub fn run(
    manifest_path: &Path,
    install_root: &Path,
    component_name: Option<&str>,
    set_options: &[(String, String)],
) -> Result<()> {
    let manifest_str = fs::read_to_string(manifest_path)
        .with_context(|| format!("reading manifest at {}", manifest_path.display()))?;
    let manifest = Manifest::parse(&manifest_str)
        .with_context(|| format!("parsing manifest at {}", manifest_path.display()))?;

    let components: Vec<_> = match component_name {
        Some(name) => match manifest.find_component(name) {
            Some(c) => vec![c],
            None => bail!("no component named '{name}' in {}", manifest_path.display()),
        },
        None => manifest.default_components(),
    };

    if components.is_empty() {
        bail!("no components to install (manifest has no default components and none were named)");
    }

    fs::create_dir_all(install_root)
        .with_context(|| format!("creating install root at {}", install_root.display()))?;

    let fetcher = HttpFetcher { token: std::env::var("MLAI_TOKEN").ok() };

    for component in components {
        if !set_options.is_empty() && !component.supports_options_protocol {
            bail!(
                "--set was provided but component '{}' does not declare supports_options_protocol = true in the manifest",
                component.name
            );
        }
        println!("Installing {}...", component.name);
        let opts = PipelineOptions {
            install_root: install_root.to_path_buf(),
            fetcher: &fetcher,
            version: component.component_ref.clone(),
            backup_keep: 3,
            set_options: set_options.to_vec(),
        };
        let result = install_component(component, &opts)
            .with_context(|| format!("installing component '{}'", component.name))?;
        match result {
            ComponentState::Healthy => println!("  {} -> healthy", component.name),
            other => println!("  {} -> {other:?} (NEEDS ATTENTION)", component.name),
        }
    }

    Ok(())
}
```

- [x] **Step 8: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS — all `mlai-core` and `mlai-credentials` unit tests, plus 4 `mlai-cli` integration tests in `install.rs` (2 from Plan A + the new `--set` rejection test... note the original 2 tests from Plan A still pass unmodified since `run()`'s new 4th parameter is additive at call sites within those tests only via the CLI binary, not a direct Rust call) plus 1 in `credential.rs`.

- [x] **Step 9: Commit**

```bash
git add crates/mlai-cli
git commit -m "feat(mlai-cli): add install --set and credential set commands"
```

---

### Task 6: Docs + final verification

**Files:**
- Modify: `docs/USAGE.md`

**Interfaces:** none new.

- [x] **Step 1: Run the full constitution-required check suite locally**

Run:
```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```
Expected: all four PASS. Fix any `cargo fmt --all` or clippy findings before proceeding (docs/CONSTITUTION.md §5).

- [x] **Step 2: Update `docs/USAGE.md`**

Add this section to `docs/USAGE.md`, after the existing "Private sources" section and before "Not yet implemented":
```markdown
## Backend options protocol

A component can declare `supports_options_protocol = true` in the manifest
to expose local-vs-hosted choices. `mlai` never probes or passes options to
a component that hasn't declared this — an unpatched setup script could
otherwise silently run its real setup instead of erroring on an unknown
flag.

```bash
mlai install --manifest manifest.toml --install-root ~/my-app --set model=qwen3:14b
```

`--set key=value` is repeatable and passed straight through to the
component's setup command, verbatim compatible with cinepipe-installer's
existing `--set key=value` convention (see
`docs/superpowers/specs/2026-08-14-foundation-design.md`).

## Credentials

Hosted-model API keys and other secrets are never stored in plaintext.
`mlai credential set <key>` stores a value (read from stdin) in an
age-encrypted vault, using the OS keychain when available and falling back
to a chmod-0600 file otherwise (with a loud warning when that fallback is
used):

```bash
echo "sk-your-api-key" | mlai credential set openai-api-key
```

**Known v1 limitation**: stdin input is not hidden (no `*` masking) — avoid
running this on a shared terminal. A proper hidden-input UX is a follow-up.
```

Also update the "Not yet implemented" list at the bottom to remove "local-vs-hosted backend selection" (now implemented) — leaving:
```markdown
## Not yet implemented

`repair`, `uninstall`, `update`, and cloud config generation are planned
follow-ups — see `docs/superpowers/specs/2026-08-14-foundation-design.md`.
```

- [x] **Step 3: Commit**

```bash
git add docs/USAGE.md
git commit -m "docs: document backend options protocol and credential vault"
```

- [x] **Step 4: Final full-workspace verification**

Run:
```bash
cargo test --workspace
```
Expected: PASS. Total test count across the workspace: `mlai-core` (manifest 6, state 3, backup 3, health 3, fetch 3, pipeline 4, options_protocol 5 = 27), `mlai-credentials` (encryption 5, vault 4 = 9), `mlai-cli` (install 3, credential 1 = 4). 40 tests total.

---

## Self-Review Notes

- **Spec coverage**: credential vault (age + keychain + fallback, generalized keyring namespace) and the backend-options protocol (verbatim cinepipe-compatible `--describe-options`/`--set key=value`, forward-compatible unknown-type handling, manifest-flag-gated probing) are both covered per `docs/superpowers/specs/2026-08-14-foundation-design.md`'s "Local vs. cloud backend selection" section. Automatic interactive prompting during `mlai install` (the full "ask → store → pass" UX) is explicitly out of scope for this plan, not a gap — see "Out of scope" above.
- **Placeholder scan**: no TBD/TODO markers; every step has complete, runnable code, including the fixes to Plan A's existing `PipelineOptions`/`Component` construction sites broken by this plan's new required fields.
- **Type consistency**: `VaultConfig`, `Vault`, `KeyCustody`, `OptionsDescriptor`, `OptionSpec`, `describe_options`, and `PipelineOptions.set_options` are each defined once and consumed with matching names/signatures in every later task (Task 5's CLI code imports Task 2's and Task 4's exact public signatures).
