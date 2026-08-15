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
