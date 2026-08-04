use super::{
    git_directory, git_integrity, integrity, storage, sync_directories, LocalAssetGitRepository,
};
use crate::modules::assets::domain::{
    Asset, AssetGitBuildInput, AssetGitReleaseBundle, AssetGitRepositoryError, IAssetGitRepository,
};
use crate::modules::shared_kernel::domain::{
    AssetReleaseId, BuildRunId, GitCommitSha, Sha256Digest,
};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub(super) async fn prepare(
    store: &LocalAssetGitRepository,
    asset: &Asset,
    commit_sha: &GitCommitSha,
    build_run_id: BuildRunId,
) -> Result<AssetGitBuildInput, AssetGitRepositoryError> {
    if build_run_id.as_uuid().is_nil() {
        return Err(AssetGitRepositoryError::Invalid(
            "Asset build run ID cannot be nil".into(),
        ));
    }
    let archive = prepare_archive(store, asset, commit_sha, &build_run_id.to_string()).await?;
    let input = AssetGitBuildInput {
        build_run_id,
        commit_sha: archive.commit_sha,
        content_digest: archive.digest,
        size_bytes: archive.size_bytes,
        path: archive.path,
    };
    input
        .validate()
        .map_err(AssetGitRepositoryError::Integrity)?;
    Ok(input)
}

pub(super) async fn prepare_release(
    store: &LocalAssetGitRepository,
    asset: &Asset,
    commit_sha: &GitCommitSha,
    asset_release_id: AssetReleaseId,
) -> Result<AssetGitReleaseBundle, AssetGitRepositoryError> {
    if asset_release_id.as_uuid().is_nil() {
        return Err(AssetGitRepositoryError::Invalid(
            "Asset release ID cannot be nil".into(),
        ));
    }
    let archive = prepare_archive(
        store,
        asset,
        commit_sha,
        &format!("release-{asset_release_id}"),
    )
    .await?;
    let bundle = AssetGitReleaseBundle {
        asset_release_id,
        commit_sha: archive.commit_sha,
        digest: archive.digest,
        size_bytes: archive.size_bytes,
        path: archive.path,
    };
    bundle
        .validate()
        .map_err(AssetGitRepositoryError::Integrity)?;
    Ok(bundle)
}

struct PreparedGitArchive {
    commit_sha: GitCommitSha,
    digest: Sha256Digest,
    size_bytes: u64,
    path: PathBuf,
}

async fn prepare_archive(
    store: &LocalAssetGitRepository,
    asset: &Asset,
    commit_sha: &GitCommitSha,
    key: &str,
) -> Result<PreparedGitArchive, AssetGitRepositoryError> {
    store.inspect(asset).await?;
    GitCommitSha::parse(commit_sha.as_str()).map_err(AssetGitRepositoryError::Invalid)?;
    let repository = store.repository_path(asset);
    let reachable = store
        .git(vec![
            git_directory(&repository),
            "for-each-ref".into(),
            format!("--contains={commit_sha}").into(),
            "--format=%(refname)".into(),
            "--count=1".into(),
        ])
        .await
        .map_err(git_integrity("verify Asset build commit reachability"))?;
    if reachable.is_empty() {
        return Err(integrity(
            "Asset build commit is not reachable from an advertised reference",
        ));
    }

    let staging = store
        .build_input_root
        .join(format!(".{key}-{}.tmp", Uuid::now_v7()));
    let target = store.build_input_root.join(format!("{key}.tar"));
    let archive = store
        .git(vec![
            git_directory(&repository),
            "archive".into(),
            "--format=tar".into(),
            format!("--output={}", staging.display()).into(),
            commit_sha.as_str().into(),
        ])
        .await;
    if let Err(error) = archive {
        let _ = tokio::fs::remove_file(&staging).await;
        return Err(git_integrity("archive pinned Asset build source")(error));
    }
    let staged = inspect_archive(staging.clone()).await?;
    if let Ok(metadata) = tokio::fs::symlink_metadata(&target).await {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            let _ = tokio::fs::remove_file(&staging).await;
            return Err(integrity(
                "Asset build input path is not an owned regular file",
            ));
        }
        let existing = inspect_archive(target.clone()).await?;
        let _ = tokio::fs::remove_file(&staging).await;
        if existing != staged {
            return Err(integrity(
                "replayed Asset build input changed its immutable archive",
            ));
        }
    } else {
        tokio::fs::rename(&staging, &target)
            .await
            .map_err(|error| storage(format!("could not commit Asset build input: {error}")))?;
        sync_directories([store.build_input_root.clone()]).await?;
    }
    Ok(PreparedGitArchive {
        commit_sha: commit_sha.clone(),
        digest: staged.0,
        size_bytes: staged.1,
        path: target,
    })
}

pub(super) async fn remove(
    store: &LocalAssetGitRepository,
    build_run_id: BuildRunId,
) -> Result<(), AssetGitRepositoryError> {
    if build_run_id.as_uuid().is_nil() {
        return Err(AssetGitRepositoryError::Invalid(
            "Asset build run ID cannot be nil".into(),
        ));
    }
    remove_archive(store, &build_run_id.to_string()).await
}

pub(super) async fn remove_release(
    store: &LocalAssetGitRepository,
    asset_release_id: AssetReleaseId,
) -> Result<(), AssetGitRepositoryError> {
    if asset_release_id.as_uuid().is_nil() {
        return Err(AssetGitRepositoryError::Invalid(
            "Asset release ID cannot be nil".into(),
        ));
    }
    remove_archive(store, &format!("release-{asset_release_id}")).await
}

async fn remove_archive(
    store: &LocalAssetGitRepository,
    key: &str,
) -> Result<(), AssetGitRepositoryError> {
    let target = store.build_input_root.join(format!("{key}.tar"));
    match tokio::fs::symlink_metadata(&target).await {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(storage(format!(
                "could not inspect Asset build input: {error}"
            )))
        }
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(integrity(
                "Asset build input path is not an owned regular file",
            ))
        }
        Ok(_) => {}
    }
    tokio::fs::remove_file(target)
        .await
        .map_err(|error| storage(format!("could not remove Asset build input: {error}")))?;
    sync_directories([store.build_input_root.clone()]).await
}

async fn inspect_archive(path: PathBuf) -> Result<(Sha256Digest, u64), AssetGitRepositoryError> {
    tokio::task::spawn_blocking(move || digest_file(&path))
        .await
        .map_err(|error| storage(format!("Asset build input digest task failed: {error}")))?
}

fn digest_file(path: &Path) -> Result<(Sha256Digest, u64), AssetGitRepositoryError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| storage(format!("could not inspect Asset build input: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
        return Err(integrity(
            "Asset build input archive is not an owned non-empty file",
        ));
    }
    let mut file = std::fs::File::open(path)
        .map_err(|error| storage(format!("could not open Asset build input: {error}")))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| storage(format!("could not read Asset build input: {error}")))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    file.sync_all()
        .map_err(|error| storage(format!("could not sync Asset build input: {error}")))?;
    let digest = Sha256Digest::parse(format!("sha256:{:x}", digest.finalize()))
        .map_err(AssetGitRepositoryError::Integrity)?;
    Ok((digest, metadata.len()))
}
