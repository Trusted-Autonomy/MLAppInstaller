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
