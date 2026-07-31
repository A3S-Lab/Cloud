use super::oci_layout::{validate_oci_layout, OciLayoutLimits, ValidatedOciOutput};
use crate::modules::artifacts::domain::{BuildOutputValidationError, OciDescriptor};
use crate::modules::sources::domain::BuildPlatform;
use serde::Deserialize;
use std::path::Path;
use tokio::io::AsyncReadExt;

const MAX_METADATA_BYTES: u64 = 1024 * 1024;

pub(super) async fn validate_exported_output(
    root: &Path,
    expected_platforms: &[BuildPlatform],
    max_blobs: usize,
    max_bytes: u64,
) -> Result<ValidatedOciOutput, BuildOutputValidationError> {
    let metadata = root.join("buildkit-metadata.json");
    let layout = root.join("oci");
    let descriptor = read_buildkit_descriptor(&metadata).await?;
    remove_empty_ingest_directory(&layout).await?;
    validate_oci_layout(
        &layout,
        &descriptor,
        expected_platforms,
        OciLayoutLimits::new(max_blobs, max_bytes)?,
    )
    .await
}

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

async fn read_buildkit_descriptor(
    path: &Path,
) -> Result<OciDescriptor, BuildOutputValidationError> {
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

async fn remove_empty_ingest_directory(layout: &Path) -> Result<(), BuildOutputValidationError> {
    let ingest = layout.join("ingest");
    let metadata = match tokio::fs::symlink_metadata(&ingest).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(storage("could not inspect BuildKit ingest directory")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(integrity("BuildKit ingest path is not an owned directory"));
    }
    let mut entries = tokio::fs::read_dir(&ingest)
        .await
        .map_err(|_| storage("could not inspect BuildKit ingest directory"))?;
    if entries
        .next_entry()
        .await
        .map_err(|_| storage("could not inspect BuildKit ingest directory"))?
        .is_some()
    {
        return Err(integrity(
            "BuildKit left an incomplete content-store ingest",
        ));
    }
    tokio::fs::remove_dir(ingest)
        .await
        .map_err(|_| storage("could not remove empty BuildKit ingest directory"))
}

fn integrity(message: impl Into<String>) -> BuildOutputValidationError {
    BuildOutputValidationError::Integrity(message.into())
}

fn storage(message: impl Into<String>) -> BuildOutputValidationError {
    BuildOutputValidationError::Storage(message.into())
}

#[cfg(test)]
mod tests {
    use super::read_buildkit_descriptor;
    use crate::modules::artifacts::domain::BuildOutputValidationError;
    use serde_json::json;

    #[tokio::test]
    async fn metadata_digest_must_match_its_root_descriptor() {
        let root = tempfile::tempdir().expect("BuildKit metadata fixture");
        let metadata = root.path().join("buildkit-metadata.json");
        let descriptor_digest = format!("sha256:{}", "a".repeat(64));
        tokio::fs::write(
            &metadata,
            serde_json::to_vec(&json!({
                "containerimage.digest": format!("sha256:{}", "b".repeat(64)),
                "containerimage.descriptor": {
                    "mediaType": "application/vnd.oci.image.manifest.v1+json",
                    "digest": descriptor_digest,
                    "size": 128,
                },
            }))
            .expect("BuildKit metadata encoding"),
        )
        .await
        .expect("BuildKit metadata");

        assert!(matches!(
            read_buildkit_descriptor(&metadata).await,
            Err(BuildOutputValidationError::Integrity(_))
        ));
    }
}
