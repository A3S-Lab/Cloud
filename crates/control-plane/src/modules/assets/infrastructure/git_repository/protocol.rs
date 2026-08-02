use super::{git_directory, git_storage, integrity, storage, LocalAssetGitRepository};
use crate::modules::assets::domain::{
    validate_asset_repository_mutation, Asset, AssetGitRepositoryError, AssetGitRpcLimits,
    AssetGitRpcResponse, AssetGitService, AssetGitWriteLease, AssetGitWriteOperation,
    IAssetGitRepository,
};
use crate::modules::shared_kernel::domain::Sha256Digest;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const MAX_REPOSITORY_ENTRIES: usize = 1_000_000;

pub(super) async fn advertise(
    store: &LocalAssetGitRepository,
    asset: &Asset,
    service: AssetGitService,
) -> Result<Vec<u8>, AssetGitRepositoryError> {
    store.inspect(asset).await?;
    let repository = store.repository_path(asset);
    let advertised = store
        .git(vec![
            service.git_subcommand().into(),
            "--stateless-rpc".into(),
            "--advertise-refs".into(),
            repository.as_os_str().to_owned(),
        ])
        .await
        .map_err(git_storage("advertise hosted Git references"))?;
    let service_line = format!("# service={}\n", service.as_str());
    let packet_length = service_line
        .len()
        .checked_add(4)
        .filter(|length| *length <= u16::MAX as usize)
        .ok_or_else(|| integrity("Git service advertisement header overflowed"))?;
    let mut response = format!("{packet_length:04x}{service_line}0000").into_bytes();
    response.extend_from_slice(&advertised);
    Ok(response)
}

pub(super) async fn execute_rpc(
    store: &LocalAssetGitRepository,
    asset: &Asset,
    service: AssetGitService,
    request: Vec<u8>,
    limits: AssetGitRpcLimits,
    write_lease: Option<&AssetGitWriteLease>,
) -> Result<AssetGitRpcResponse, AssetGitRepositoryError> {
    store.inspect(asset).await?;
    let limits = limits
        .validate()
        .map_err(AssetGitRepositoryError::Invalid)?;
    match (service, write_lease) {
        (AssetGitService::UploadPack, None) => {}
        (AssetGitService::ReceivePack, Some(lease))
            if lease.operation == AssetGitWriteOperation::ReceivePack && !lease.recovery =>
        {
            validate_asset_repository_mutation(asset).map_err(AssetGitRepositoryError::Invalid)?;
            super::journal::require_prepared(store, asset, lease).await?;
        }
        _ => {
            return Err(AssetGitRepositoryError::Invalid(
                "Git RPC service does not match its write lease".into(),
            ))
        }
    }
    if request.len() as u64 > limits.maximum_input_bytes {
        return Err(AssetGitRepositoryError::QuotaExceeded);
    }
    let repository = store.repository_path(asset);
    let mut args = Vec::<OsString>::new();
    if service == AssetGitService::ReceivePack {
        args.extend([
            "-c".into(),
            format!("receive.maxInputSize={}", limits.maximum_input_bytes).into(),
        ]);
    }
    args.extend([
        service.git_subcommand().into(),
        "--stateless-rpc".into(),
        repository.as_os_str().to_owned(),
    ]);
    let body = match run_with_input(store, args, request).await {
        Ok(body) => body,
        Err(error) => return Err(git_storage("serve hosted Git RPC")(error)),
    };
    let response = AssetGitRpcResponse {
        body,
        repository_bytes: repository_bytes(store, asset).await?,
        refs_digest: refs_digest(store, asset).await?,
    };
    if service == AssetGitService::ReceivePack
        && response.repository_bytes > limits.maximum_repository_bytes
    {
        return Err(AssetGitRepositoryError::QuotaExceeded);
    }
    Ok(response)
}

pub(super) async fn repository_bytes(
    store: &LocalAssetGitRepository,
    asset: &Asset,
) -> Result<u64, AssetGitRepositoryError> {
    store.inspect(asset).await?;
    let path = store.repository_path(asset);
    tokio::task::spawn_blocking(move || measure_tree(&path))
        .await
        .map_err(|error| storage(format!("repository quota measurement failed: {error}")))?
}

pub(super) async fn refs_digest(
    store: &LocalAssetGitRepository,
    asset: &Asset,
) -> Result<Sha256Digest, AssetGitRepositoryError> {
    let refs = list_refs(store, asset).await?;
    digest_refs(&refs)
}

pub(super) async fn list_refs(
    store: &LocalAssetGitRepository,
    asset: &Asset,
) -> Result<BTreeMap<String, String>, AssetGitRepositoryError> {
    store.inspect(asset).await?;
    let output = store
        .git(vec![
            git_directory(&store.repository_path(asset)),
            "for-each-ref".into(),
            "--sort=refname".into(),
            "--format=%(refname) %(objectname)".into(),
        ])
        .await
        .map_err(git_storage("list hosted Git references"))?;
    parse_refs(&output)
}

pub(super) fn digest_refs(
    refs: &BTreeMap<String, String>,
) -> Result<Sha256Digest, AssetGitRepositoryError> {
    let mut digest = Sha256::new();
    for (reference, object_id) in refs {
        digest.update(reference.as_bytes());
        digest.update([0]);
        digest.update(object_id.as_bytes());
        digest.update(b"\n");
    }
    Sha256Digest::parse(format!("sha256:{:x}", digest.finalize()))
        .map_err(AssetGitRepositoryError::Integrity)
}

pub(super) async fn replace_refs(
    store: &LocalAssetGitRepository,
    asset: &Asset,
    target: &BTreeMap<String, String>,
) -> Result<(), AssetGitRepositoryError> {
    let current = list_refs(store, asset).await?;
    let transaction = update_ref_transaction(&current, target);
    if transaction.is_empty() {
        return Ok(());
    }
    run_with_input(
        store,
        vec![
            git_directory(&store.repository_path(asset)),
            "update-ref".into(),
            "--stdin".into(),
        ],
        transaction,
    )
    .await
    .map_err(super::git_integrity(
        "atomically replace hosted Git references",
    ))?;
    Ok(())
}

pub(super) fn update_ref_transaction(
    current: &BTreeMap<String, String>,
    target: &BTreeMap<String, String>,
) -> Vec<u8> {
    let mut commands = String::new();
    for (reference, object_id) in target {
        match current.get(reference) {
            Some(existing) if existing == object_id => {}
            Some(existing) => {
                commands.push_str(&format!("update {reference} {object_id} {existing}\n"));
            }
            None => commands.push_str(&format!("create {reference} {object_id}\n")),
        }
    }
    for (reference, object_id) in current {
        if !target.contains_key(reference) {
            commands.push_str(&format!("delete {reference} {object_id}\n"));
        }
    }
    if commands.is_empty() {
        Vec::new()
    } else {
        format!("start\n{commands}prepare\ncommit\n").into_bytes()
    }
}

pub(super) async fn run_with_input(
    store: &LocalAssetGitRepository,
    args: Vec<OsString>,
    input: Vec<u8>,
) -> Result<Vec<u8>, crate::infrastructure::GitCommandError> {
    store
        .commands
        .run_with_input(&store.root, &store.git_home, &store.hooks, &args, input)
        .await
}

fn parse_refs(output: &[u8]) -> Result<BTreeMap<String, String>, AssetGitRepositoryError> {
    let text =
        std::str::from_utf8(output).map_err(|_| integrity("Git reference list is not UTF-8"))?;
    let mut refs = BTreeMap::new();
    for line in text.lines() {
        let (reference, object_id) = line
            .split_once(' ')
            .ok_or_else(|| integrity("Git reference list is malformed"))?;
        validate_ref(reference, object_id)?;
        if refs
            .insert(reference.to_owned(), object_id.to_owned())
            .is_some()
        {
            return Err(integrity("Git reference list contains duplicates"));
        }
    }
    Ok(refs)
}

pub(super) fn validate_ref(
    reference: &str,
    object_id: &str,
) -> Result<(), AssetGitRepositoryError> {
    let valid_object = matches!(object_id.len(), 40 | 64)
        && object_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    let valid_reference = reference.starts_with("refs/")
        && reference.len() <= 1024
        && !reference.contains("..")
        && !reference.contains("@{")
        && !reference.ends_with('.')
        && !reference.ends_with('/')
        && !reference.ends_with(".lock")
        && reference.split('/').all(|component| {
            !component.is_empty()
                && component != "."
                && !component.starts_with('.')
                && component.bytes().all(|byte| {
                    byte > 0x20
                        && byte != 0x7f
                        && !matches!(byte, b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\')
                })
        });
    if !valid_object || !valid_reference {
        return Err(integrity("Git reference identity is invalid"));
    }
    Ok(())
}

fn measure_tree(root: &Path) -> Result<u64, AssetGitRepositoryError> {
    let mut pending = vec![PathBuf::from(root)];
    let mut entries = 0_usize;
    let mut bytes = 0_u64;
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)
            .map_err(|error| storage(format!("could not measure repository quota: {error}")))?
        {
            let entry = entry
                .map_err(|error| storage(format!("could not measure repository entry: {error}")))?;
            entries = entries
                .checked_add(1)
                .filter(|count| *count <= MAX_REPOSITORY_ENTRIES)
                .ok_or_else(|| integrity("Git repository has too many storage entries"))?;
            let metadata = std::fs::symlink_metadata(entry.path()).map_err(|error| {
                storage(format!("could not inspect repository quota entry: {error}"))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(integrity("Git repository contains a symlink"));
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                bytes = bytes
                    .checked_add(metadata.len())
                    .ok_or_else(|| integrity("Git repository size overflowed"))?;
            } else {
                return Err(integrity("Git repository contains a special file"));
            }
        }
    }
    Ok(bytes)
}
