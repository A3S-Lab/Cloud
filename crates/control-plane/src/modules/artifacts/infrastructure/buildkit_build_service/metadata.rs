use crate::modules::artifacts::domain::{BuildServiceError, OciDescriptor};
use serde::Deserialize;
use std::path::Path;
use tokio::io::AsyncReadExt;

const MAX_METADATA_BYTES: u64 = 1024 * 1024;
#[derive(Deserialize)]
struct BuildkitMetadata {
    #[serde(rename = "containerimage.digest")]
    digest: String,
    #[serde(rename = "containerimage.descriptor")]
    descriptor: RawDescriptor,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDescriptor {
    media_type: String,
    digest: String,
    size: u64,
}

pub(super) async fn read_buildkit_descriptor(
    path: &Path,
) -> Result<OciDescriptor, BuildServiceError> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|_| integrity("BuildKit metadata file is unavailable"))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_METADATA_BYTES
    {
        return Err(integrity("BuildKit metadata file is invalid"));
    }
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|_| storage("could not read BuildKit metadata"))?;
    let mut encoded = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut encoded)
        .await
        .map_err(|_| storage("could not read BuildKit metadata"))?;
    if encoded.len() as u64 > MAX_METADATA_BYTES {
        return Err(integrity("BuildKit metadata exceeds its bound"));
    }
    let metadata: BuildkitMetadata =
        serde_json::from_slice(&encoded).map_err(|_| integrity("BuildKit metadata is invalid"))?;
    let descriptor = OciDescriptor::new(
        metadata.descriptor.media_type,
        metadata.descriptor.digest,
        metadata.descriptor.size,
    )
    .map_err(integrity)?;
    if descriptor.digest() != metadata.digest {
        return Err(integrity("BuildKit digest does not match its descriptor"));
    }
    Ok(descriptor)
}

fn integrity(message: impl Into<String>) -> BuildServiceError {
    BuildServiceError::Integrity(message.into())
}

fn storage(message: impl Into<String>) -> BuildServiceError {
    BuildServiceError::Storage(message.into())
}
