use super::protocol::{digest_refs, list_refs, replace_refs, validate_ref};
use super::{
    git_directory, git_integrity, git_storage, integrity, storage, LocalAssetGitRepository,
};
use crate::infrastructure::ImmutableObjectOpenResult;
use crate::modules::assets::domain::{
    Asset, AssetGitBackup, AssetGitRepositoryError, AssetGitRpcResponse, AssetGitWriteLease,
    AssetGitWriteOperation, IAssetGitRepository,
};
use crate::modules::shared_kernel::domain::Sha256Digest;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

const EMPTY_BACKUP: &[u8] = b"a3s.cloud.empty-git-bundle.v1\n";
const STREAM_BUFFER_BYTES: usize = 64 * 1024;

pub(super) async fn create(
    store: &LocalAssetGitRepository,
    asset: &Asset,
    lease: &AssetGitWriteLease,
    created_at: DateTime<Utc>,
) -> Result<AssetGitBackup, AssetGitRepositoryError> {
    if lease.operation != AssetGitWriteOperation::Backup || lease.recovery {
        return Err(AssetGitRepositoryError::Invalid(
            "Git backup does not match its write lease".into(),
        ));
    }
    super::journal::require_prepared(store, asset, lease).await?;
    store.inspect(asset).await?;
    let objects = store
        .backup_objects
        .as_ref()
        .ok_or(AssetGitRepositoryError::BackupUnavailable)?;
    let refs = list_refs(store, asset).await?;
    let refs_digest = digest_refs(&refs)?;
    let staging = store
        .staging_root
        .join(format!("{}.gitbundle", Uuid::now_v7()));
    let prepared = if refs.is_empty() {
        tokio::fs::write(&staging, EMPTY_BACKUP)
            .await
            .map_err(|error| storage(format!("could not create empty Git backup: {error}")))
    } else {
        store
            .git(vec![
                git_directory(&store.repository_path(asset)),
                "bundle".into(),
                "create".into(),
                staging.as_os_str().to_owned(),
                "--all".into(),
            ])
            .await
            .map(|_| ())
            .map_err(git_storage("create hosted Git backup bundle"))
    };
    if let Err(error) = prepared {
        remove_file(&staging).await;
        return Err(error);
    }
    secure_file(&staging).await?;
    let (size_bytes, digest) = match file_identity(&staging, store.backup_max_bytes).await {
        Ok(identity) => identity,
        Err(error) => {
            remove_file(&staging).await;
            return Err(error);
        }
    };
    let hexadecimal = digest
        .as_str()
        .strip_prefix("sha256:")
        .ok_or_else(|| integrity("Git backup digest lost its algorithm"))?;
    let object_key = format!(
        "repositories/{}/{}/{hexadecimal}.bundle",
        asset.organization_id, asset.id
    );
    let file = tokio::fs::File::open(&staging)
        .await
        .map_err(|error| storage(format!("could not open Git backup bundle: {error}")))?;
    let publication = objects
        .put_stream(
            &object_key,
            Box::pin(file),
            size_bytes,
            digest.as_str(),
            store.backup_max_bytes,
        )
        .await
        .map_err(|error| storage(format!("could not publish Git backup: {error}")));
    remove_file(&staging).await;
    publication?;
    let backup = AssetGitBackup {
        object_key,
        digest,
        size_bytes,
        refs_digest,
        created_at,
    };
    backup
        .validate()
        .map_err(AssetGitRepositoryError::Integrity)?;
    Ok(backup)
}

pub(super) async fn restore(
    store: &LocalAssetGitRepository,
    asset: &Asset,
    lease: &AssetGitWriteLease,
    backup: &AssetGitBackup,
    maximum_repository_bytes: u64,
) -> Result<AssetGitRpcResponse, AssetGitRepositoryError> {
    if lease.operation != AssetGitWriteOperation::Restore || lease.recovery {
        return Err(AssetGitRepositoryError::Invalid(
            "Git restore does not match its write lease".into(),
        ));
    }
    super::journal::require_prepared(store, asset, lease).await?;
    backup
        .validate()
        .map_err(AssetGitRepositoryError::Invalid)?;
    if maximum_repository_bytes == 0 {
        return Err(AssetGitRepositoryError::Invalid(
            "restore repository maximum must be positive".into(),
        ));
    }
    store.inspect(asset).await?;
    let objects = store
        .backup_objects
        .as_ref()
        .ok_or(AssetGitRepositoryError::BackupUnavailable)?;
    let opened = match objects
        .open(&backup.object_key, store.backup_max_bytes)
        .await
        .map_err(|error| storage(format!("could not open Git backup: {error}")))?
    {
        ImmutableObjectOpenResult::Found(opened) => opened,
        ImmutableObjectOpenResult::Missing => return Err(AssetGitRepositoryError::NotFound),
        ImmutableObjectOpenResult::Corrupt => {
            return Err(integrity("Git backup object exceeds its bound"))
        }
    };
    if opened.size_bytes != backup.size_bytes {
        return Err(integrity("Git backup size does not match its receipt"));
    }
    let staging = store
        .staging_root
        .join(format!("{}.restore-bundle", Uuid::now_v7()));
    if let Err(error) = write_verified_backup(
        &staging,
        opened.reader,
        backup.size_bytes,
        &backup.digest,
        store.backup_max_bytes,
    )
    .await
    {
        remove_file(&staging).await;
        return Err(error);
    }
    let target_refs = match target_refs(store, &staging).await {
        Ok(refs) => refs,
        Err(error) => {
            remove_file(&staging).await;
            return Err(error);
        }
    };
    if digest_refs(&target_refs)? != backup.refs_digest {
        remove_file(&staging).await;
        return Err(integrity("Git backup references do not match its receipt"));
    }
    let restored = async {
        if !target_refs.is_empty() {
            store
                .git(vec![
                    git_directory(&store.repository_path(asset)),
                    "bundle".into(),
                    "unbundle".into(),
                    staging.as_os_str().to_owned(),
                ])
                .await
                .map_err(git_integrity("unbundle hosted Git backup"))?;
        }
        replace_refs(store, asset, &target_refs).await?;
        let refs_digest = store.refs_digest(asset).await?;
        if refs_digest != backup.refs_digest {
            return Err(integrity("restored Git references changed after commit"));
        }
        let repository_bytes = store.repository_bytes(asset).await?;
        if repository_bytes > maximum_repository_bytes {
            return Err(AssetGitRepositoryError::QuotaExceeded);
        }
        Ok(AssetGitRpcResponse {
            body: Vec::new(),
            repository_bytes,
            refs_digest,
        })
    }
    .await;
    remove_file(&staging).await;
    match restored {
        Ok(response) => Ok(response),
        Err(error) => Err(error),
    }
}

async fn target_refs(
    store: &LocalAssetGitRepository,
    path: &Path,
) -> Result<BTreeMap<String, String>, AssetGitRepositoryError> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|error| storage(format!("could not inspect Git backup: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(integrity("Git backup staging path is not an owned file"));
    }
    if metadata.len() == EMPTY_BACKUP.len() as u64 {
        let mut file = tokio::fs::File::open(path)
            .await
            .map_err(|error| storage(format!("could not open Git backup marker: {error}")))?;
        let mut marker = [0_u8; EMPTY_BACKUP.len()];
        file.read_exact(&mut marker)
            .await
            .map_err(|error| storage(format!("could not read Git backup marker: {error}")))?;
        if marker == EMPTY_BACKUP {
            return Ok(BTreeMap::new());
        }
    }
    let output = store
        .git(vec![
            "bundle".into(),
            "list-heads".into(),
            path.as_os_str().to_owned(),
        ])
        .await
        .map_err(git_integrity("inspect hosted Git backup references"))?;
    parse_bundle_heads(&output)
}

fn parse_bundle_heads(output: &[u8]) -> Result<BTreeMap<String, String>, AssetGitRepositoryError> {
    let text = std::str::from_utf8(output)
        .map_err(|_| integrity("Git backup reference list is not UTF-8"))?;
    let mut refs = BTreeMap::new();
    for line in text.lines() {
        let (object_id, reference) = line
            .split_once(' ')
            .ok_or_else(|| integrity("Git backup reference list is malformed"))?;
        if reference == "HEAD" {
            continue;
        }
        validate_ref(reference, object_id)?;
        if refs
            .insert(reference.to_owned(), object_id.to_owned())
            .is_some()
        {
            return Err(integrity("Git backup contains duplicate references"));
        }
    }
    Ok(refs)
}

async fn file_identity(
    path: &Path,
    maximum_bytes: u64,
) -> Result<(u64, Sha256Digest), AssetGitRepositoryError> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| storage(format!("could not read Git backup: {error}")))?;
    let mut digest = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = vec![0_u8; STREAM_BUFFER_BYTES];
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|error| storage(format!("could not hash Git backup: {error}")))?;
        if read == 0 {
            break;
        }
        size = size
            .checked_add(read as u64)
            .filter(|size| *size <= maximum_bytes)
            .ok_or(AssetGitRepositoryError::QuotaExceeded)?;
        digest.update(&buffer[..read]);
    }
    if size == 0 {
        return Err(integrity("Git backup is empty"));
    }
    let digest = Sha256Digest::parse(format!("sha256:{:x}", digest.finalize()))
        .map_err(AssetGitRepositoryError::Integrity)?;
    Ok((size, digest))
}

async fn write_verified_backup(
    path: &Path,
    mut reader: crate::infrastructure::ImmutableObjectReader,
    expected_size: u64,
    expected_digest: &Sha256Digest,
    maximum_bytes: u64,
) -> Result<(), AssetGitRepositoryError> {
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .await
        .map_err(|error| storage(format!("could not stage Git backup restore: {error}")))?;
    let mut digest = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = vec![0_u8; STREAM_BUFFER_BYTES];
    loop {
        let read = reader
            .read(&mut buffer)
            .await
            .map_err(|error| storage(format!("could not read Git backup object: {error}")))?;
        if read == 0 {
            break;
        }
        size = size
            .checked_add(read as u64)
            .filter(|size| *size <= expected_size && *size <= maximum_bytes)
            .ok_or_else(|| integrity("Git backup object exceeds its receipt"))?;
        digest.update(&buffer[..read]);
        file.write_all(&buffer[..read])
            .await
            .map_err(|error| storage(format!("could not stage Git backup object: {error}")))?;
    }
    file.sync_all()
        .await
        .map_err(|error| storage(format!("could not sync Git backup restore: {error}")))?;
    let observed = Sha256Digest::parse(format!("sha256:{:x}", digest.finalize()))
        .map_err(AssetGitRepositoryError::Integrity)?;
    if size != expected_size || observed != *expected_digest {
        return Err(integrity("Git backup object does not match its receipt"));
    }
    Ok(())
}

async fn secure_file(path: &Path) -> Result<(), AssetGitRepositoryError> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|error| storage(format!("could not inspect Git backup: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(integrity("Git backup staging path is not an owned file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .await
            .map_err(|error| storage(format!("could not secure Git backup: {error}")))?;
    }
    Ok(())
}

async fn remove_file(path: &Path) {
    let _ = tokio::fs::remove_file(path).await;
}
