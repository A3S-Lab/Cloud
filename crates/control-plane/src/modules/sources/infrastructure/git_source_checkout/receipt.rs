use crate::modules::sources::domain::{
    CheckedOutSource, SourceCheckoutError, SourceCheckoutRequest,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

pub(super) const RECEIPT_SCHEMA: &str = "a3s.cloud.source-checkout.v1";
const MAX_RECEIPT_BYTES: u64 = 64 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CheckoutReceipt {
    pub(super) schema: String,
    pub(super) checkout_id: Uuid,
    pub(super) repository_identity: String,
    pub(super) repository_url: String,
    pub(super) commit_sha: String,
    pub(super) git_tree_id: String,
    pub(super) content_digest: String,
    pub(super) file_count: usize,
    pub(super) content_bytes: u64,
}

impl CheckoutReceipt {
    pub(super) fn checked_out_source(
        &self,
        directory: PathBuf,
        request: &SourceCheckoutRequest,
    ) -> CheckedOutSource {
        CheckedOutSource {
            checkout_id: self.checkout_id,
            repository: request.repository.clone(),
            commit_sha: request.commit_sha.clone(),
            directory,
            git_tree_id: self.git_tree_id.clone(),
            content_digest: self.content_digest.clone(),
            file_count: self.file_count,
            content_bytes: self.content_bytes,
        }
    }
}

pub(super) async fn write_receipt(
    checkout: &Path,
    receipt: &CheckoutReceipt,
) -> Result<(), SourceCheckoutError> {
    let encoded = serde_json::to_vec(receipt).map_err(|_| {
        SourceCheckoutError::Integrity("source checkout receipt could not be encoded".into())
    })?;
    tokio::fs::write(checkout.join("receipt.json"), encoded)
        .await
        .map_err(|_| SourceCheckoutError::Storage("could not write source checkout receipt".into()))
}

pub(super) async fn read_receipt(checkout: &Path) -> Result<CheckoutReceipt, SourceCheckoutError> {
    let path = checkout.join("receipt.json");
    let metadata = tokio::fs::symlink_metadata(&path).await.map_err(|_| {
        SourceCheckoutError::Integrity("source checkout receipt is unavailable".into())
    })?;
    if !metadata.is_file() || metadata.len() > MAX_RECEIPT_BYTES {
        return Err(SourceCheckoutError::Integrity(
            "source checkout receipt is invalid".into(),
        ));
    }
    let file = tokio::fs::File::open(path).await.map_err(|_| {
        SourceCheckoutError::Storage("could not read source checkout receipt".into())
    })?;
    let mut encoded = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_RECEIPT_BYTES + 1)
        .read_to_end(&mut encoded)
        .await
        .map_err(|_| {
            SourceCheckoutError::Storage("could not read source checkout receipt".into())
        })?;
    if encoded.len() as u64 > MAX_RECEIPT_BYTES {
        return Err(SourceCheckoutError::Integrity(
            "source checkout receipt is invalid".into(),
        ));
    }
    serde_json::from_slice(&encoded)
        .map_err(|_| SourceCheckoutError::Integrity("source checkout receipt is invalid".into()))
}
