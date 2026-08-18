use super::protocol::{digest_refs, list_refs, replace_refs};
use super::{integrity, storage, LocalAssetGitRepository};
use crate::infrastructure::{
    sync_directories as sync_filesystem_directories, sync_directory as sync_filesystem_directory,
};
use crate::modules::assets::domain::{
    Asset, AssetGitRepositoryError, AssetGitWriteJournal, AssetGitWriteLease, IAssetGitRepository,
};
use crate::modules::shared_kernel::domain::Sha256Digest;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use tokio::io::AsyncWriteExt;

const JOURNAL_SCHEMA: &str = "a3s.cloud.asset-git-write-journal.v1";
const MAX_REPOSITORY_ENTRIES: usize = 1_000_000;
const MAX_JOURNAL_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JournalPayload {
    schema: String,
    organization_id: uuid::Uuid,
    asset_id: uuid::Uuid,
    lease_id: uuid::Uuid,
    operation: String,
    refs: BTreeMap<String, String>,
    refs_digest: String,
    object_files: BTreeSet<String>,
    object_directories: BTreeSet<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JournalEnvelope {
    payload: JournalPayload,
    checksum: String,
}

#[derive(Default)]
struct ObjectSnapshot {
    files: BTreeSet<PathBuf>,
    directories: BTreeSet<PathBuf>,
}

pub(super) async fn prepare(
    store: &LocalAssetGitRepository,
    asset: &Asset,
    lease: &AssetGitWriteLease,
) -> Result<(), AssetGitRepositoryError> {
    lease
        .validate_for(asset)
        .map_err(AssetGitRepositoryError::Invalid)?;
    if lease.recovery {
        return Err(AssetGitRepositoryError::Invalid(
            "recovery lease cannot prepare another hosted Git write".into(),
        ));
    }
    store.inspect(asset).await?;
    let journal = lease.journal();
    if let Some(existing) = load_optional(store, asset, &journal).await? {
        validate_lease_payload(&existing, lease)?;
        return Ok(());
    }

    remove_if_present(&pending_path(store, journal.lease_id)).await?;
    let refs = list_refs(store, asset).await?;
    let refs_digest = digest_refs(&refs)?;
    let objects = store.repository_path(asset).join("objects");
    let snapshot = tokio::task::spawn_blocking(move || snapshot_objects(&objects))
        .await
        .map_err(|error| storage(format!("repository journal snapshot failed: {error}")))??;
    let payload = JournalPayload {
        schema: JOURNAL_SCHEMA.into(),
        organization_id: asset.organization_id.as_uuid(),
        asset_id: asset.id.as_uuid(),
        lease_id: lease.lease_id,
        operation: lease.operation.as_str().into(),
        refs,
        refs_digest: refs_digest.as_str().into(),
        object_files: encode_paths(snapshot.files)?,
        object_directories: encode_paths(snapshot.directories)?,
    };
    validate_payload(asset, &journal, &payload)?;
    let checksum = payload_checksum(&payload)?;
    let bytes = serde_json::to_vec(&JournalEnvelope { payload, checksum }).map_err(|error| {
        storage(format!(
            "could not encode hosted Git write journal: {error}"
        ))
    })?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(AssetGitRepositoryError::QuotaExceeded);
    }

    let pending = pending_path(store, lease.lease_id);
    let published = journal_path(store, lease.lease_id);
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    let mut file = options.open(&pending).await.map_err(|error| {
        storage(format!(
            "could not create hosted Git write journal: {error}"
        ))
    })?;
    let publication: Result<(), AssetGitRepositoryError> = async {
        file.write_all(&bytes)
            .await
            .map_err(|error| storage(format!("could not write hosted Git journal: {error}")))?;
        file.sync_all()
            .await
            .map_err(|error| storage(format!("could not sync hosted Git journal: {error}")))?;
        tokio::fs::rename(&pending, &published)
            .await
            .map_err(|error| storage(format!("could not rename hosted Git journal: {error}")))?;
        sync_directory(&store.staging_root).await
    }
    .await;
    if let Err(error) = publication {
        let _ = tokio::fs::remove_file(&pending).await;
        return Err(storage(format!(
            "could not publish hosted Git write journal: {error}"
        )));
    }
    Ok(())
}

pub(super) async fn rollback(
    store: &LocalAssetGitRepository,
    asset: &Asset,
    lease: &AssetGitWriteLease,
) -> Result<(), AssetGitRepositoryError> {
    lease
        .validate_for(asset)
        .map_err(AssetGitRepositoryError::Invalid)?;
    let journal = lease.journal();
    let Some(payload) = load_optional(store, asset, &journal).await? else {
        remove_if_present(&pending_path(store, lease.lease_id)).await?;
        sync_directory(&store.staging_root).await?;
        return Ok(());
    };
    validate_lease_payload(&payload, lease)?;
    replace_refs(store, asset, &payload.refs).await?;
    let objects = store.repository_path(asset).join("objects");
    let previous = ObjectSnapshot {
        files: decode_paths(&payload.object_files)?,
        directories: decode_paths(&payload.object_directories)?,
    };
    tokio::task::spawn_blocking(move || restore_object_snapshot(&objects, &previous))
        .await
        .map_err(|error| storage(format!("repository journal rollback failed: {error}")))??;
    if store.refs_digest(asset).await?.as_str() != payload.refs_digest {
        return Err(integrity(
            "hosted Git write-journal rollback changed reference identity",
        ));
    }
    remove_journal(store, lease.lease_id).await
}

pub(super) async fn require_prepared(
    store: &LocalAssetGitRepository,
    asset: &Asset,
    lease: &AssetGitWriteLease,
) -> Result<(), AssetGitRepositoryError> {
    lease
        .validate_for(asset)
        .map_err(AssetGitRepositoryError::Invalid)?;
    if lease.recovery {
        return Err(AssetGitRepositoryError::Invalid(
            "recovery lease cannot execute another hosted Git write".into(),
        ));
    }
    let payload = load_optional(store, asset, &lease.journal())
        .await?
        .ok_or_else(|| integrity("hosted Git write journal was not prepared"))?;
    validate_lease_payload(&payload, lease)
}

pub(super) async fn settle(
    store: &LocalAssetGitRepository,
    asset: &Asset,
    journal: &AssetGitWriteJournal,
) -> Result<(), AssetGitRepositoryError> {
    journal
        .validate_for(asset)
        .map_err(AssetGitRepositoryError::Invalid)?;
    if load_optional(store, asset, journal).await?.is_some() {
        remove_journal(store, journal.lease_id).await?;
    } else {
        remove_if_present(&pending_path(store, journal.lease_id)).await?;
        sync_directory(&store.staging_root).await?;
    }
    Ok(())
}

async fn load_optional(
    store: &LocalAssetGitRepository,
    asset: &Asset,
    journal: &AssetGitWriteJournal,
) -> Result<Option<JournalPayload>, AssetGitRepositoryError> {
    journal
        .validate_for(asset)
        .map_err(AssetGitRepositoryError::Invalid)?;
    let path = journal_path(store, journal.lease_id);
    let metadata = match tokio::fs::symlink_metadata(&path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(storage(format!(
                "could not inspect hosted Git write journal: {error}"
            )))
        }
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_JOURNAL_BYTES
    {
        return Err(integrity(
            "hosted Git write journal is not an owned bounded file",
        ));
    }
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|error| storage(format!("could not read hosted Git write journal: {error}")))?;
    let envelope: JournalEnvelope = serde_json::from_slice(&bytes)
        .map_err(|_| integrity("hosted Git write journal is malformed"))?;
    validate_payload(asset, journal, &envelope.payload)?;
    if payload_checksum(&envelope.payload)? != envelope.checksum {
        return Err(integrity("hosted Git write journal checksum changed"));
    }
    Ok(Some(envelope.payload))
}

fn validate_payload(
    asset: &Asset,
    journal: &AssetGitWriteJournal,
    payload: &JournalPayload,
) -> Result<(), AssetGitRepositoryError> {
    if payload.schema != JOURNAL_SCHEMA
        || payload.organization_id != asset.organization_id.as_uuid()
        || payload.asset_id != asset.id.as_uuid()
        || payload.lease_id != journal.lease_id
        || !matches!(
            payload.operation.as_str(),
            "receive_pack" | "backup" | "restore"
        )
        || payload.refs.len() > MAX_REPOSITORY_ENTRIES
        || payload
            .object_files
            .len()
            .saturating_add(payload.object_directories.len())
            > MAX_REPOSITORY_ENTRIES
    {
        return Err(integrity("hosted Git write journal identity is invalid"));
    }
    let expected = digest_refs(&payload.refs)?;
    let stored =
        Sha256Digest::parse(&payload.refs_digest).map_err(AssetGitRepositoryError::Integrity)?;
    if expected != stored {
        return Err(integrity(
            "hosted Git write journal reference digest changed",
        ));
    }
    decode_paths(&payload.object_files)?;
    decode_paths(&payload.object_directories)?;
    Ok(())
}

fn validate_lease_payload(
    payload: &JournalPayload,
    lease: &AssetGitWriteLease,
) -> Result<(), AssetGitRepositoryError> {
    if payload.operation != lease.operation.as_str() {
        return Err(integrity("hosted Git write journal operation changed"));
    }
    Ok(())
}

fn payload_checksum(payload: &JournalPayload) -> Result<String, AssetGitRepositoryError> {
    let bytes = serde_json::to_vec(payload)
        .map_err(|error| storage(format!("could not hash hosted Git write journal: {error}")))?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn encode_paths(paths: BTreeSet<PathBuf>) -> Result<BTreeSet<String>, AssetGitRepositoryError> {
    paths
        .into_iter()
        .map(|path| {
            validate_relative_path(&path)?;
            path.to_str()
                .map(str::to_owned)
                .ok_or_else(|| integrity("Git object path is not UTF-8"))
        })
        .collect()
}

fn decode_paths(paths: &BTreeSet<String>) -> Result<BTreeSet<PathBuf>, AssetGitRepositoryError> {
    paths
        .iter()
        .map(|value| {
            let path = PathBuf::from(value);
            validate_relative_path(&path)?;
            Ok(path)
        })
        .collect()
}

fn validate_relative_path(path: &Path) -> Result<(), AssetGitRepositoryError> {
    if path.as_os_str().is_empty()
        || path.to_string_lossy().len() > 4096
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(integrity("Git object path escaped its repository"));
    }
    Ok(())
}

fn snapshot_objects(root: &Path) -> Result<ObjectSnapshot, AssetGitRepositoryError> {
    let mut snapshot = ObjectSnapshot::default();
    visit_tree(root, |path, metadata| {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| integrity("Git object path escaped its repository"))?
            .to_owned();
        validate_relative_path(&relative)?;
        if metadata.is_dir() {
            snapshot.directories.insert(relative);
        } else {
            snapshot.files.insert(relative);
        }
        Ok(())
    })?;
    Ok(snapshot)
}

fn restore_object_snapshot(
    root: &Path,
    previous: &ObjectSnapshot,
) -> Result<(), AssetGitRepositoryError> {
    let current = snapshot_objects(root)?;
    for relative in current.files.difference(&previous.files) {
        let path = root.join(relative);
        std::fs::remove_file(&path).map_err(|error| {
            storage(format!(
                "could not remove rejected Git object {}: {error}",
                path.display()
            ))
        })?;
    }
    let mut new_directories = current
        .directories
        .difference(&previous.directories)
        .cloned()
        .collect::<Vec<_>>();
    new_directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for relative in new_directories {
        let path = root.join(relative);
        std::fs::remove_dir(&path).map_err(|error| {
            storage(format!(
                "could not remove rejected Git object directory {}: {error}",
                path.display()
            ))
        })?;
    }
    sync_filesystem_directory(root)
        .map_err(|error| storage(format!("could not sync Git object rollback: {error}")))
}

fn visit_tree(
    root: &Path,
    mut visit: impl FnMut(&Path, &std::fs::Metadata) -> Result<(), AssetGitRepositoryError>,
) -> Result<(), AssetGitRepositoryError> {
    let mut pending = vec![root.to_owned()];
    let mut entries = 0_usize;
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)
            .map_err(|error| storage(format!("could not inspect Git object tree: {error}")))?
        {
            let entry = entry
                .map_err(|error| storage(format!("could not inspect Git object entry: {error}")))?;
            entries = entries
                .checked_add(1)
                .filter(|count| *count <= MAX_REPOSITORY_ENTRIES)
                .ok_or_else(|| integrity("Git repository has too many storage entries"))?;
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
                storage(format!("could not inspect Git object metadata: {error}"))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(integrity("Git repository contains a symlink"));
            }
            if metadata.is_dir() {
                pending.push(path.clone());
            } else if !metadata.is_file() {
                return Err(integrity("Git repository contains a special file"));
            }
            visit(&path, &metadata)?;
        }
    }
    Ok(())
}

async fn remove_journal(
    store: &LocalAssetGitRepository,
    lease_id: uuid::Uuid,
) -> Result<(), AssetGitRepositoryError> {
    remove_if_present(&journal_path(store, lease_id)).await?;
    remove_if_present(&pending_path(store, lease_id)).await?;
    sync_directory(&store.staging_root).await
}

async fn remove_if_present(path: &Path) -> Result<(), AssetGitRepositoryError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage(format!(
            "could not remove hosted Git write journal: {error}"
        ))),
    }
}

async fn sync_directory(path: &Path) -> Result<(), AssetGitRepositoryError> {
    sync_filesystem_directories(vec![path.to_owned()])
        .await
        .map_err(|error| storage(format!("could not sync journal directory: {error}")))
}

fn journal_path(store: &LocalAssetGitRepository, lease_id: uuid::Uuid) -> PathBuf {
    store
        .staging_root
        .join(format!("{lease_id}.asset-git-write.json"))
}

fn pending_path(store: &LocalAssetGitRepository, lease_id: uuid::Uuid) -> PathBuf {
    store
        .staging_root
        .join(format!("{lease_id}.asset-git-write.pending"))
}
