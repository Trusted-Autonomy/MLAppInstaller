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
    vault
        .set(key, value)
        .context("writing credential to vault")?;
    println!("Stored credential '{key}'.");
    Ok(())
}

fn default_vault_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".mlai").join("credentials")
}
