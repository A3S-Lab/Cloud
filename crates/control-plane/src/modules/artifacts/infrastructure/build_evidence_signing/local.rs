use crate::modules::artifacts::domain::{
    sha256_digest, BuildEvidenceSigningError, BuildEvidenceSigningKey, IBuildEvidenceSigner,
    VerifiedBuildEvidenceSignature,
};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

const MAX_KEY_BYTES: usize = 4096;
const MAX_PAE_BYTES: usize = 64 * 1024 * 1024 + 1024;

pub struct LocalBuildEvidenceSigner {
    key_path: PathBuf,
    key: Arc<Ed25519KeyPair>,
    key_id: String,
}

impl LocalBuildEvidenceSigner {
    pub async fn load_or_create(
        key_path: impl Into<PathBuf>,
    ) -> Result<Self, BuildEvidenceSigningError> {
        let key_path = key_path.into();
        validate_key_path(&key_path)?;
        let path = key_path.clone();
        let pkcs8 = tokio::task::spawn_blocking(move || load_or_create_pkcs8(&path))
            .await
            .map_err(|error| {
                BuildEvidenceSigningError::Unavailable(format!(
                    "local build evidence key task failed: {error}"
                ))
            })??;
        let key = Ed25519KeyPair::from_pkcs8(&pkcs8).map_err(|_| {
            BuildEvidenceSigningError::Rejected(
                "local build evidence key is not valid Ed25519 PKCS#8".into(),
            )
        })?;
        let key_id = sha256_digest(key.public_key().as_ref());
        Ok(Self {
            key_path,
            key: Arc::new(key),
            key_id,
        })
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn key_path(&self) -> &Path {
        &self.key_path
    }
}

#[async_trait]
impl IBuildEvidenceSigner for LocalBuildEvidenceSigner {
    async fn sign(
        &self,
        pae: &[u8],
    ) -> Result<VerifiedBuildEvidenceSignature, BuildEvidenceSigningError> {
        validate_pae(pae)?;
        let signature = self.key.sign(pae);
        UnparsedPublicKey::new(&ED25519, self.key.public_key().as_ref())
            .verify(pae, signature.as_ref())
            .map_err(|_| {
                BuildEvidenceSigningError::Rejected(
                    "local build evidence signature failed Ed25519 verification".into(),
                )
            })?;
        VerifiedBuildEvidenceSignature::new(
            BuildEvidenceSigningKey {
                algorithm: "ed25519".into(),
                key_id: self.key_id.clone(),
                public_key: STANDARD.encode(self.key.public_key().as_ref()),
                key_version: None,
            },
            signature.as_ref().to_vec(),
        )
    }
}

fn validate_key_path(path: &Path) -> Result<(), BuildEvidenceSigningError> {
    let value = path.to_str().ok_or_else(|| {
        BuildEvidenceSigningError::Invalid(
            "local build evidence key path must be valid UTF-8".into(),
        )
    })?;
    if value.trim().is_empty()
        || value.len() > 4096
        || value.contains(['\0', '\r', '\n'])
        || path.file_name().is_none()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(BuildEvidenceSigningError::Invalid(
            "local build evidence key path is invalid".into(),
        ));
    }
    Ok(())
}

fn validate_pae(pae: &[u8]) -> Result<(), BuildEvidenceSigningError> {
    if pae.is_empty() || pae.len() > MAX_PAE_BYTES {
        return Err(BuildEvidenceSigningError::Invalid(
            "DSSE PAE exceeds the build evidence signing bound".into(),
        ));
    }
    Ok(())
}

fn load_or_create_pkcs8(path: &Path) -> Result<Vec<u8>, BuildEvidenceSigningError> {
    match read_existing_key(path) {
        Ok(key) => return Ok(key),
        Err(BuildEvidenceSigningError::Unavailable(message))
            if message == "local build evidence key does not exist" => {}
        Err(error) => return Err(error),
    }
    let parent = path.parent().ok_or_else(|| {
        BuildEvidenceSigningError::Invalid(
            "local build evidence key path has no parent directory".into(),
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        BuildEvidenceSigningError::Unavailable(format!(
            "could not create local build evidence key directory: {error}"
        ))
    })?;
    secure_directory(parent)?;
    let generated = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| {
        BuildEvidenceSigningError::Unavailable(
            "could not generate local Ed25519 build evidence key".into(),
        )
    })?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            file.write_all(generated.as_ref()).map_err(|error| {
                BuildEvidenceSigningError::Unavailable(format!(
                    "could not write local build evidence key: {error}"
                ))
            })?;
            file.sync_all().map_err(|error| {
                BuildEvidenceSigningError::Unavailable(format!(
                    "could not sync local build evidence key: {error}"
                ))
            })?;
            validate_key_metadata(path)?;
            Ok(generated.as_ref().to_vec())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => read_existing_key(path),
        Err(error) => Err(BuildEvidenceSigningError::Unavailable(format!(
            "could not create local build evidence key: {error}"
        ))),
    }
}

fn read_existing_key(path: &Path) -> Result<Vec<u8>, BuildEvidenceSigningError> {
    let file = match OpenOptions::new().read(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(BuildEvidenceSigningError::Unavailable(
                "local build evidence key does not exist".into(),
            ))
        }
        Err(error) => {
            return Err(BuildEvidenceSigningError::Unavailable(format!(
                "could not open local build evidence key: {error}"
            )))
        }
    };
    validate_key_metadata(path)?;
    let metadata = file.metadata().map_err(|error| {
        BuildEvidenceSigningError::Unavailable(format!(
            "could not inspect local build evidence key: {error}"
        ))
    })?;
    if metadata.len() == 0 || metadata.len() > MAX_KEY_BYTES as u64 {
        return Err(BuildEvidenceSigningError::Rejected(
            "local build evidence key exceeds its byte bound".into(),
        ));
    }
    let mut key = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_KEY_BYTES as u64 + 1)
        .read_to_end(&mut key)
        .map_err(|error| {
            BuildEvidenceSigningError::Unavailable(format!(
                "could not read local build evidence key: {error}"
            ))
        })?;
    if key.len() > MAX_KEY_BYTES {
        return Err(BuildEvidenceSigningError::Rejected(
            "local build evidence key exceeds its byte bound".into(),
        ));
    }
    Ok(key)
}

fn secure_directory(path: &Path) -> Result<(), BuildEvidenceSigningError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        BuildEvidenceSigningError::Unavailable(format!(
            "could not inspect local build evidence key directory: {error}"
        ))
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(BuildEvidenceSigningError::Rejected(
            "local build evidence key directory is not an owned directory".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
            BuildEvidenceSigningError::Unavailable(format!(
                "could not secure local build evidence key directory: {error}"
            ))
        })?;
    }
    Ok(())
}

fn validate_key_metadata(path: &Path) -> Result<(), BuildEvidenceSigningError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        BuildEvidenceSigningError::Unavailable(format!(
            "could not inspect local build evidence key: {error}"
        ))
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(BuildEvidenceSigningError::Rejected(
            "local build evidence key is not a regular file".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(BuildEvidenceSigningError::Rejected(
                "local build evidence key permissions must be 0600 or stricter".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::artifacts::domain::dsse_pae;

    #[tokio::test]
    async fn local_signer_persists_one_private_ed25519_key_with_private_permissions() {
        let root = tempfile::tempdir().expect("temporary directory");
        let key_path = root.path().join("nested/build-evidence-ed25519.pk8");
        let first = LocalBuildEvidenceSigner::load_or_create(&key_path)
            .await
            .expect("first signer");
        let pae = dsse_pae("application/vnd.in-toto+json", b"{}").expect("DSSE PAE");
        let first_signature = first.sign(&pae).await.expect("first signature");

        let second = LocalBuildEvidenceSigner::load_or_create(&key_path)
            .await
            .expect("reloaded signer");
        let second_signature = second.sign(&pae).await.expect("second signature");

        assert_eq!(first.key_id(), second.key_id());
        assert_eq!(first_signature, second_signature);
        assert_eq!(first_signature.key.algorithm, "ed25519");
        assert_eq!(
            first_signature.key.public_key,
            STANDARD.encode(first.key.public_key().as_ref())
        );
        assert_eq!(first_signature.key.key_version, None);
        assert_eq!(first_signature.signature.len(), 64);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&key_path)
                    .expect("key metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_signer_rejects_a_private_key_readable_by_other_users() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temporary directory");
        let key_path = root.path().join("build-evidence-ed25519.pk8");
        LocalBuildEvidenceSigner::load_or_create(&key_path)
            .await
            .expect("initial signer");
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o644))
            .expect("relax key permissions");

        assert!(matches!(
            LocalBuildEvidenceSigner::load_or_create(&key_path).await,
            Err(BuildEvidenceSigningError::Rejected(_))
        ));
    }
}
