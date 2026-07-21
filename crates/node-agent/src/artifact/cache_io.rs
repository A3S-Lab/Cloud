use super::store::NodeArtifactError;
use a3s_cloud_contracts::NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE;
use a3s_runtime::contract::{ArtifactRef, RuntimeOutputArtifact};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

pub(super) const BLOB_RECEIPT_SCHEMA: &str = "a3s.cloud.node-local-artifact-blob.v1";
pub(super) const MOUNT_RECEIPT_SCHEMA: &str = "a3s.cloud.node-local-artifact-mount.v1";
pub(super) const OUTPUT_RECEIPT_SCHEMA: &str = "a3s.cloud.node-local-artifact-output.v1";
const LOCAL_URI_PREFIX: &str = "a3s-node-artifact://sha256/";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BlobReceipt {
    pub(super) schema: String,
    pub(super) digest: String,
    pub(super) media_type: String,
    pub(super) size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MountReceipt {
    pub(super) schema: String,
    pub(super) spec_digest: String,
    pub(super) mount_name: String,
    pub(super) artifact: ArtifactRef,
    pub(super) size_bytes: u64,
    pub(super) entry_count: usize,
    pub(super) expanded_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OutputReceipt {
    pub(super) schema: String,
    pub(super) spec_digest: String,
    pub(super) output: RuntimeOutputArtifact,
}

pub(super) fn digest_hex(value: &str) -> Result<&str, NodeArtifactError> {
    value
        .strip_prefix("sha256:")
        .filter(|hex| {
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or_else(|| NodeArtifactError::Invalid("digest must be lowercase SHA-256".into()))
}

pub(super) fn name_key(name: &str) -> Result<String, NodeArtifactError> {
    if name.trim().is_empty() || name.len() > 255 || name.contains(['\0', '\r', '\n']) {
        return Err(NodeArtifactError::Invalid(
            "artifact name is invalid".into(),
        ));
    }
    Ok(format!("{:x}", Sha256::digest(name.as_bytes())))
}

pub(super) fn local_artifact_uri(digest: &str) -> Result<String, NodeArtifactError> {
    Ok(format!("{LOCAL_URI_PREFIX}{}", digest_hex(digest)?))
}

pub(super) fn validate_local_artifact(artifact: &ArtifactRef) -> Result<(), NodeArtifactError> {
    artifact.validate().map_err(NodeArtifactError::Invalid)?;
    if artifact.uri != local_artifact_uri(&artifact.digest)?
        || artifact.media_type != NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE
    {
        return Err(NodeArtifactError::Integrity(
            "node-local artifact URI, digest, or media type is invalid".into(),
        ));
    }
    Ok(())
}

pub(super) async fn verify_file(
    path: &Path,
    expected_digest: &str,
    expected_size: u64,
) -> Result<bool, NodeArtifactError> {
    let mut file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(storage(error)),
    };
    let metadata = file.metadata().await.map_err(storage)?;
    if !metadata.is_file() || metadata.len() != expected_size {
        return Ok(false);
    }
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).await.map_err(storage)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", digest.finalize()) == expected_digest)
}

pub(super) async fn is_directory(path: &Path) -> Result<bool, NodeArtifactError> {
    match tokio::fs::metadata(path).await {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(storage(error)),
    }
}

pub(super) async fn is_regular_file(path: &Path) -> Result<bool, NodeArtifactError> {
    match tokio::fs::metadata(path).await {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(storage(error)),
    }
}

pub(super) async fn seal_regular_file(path: &Path) -> Result<(), NodeArtifactError> {
    let metadata = tokio::fs::metadata(path).await.map_err(storage)?;
    if !metadata.is_file() {
        return Err(NodeArtifactError::Integrity(
            "artifact blob is not a regular file".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o444))
            .await
            .map_err(storage)?;
    }
    #[cfg(not(unix))]
    {
        let mut permissions = metadata.permissions();
        permissions.set_readonly(true);
        tokio::fs::set_permissions(path, permissions)
            .await
            .map_err(storage)?;
    }
    Ok(())
}

pub(super) async fn is_read_only_file(path: &Path) -> Result<bool, NodeArtifactError> {
    let metadata = tokio::fs::metadata(path).await.map_err(storage)?;
    if !metadata.is_file() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(metadata.permissions().mode() & 0o222 == 0)
    }
    #[cfg(not(unix))]
    {
        Ok(metadata.permissions().readonly())
    }
}

pub(super) async fn read_required_json<T>(path: &Path) -> Result<T, NodeArtifactError>
where
    T: DeserializeOwned,
{
    read_optional_json(path)
        .await?
        .ok_or_else(|| NodeArtifactError::Integrity("artifact cache receipt is missing".into()))
}

pub(super) async fn read_optional_json<T>(path: &Path) -> Result<Option<T>, NodeArtifactError>
where
    T: DeserializeOwned,
{
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(storage(error)),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| NodeArtifactError::Integrity("artifact cache receipt is invalid".into()))
}

pub(super) async fn write_json_atomic<T>(path: &Path, value: &T) -> Result<(), NodeArtifactError>
where
    T: Serialize,
{
    let parent = path
        .parent()
        .ok_or_else(|| NodeArtifactError::Storage("artifact receipt path has no parent".into()))?;
    tokio::fs::create_dir_all(parent).await.map_err(storage)?;
    let bytes = serde_json::to_vec(value).map_err(|error| {
        NodeArtifactError::Storage(format!("could not encode receipt: {error}"))
    })?;
    let staging = parent.join(format!(".{}.tmp", Uuid::now_v7()));
    let result = async {
        let mut file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&staging)
            .await
            .map_err(|error| {
                NodeArtifactError::Storage(format!(
                    "could not create receipt staging file: {error}"
                ))
            })?;
        file.write_all(&bytes).await.map_err(|error| {
            NodeArtifactError::Storage(format!("could not write receipt staging file: {error}"))
        })?;
        file.sync_all().await.map_err(|error| {
            NodeArtifactError::Storage(format!("could not sync receipt staging file: {error}"))
        })?;
        drop(file);
        tokio::fs::rename(&staging, path).await.map_err(|error| {
            NodeArtifactError::Storage(format!("could not commit artifact receipt: {error}"))
        })
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&staging).await;
    }
    result
}

pub(super) async fn remove_tree(path: PathBuf) -> Result<(), NodeArtifactError> {
    tokio::task::spawn_blocking(move || {
        make_tree_writable(&path)?;
        std::fs::remove_dir_all(&path)
            .map_err(|error| format!("could not remove artifact tree {}: {error}", path.display()))
    })
    .await
    .map_err(|error| NodeArtifactError::Storage(format!("artifact cleanup task failed: {error}")))?
    .map_err(NodeArtifactError::Storage)
}

fn make_tree_writable(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    for entry in
        std::fs::read_dir(path).map_err(|error| format!("could not scan artifact tree: {error}"))?
    {
        let entry = entry.map_err(|error| format!("could not scan artifact entry: {error}"))?;
        let metadata = std::fs::symlink_metadata(entry.path())
            .map_err(|error| format!("could not inspect artifact entry: {error}"))?;
        if metadata.is_dir() {
            set_writable(&entry.path())?;
            make_tree_writable(&entry.path())?;
        }
    }
    set_writable(path)
}

#[cfg(unix)]
fn set_writable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect artifact permissions: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    let owner_mode = if metadata.is_dir() { 0o700 } else { 0o600 };
    let mode = metadata.permissions().mode() | owner_mode;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|error| format!("could not restore artifact permissions: {error}"))
}

#[cfg(not(unix))]
fn set_writable(path: &Path) -> Result<(), String> {
    let mut permissions = std::fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect artifact permissions: {error}"))?
        .permissions();
    permissions.set_readonly(false);
    std::fs::set_permissions(path, permissions)
        .map_err(|error| format!("could not restore artifact permissions: {error}"))
}

pub(super) fn storage(error: std::io::Error) -> NodeArtifactError {
    NodeArtifactError::Storage(error.to_string())
}
