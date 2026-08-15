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
        Ok(secret) => {
            Identity::from_str(&secret).map_err(|e| format!("stored age identity is corrupt: {e}"))
        }
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

        // age::x25519::Identity doesn't implement Debug in age 0.12, so
        // Result::unwrap_err() (which requires T: Debug) can't be used here.
        match load_or_create_identity(dir.path(), "mlai-test-service", "test-user", false) {
            Err(err) => assert!(matches!(err, VaultError::KeyUnreadable { .. })),
            Ok(_) => panic!("expected an error for a corrupt fallback key file"),
        }
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
