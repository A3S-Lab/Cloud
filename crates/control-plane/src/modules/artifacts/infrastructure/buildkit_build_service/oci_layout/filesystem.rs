use super::{integrity, storage, valid_hex_digest};
use crate::modules::artifacts::domain::BuildServiceError;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tokio::io::AsyncReadExt;

pub(super) async fn read_regular_file(
    path: &Path,
    maximum: u64,
) -> Result<Vec<u8>, BuildServiceError> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|_| integrity("OCI layout file is unavailable"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > maximum {
        return Err(integrity("OCI layout file is invalid"));
    }
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|_| storage("could not read OCI layout file"))?;
    let mut content = Vec::with_capacity(metadata.len() as usize);
    file.take(maximum + 1)
        .read_to_end(&mut content)
        .await
        .map_err(|_| storage("could not read OCI layout file"))?;
    if content.len() as u64 > maximum {
        return Err(integrity("OCI layout file exceeds its bound"));
    }
    Ok(content)
}

pub(super) async fn validate_root_entries(layout: &Path) -> Result<(), BuildServiceError> {
    let mut entries = tokio::fs::read_dir(layout)
        .await
        .map_err(|_| storage("could not inspect OCI layout directory"))?;
    let mut names = HashSet::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|_| storage("could not inspect OCI layout directory"))?
    {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| integrity("OCI layout contains a non-UTF-8 entry"))?;
        if !matches!(name.as_str(), "oci-layout" | "index.json" | "blobs") {
            return Err(integrity("OCI layout contains an unexpected entry"));
        }
        names.insert(name);
    }
    if names != HashSet::from(["oci-layout".into(), "index.json".into(), "blobs".into()]) {
        return Err(integrity("OCI layout is incomplete"));
    }
    require_owned_directory(&layout.join("blobs"), "OCI blobs directory").await?;
    let mut algorithms = tokio::fs::read_dir(layout.join("blobs"))
        .await
        .map_err(|_| storage("could not inspect OCI blobs directory"))?;
    let algorithm = algorithms
        .next_entry()
        .await
        .map_err(|_| storage("could not inspect OCI blobs directory"))?
        .ok_or_else(|| integrity("OCI blobs directory is empty"))?;
    if algorithm.file_name() != "sha256"
        || algorithms
            .next_entry()
            .await
            .map_err(|_| storage("could not inspect OCI blobs directory"))?
            .is_some()
    {
        return Err(integrity(
            "OCI blobs directory contains an unsupported digest algorithm",
        ));
    }
    require_owned_directory(&algorithm.path(), "OCI SHA-256 blobs directory").await
}

pub(super) async fn remove_empty_ingest_directory(layout: &Path) -> Result<(), BuildServiceError> {
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

pub(super) async fn validate_blob_inventory(
    layout: &Path,
    seen: &HashMap<String, (String, u64)>,
) -> Result<(), BuildServiceError> {
    let mut entries = tokio::fs::read_dir(layout.join("blobs/sha256"))
        .await
        .map_err(|_| storage("could not inspect OCI blob inventory"))?;
    let mut count = 0_usize;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|_| storage("could not inspect OCI blob inventory"))?
    {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| integrity("OCI blob name is not UTF-8"))?;
        let file_type = entry
            .file_type()
            .await
            .map_err(|_| storage("could not inspect OCI blob inventory"))?;
        if !file_type.is_file()
            || !valid_hex_digest(&name)
            || !seen.contains_key(&format!("sha256:{name}"))
        {
            return Err(integrity(
                "OCI blob inventory contains an unreferenced or invalid blob",
            ));
        }
        count += 1;
    }
    if count != seen.len() {
        return Err(integrity("OCI blob inventory is incomplete"));
    }
    Ok(())
}

pub(super) async fn require_owned_directory(
    path: &Path,
    label: &str,
) -> Result<(), BuildServiceError> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|_| integrity(format!("{label} is unavailable")))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(integrity(format!("{label} is not an owned directory")));
    }
    Ok(())
}
