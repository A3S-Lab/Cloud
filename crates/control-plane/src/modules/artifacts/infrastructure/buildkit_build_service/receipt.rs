use crate::modules::artifacts::domain::{
    BuildServiceError, BuiltOciArtifact, OciBuildRequest, OciDescriptor,
};
use crate::modules::sources::domain::BuildPlatform;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

pub(super) const RECEIPT_SCHEMA: &str = "a3s.cloud.oci-build-output.v1";
const MAX_RECEIPT_BYTES: u64 = 64 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct BuildReceipt {
    pub(super) schema: String,
    pub(super) build_id: Uuid,
    pub(super) source_content_digest: String,
    pub(super) recipe_digest: String,
    pub(super) descriptor: OciDescriptor,
    pub(super) platforms: Vec<BuildPlatform>,
    pub(super) content_bytes: u64,
    pub(super) blob_count: usize,
}

impl BuildReceipt {
    pub(super) fn built_artifact(&self, oci_layout_directory: PathBuf) -> BuiltOciArtifact {
        BuiltOciArtifact {
            build_id: self.build_id,
            source_content_digest: self.source_content_digest.clone(),
            recipe_digest: self.recipe_digest.clone(),
            descriptor: self.descriptor.clone(),
            platforms: self.platforms.clone(),
            oci_layout_directory,
            content_bytes: self.content_bytes,
            blob_count: self.blob_count,
        }
    }

    pub(super) fn matches(&self, request: &OciBuildRequest, recipe_digest: &str) -> bool {
        self.build_id == request.build_id()
            && self.source_content_digest == request.source_content_digest()
            && self.recipe_digest == recipe_digest
    }
}

pub(super) async fn write_receipt(
    output: &Path,
    receipt: &BuildReceipt,
) -> Result<(), BuildServiceError> {
    let encoded = serde_json::to_vec(receipt)
        .map_err(|_| integrity("OCI build receipt could not be encoded"))?;
    tokio::fs::write(output.join("receipt.json"), encoded)
        .await
        .map_err(|_| storage("could not write OCI build receipt"))
}

pub(super) async fn read_receipt(output: &Path) -> Result<BuildReceipt, BuildServiceError> {
    let path = output.join("receipt.json");
    let metadata = tokio::fs::symlink_metadata(&path)
        .await
        .map_err(|_| integrity("OCI build receipt is unavailable"))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_RECEIPT_BYTES
    {
        return Err(integrity("OCI build receipt is invalid"));
    }
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|_| storage("could not read OCI build receipt"))?;
    let mut encoded = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_RECEIPT_BYTES + 1)
        .read_to_end(&mut encoded)
        .await
        .map_err(|_| storage("could not read OCI build receipt"))?;
    if encoded.len() as u64 > MAX_RECEIPT_BYTES {
        return Err(integrity("OCI build receipt is invalid"));
    }
    serde_json::from_slice(&encoded).map_err(|_| integrity("OCI build receipt is invalid"))
}

fn integrity(message: impl Into<String>) -> BuildServiceError {
    BuildServiceError::Integrity(message.into())
}

fn storage(message: impl Into<String>) -> BuildServiceError {
    BuildServiceError::Storage(message.into())
}
