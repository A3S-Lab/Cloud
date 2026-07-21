use super::cache_io::{
    digest_hex, read_optional_json, read_required_json, storage, validate_local_artifact,
    MountReceipt, OutputReceipt, MOUNT_RECEIPT_SCHEMA, OUTPUT_RECEIPT_SCHEMA,
};
use super::store::NodeArtifactError;
use a3s_cloud_contracts::validate_cloud_artifact;
use std::collections::BTreeSet;
use std::path::Path;

pub(super) async fn garbage_collect_blobs(root: &Path) -> Result<(), NodeArtifactError> {
    let referenced = referenced_blob_digests(root).await?;
    let blob_root = root.join("blobs/sha256");
    let receipt_root = root.join("blob-receipts/sha256");
    let mut candidates = BTreeSet::new();
    collect_digest_file_names(&blob_root, false, &mut candidates).await?;
    collect_digest_file_names(&receipt_root, true, &mut candidates).await?;
    for digest in candidates.difference(&referenced) {
        for path in [
            blob_root.join(digest),
            receipt_root.join(format!("{digest}.json")),
        ] {
            match tokio::fs::remove_file(&path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(storage(error)),
            }
        }
    }
    Ok(())
}

async fn referenced_blob_digests(root: &Path) -> Result<BTreeSet<String>, NodeArtifactError> {
    let mut referenced = BTreeSet::new();
    let mounts = root.join("mounts");
    let mut specifications = tokio::fs::read_dir(&mounts).await.map_err(storage)?;
    while let Some(specification) = specifications.next_entry().await.map_err(storage)? {
        if !specification.file_type().await.map_err(storage)?.is_dir() {
            return Err(NodeArtifactError::Integrity(
                "artifact mount cache contains an unexpected entry".into(),
            ));
        }
        let mut views = tokio::fs::read_dir(specification.path())
            .await
            .map_err(storage)?;
        while let Some(view) = views.next_entry().await.map_err(storage)? {
            if !view.file_type().await.map_err(storage)?.is_dir() {
                return Err(NodeArtifactError::Integrity(
                    "artifact mount cache contains an unexpected view".into(),
                ));
            }
            let receipt =
                read_optional_json::<MountReceipt>(&view.path().join("receipt.json")).await?;
            let Some(receipt) = receipt else {
                continue;
            };
            if receipt.schema != MOUNT_RECEIPT_SCHEMA {
                return Err(NodeArtifactError::Integrity(
                    "artifact mount cache receipt has an unsupported schema".into(),
                ));
            }
            validate_cloud_artifact(&receipt.artifact).map_err(NodeArtifactError::Integrity)?;
            referenced.insert(digest_hex(&receipt.artifact.digest)?.to_owned());
        }
    }

    let outputs = root.join("outputs");
    let mut specifications = tokio::fs::read_dir(&outputs).await.map_err(storage)?;
    while let Some(specification) = specifications.next_entry().await.map_err(storage)? {
        if !specification.file_type().await.map_err(storage)?.is_dir() {
            return Err(NodeArtifactError::Integrity(
                "artifact output cache contains an unexpected entry".into(),
            ));
        }
        let mut receipts = tokio::fs::read_dir(specification.path())
            .await
            .map_err(storage)?;
        while let Some(receipt_entry) = receipts.next_entry().await.map_err(storage)? {
            if !receipt_entry.file_type().await.map_err(storage)?.is_file() {
                return Err(NodeArtifactError::Integrity(
                    "artifact output cache contains an unexpected receipt".into(),
                ));
            }
            let receipt = read_required_json::<OutputReceipt>(&receipt_entry.path()).await?;
            if receipt.schema != OUTPUT_RECEIPT_SCHEMA {
                return Err(NodeArtifactError::Integrity(
                    "artifact output cache receipt has an unsupported schema".into(),
                ));
            }
            validate_local_artifact(&receipt.output.artifact)?;
            referenced.insert(digest_hex(&receipt.output.artifact.digest)?.to_owned());
        }
    }
    Ok(referenced)
}

async fn collect_digest_file_names(
    root: &Path,
    json_suffix: bool,
    output: &mut BTreeSet<String>,
) -> Result<(), NodeArtifactError> {
    let mut entries = tokio::fs::read_dir(root).await.map_err(storage)?;
    while let Some(entry) = entries.next_entry().await.map_err(storage)? {
        if !entry.file_type().await.map_err(storage)?.is_file() {
            return Err(NodeArtifactError::Integrity(
                "artifact blob cache contains an unexpected entry".into(),
            ));
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| NodeArtifactError::Integrity("artifact blob name is not UTF-8".into()))?;
        let digest = if json_suffix {
            name.strip_suffix(".json").ok_or_else(|| {
                NodeArtifactError::Integrity("artifact blob receipt name is invalid".into())
            })?
        } else {
            name.as_str()
        };
        digest_hex(&format!("sha256:{digest}")).map_err(|_| {
            NodeArtifactError::Integrity("artifact blob cache name is invalid".into())
        })?;
        output.insert(digest.to_owned());
    }
    Ok(())
}
