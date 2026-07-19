use crate::modules::secrets::domain::{
    EncryptedSecretValue, ISecretEncryptionService, SecretEncryptionError,
};
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;

pub struct LocalKeyEncryptionService {
    key_path: PathBuf,
    key_id: String,
    cipher: Aes256Gcm,
}

impl LocalKeyEncryptionService {
    pub fn load_or_create(key_path: impl Into<PathBuf>) -> Result<Self, SecretEncryptionError> {
        let key_path = key_path.into();
        let key = match fs::read(&key_path) {
            Ok(key) => key,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => create_key(&key_path)?,
            Err(error) => {
                return Err(SecretEncryptionError::Unavailable(format!(
                    "could not read local encryption key: {error}"
                )))
            }
        };
        if key.len() != KEY_BYTES {
            return Err(SecretEncryptionError::Rejected(
                "local encryption key must contain exactly 32 bytes".into(),
            ));
        }
        let key_id = format!("local:sha256:{:x}", Sha256::digest(&key));
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|error| {
            SecretEncryptionError::Rejected(format!("local encryption key is invalid: {error}"))
        })?;
        Ok(Self {
            key_path,
            key_id,
            cipher,
        })
    }
}

#[async_trait]
impl ISecretEncryptionService for LocalKeyEncryptionService {
    async fn encrypt(
        &self,
        plaintext: &[u8],
        context: &[u8],
    ) -> Result<EncryptedSecretValue, SecretEncryptionError> {
        if plaintext.is_empty() || plaintext.len() > 1024 * 1024 || context.len() > 16 * 1024 {
            return Err(SecretEncryptionError::InvalidInput(
                "local encryption input exceeds protocol bounds".into(),
            ));
        }
        let mut nonce = [0_u8; NONCE_BYTES];
        getrandom::fill(&mut nonce).map_err(|error| {
            SecretEncryptionError::Unavailable(format!(
                "could not generate encryption nonce: {error}"
            ))
        })?;
        let ciphertext = self
            .cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: context,
                },
            )
            .map_err(|_| SecretEncryptionError::Rejected("local encryption failed".into()))?;
        EncryptedSecretValue::new(
            self.key_id.clone(),
            format!(
                "v1.{}.{}",
                URL_SAFE_NO_PAD.encode(nonce),
                URL_SAFE_NO_PAD.encode(ciphertext)
            ),
        )
        .map_err(SecretEncryptionError::Rejected)
    }

    async fn decrypt(
        &self,
        value: &EncryptedSecretValue,
        context: &[u8],
    ) -> Result<Vec<u8>, SecretEncryptionError> {
        if value.key_id() != self.key_id || context.len() > 16 * 1024 {
            return Err(SecretEncryptionError::Rejected(
                "encrypted value belongs to another key or context is invalid".into(),
            ));
        }
        let mut parts = value.ciphertext().split('.');
        if parts.next() != Some("v1") {
            return Err(SecretEncryptionError::Rejected(
                "encrypted value uses an unsupported version".into(),
            ));
        }
        let nonce = parts
            .next()
            .ok_or_else(|| SecretEncryptionError::Rejected("encrypted value has no nonce".into()))
            .and_then(decode)?;
        let ciphertext = parts
            .next()
            .ok_or_else(|| SecretEncryptionError::Rejected("encrypted value has no payload".into()))
            .and_then(decode)?;
        if parts.next().is_some()
            || nonce.len() != NONCE_BYTES
            || ciphertext.len() > 2 * 1024 * 1024
        {
            return Err(SecretEncryptionError::Rejected(
                "encrypted value is malformed".into(),
            ));
        }
        self.cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: context,
                },
            )
            .map_err(|_| {
                SecretEncryptionError::Rejected("encrypted value authentication failed".into())
            })
    }

    async fn health(&self) -> Result<bool, SecretEncryptionError> {
        Ok(self.key_path.is_file())
    }
}

fn create_key(path: &Path) -> Result<Vec<u8>, SecretEncryptionError> {
    let parent = path.parent().ok_or_else(|| {
        SecretEncryptionError::InvalidInput("local encryption key path has no parent".into())
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        SecretEncryptionError::Unavailable(format!(
            "could not create local encryption directory: {error}"
        ))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|error| {
            SecretEncryptionError::Unavailable(format!(
                "could not secure local encryption directory: {error}"
            ))
        })?;
    }
    let mut key = vec![0_u8; KEY_BYTES];
    getrandom::fill(&mut key).map_err(|error| {
        SecretEncryptionError::Unavailable(format!(
            "could not generate local encryption key: {error}"
        ))
    })?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| {
        SecretEncryptionError::Unavailable(format!(
            "could not create local encryption key: {error}"
        ))
    })?;
    file.write_all(&key).map_err(|error| {
        SecretEncryptionError::Unavailable(format!("could not write local encryption key: {error}"))
    })?;
    file.sync_all().map_err(|error| {
        SecretEncryptionError::Unavailable(format!("could not sync local encryption key: {error}"))
    })?;
    Ok(key)
}

fn decode(value: &str) -> Result<Vec<u8>, SecretEncryptionError> {
    URL_SAFE_NO_PAD.decode(value).map_err(|_| {
        SecretEncryptionError::Rejected("encrypted value contains invalid base64".into())
    })
}
