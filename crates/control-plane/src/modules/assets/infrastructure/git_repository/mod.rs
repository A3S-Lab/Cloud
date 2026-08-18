mod backup;
mod build_input;
mod journal;
mod manifest;
mod protocol;
#[cfg(test)]
mod tests;

use crate::infrastructure::{
    sync_directories as sync_filesystem_directories, sync_directory as sync_filesystem_directory,
    GitCommandError, GitCommandRunner, ImmutableObjectClient,
};
use crate::modules::assets::domain::{
    validate_asset_repository_mutation, Asset, AssetGitBackup, AssetGitBuildInput,
    AssetGitReleaseBundle, AssetGitRepository, AssetGitRepositoryError, AssetGitRepositoryWrite,
    AssetGitRpcLimits, AssetGitRpcResponse, AssetGitService, AssetGitWriteJournal,
    AssetGitWriteLease, AssetManifestAdmission, IAssetGitRepository, DEFAULT_ASSET_BRANCH,
};
use crate::modules::shared_kernel::domain::{
    AssetReleaseId, BuildRunId, GitCommitSha, Sha256Digest,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

const REPOSITORY_SCHEMA: &str = "a3s.cloud.asset-git-repository.v1";
const STORAGE_IDENTITY_SCHEMA: &str = "a3s.cloud.asset-git-storage.v1";
const STORAGE_IDENTITY_DIRECTORY: &str = ".storage-identity";
const STORAGE_IDENTITY_FILE: &str = "identity.json";
const MAX_STORAGE_IDENTITY_BYTES: u64 = 1024;
const MAX_ROOT_PATH_BYTES: usize = 4096;
const MAX_GIT_COMMAND_TIMEOUT: Duration = Duration::from_secs(600);

pub struct LocalAssetGitRepository {
    root: PathBuf,
    storage_id: Uuid,
    staging_root: PathBuf,
    build_input_root: PathBuf,
    git_home: PathBuf,
    hooks: PathBuf,
    commands: GitCommandRunner,
    backup_objects: Option<ImmutableObjectClient>,
    backup_max_bytes: u64,
}

impl LocalAssetGitRepository {
    pub fn new(
        root: impl Into<PathBuf>,
        command_timeout: Duration,
    ) -> Result<Self, AssetGitRepositoryError> {
        let root = root.into();
        let root_text = root.to_str().ok_or_else(|| {
            AssetGitRepositoryError::Invalid("repository root must be UTF-8".into())
        })?;
        if root_text.is_empty()
            || root_text.len() > MAX_ROOT_PATH_BYTES
            || root_text.contains('\0')
            || command_timeout.is_zero()
            || command_timeout > MAX_GIT_COMMAND_TIMEOUT
        {
            return Err(AssetGitRepositoryError::Invalid(
                "repository options are invalid".into(),
            ));
        }
        create_secure_directory_sync(&root, "root")?;
        let root =
            GitCommandRunner::normalize_path(std::fs::canonicalize(root).map_err(|error| {
                storage(format!("could not canonicalize repository root: {error}"))
            })?);
        let storage_id = load_or_create_storage_identity(&root)?;
        let staging_root = root.join(".repository-staging");
        let build_input_root = root.join(".build-inputs");
        let sandbox = root.join(".git-command-sandbox");
        let git_home = sandbox.join("home");
        let hooks = sandbox.join("hooks");
        for (path, label) in [
            (&staging_root, "staging root"),
            (&build_input_root, "build input root"),
            (&sandbox, "Git sandbox"),
            (&git_home, "Git home"),
            (&hooks, "Git hooks"),
        ] {
            create_secure_directory_sync(path, label)?;
        }
        let commands = GitCommandRunner::discover(command_timeout, false, false)
            .map_err(git_storage("initialize hosted Git command runner"))?;
        Ok(Self {
            root,
            storage_id,
            staging_root,
            build_input_root,
            git_home,
            hooks,
            commands,
            backup_objects: None,
            backup_max_bytes: 0,
        })
    }

    pub(crate) fn with_backup_objects(
        mut self,
        objects: ImmutableObjectClient,
        maximum_bytes: u64,
    ) -> Result<Self, AssetGitRepositoryError> {
        if maximum_bytes == 0 {
            return Err(AssetGitRepositoryError::Invalid(
                "backup maximum must be positive".into(),
            ));
        }
        self.backup_objects = Some(objects);
        self.backup_max_bytes = maximum_bytes;
        Ok(self)
    }

    /// Stable identity of the exact hosted-Git filesystem. Composition binds
    /// these bytes through PostgreSQL before API or Worker capabilities start.
    pub(crate) fn infrastructure_identity(&self) -> &[u8] {
        self.storage_id.as_bytes()
    }

    async fn prepare(
        &self,
        asset: &Asset,
        staging: &Path,
    ) -> Result<AssetGitRepository, AssetGitRepositoryError> {
        self.git(vec![
            "init".into(),
            "--bare".into(),
            "--quiet".into(),
            format!("--initial-branch={DEFAULT_ASSET_BRANCH}").into(),
            staging.as_os_str().to_owned(),
        ])
        .await
        .map_err(git_storage("initialize bare Asset repository"))?;
        secure_directory(staging, "staged repository").await?;
        for (key, value) in [
            ("a3s.schema", REPOSITORY_SCHEMA.to_owned()),
            ("a3s.organization-id", asset.organization_id.to_string()),
            ("a3s.asset-id", asset.id.to_string()),
            ("receive.fsckObjects", "true".into()),
            ("transfer.fsckObjects", "true".into()),
            ("http.receivepack", "true".into()),
            ("receive.autogc", "false".into()),
        ] {
            self.git(vec![
                git_directory(staging),
                "config".into(),
                "--local".into(),
                key.into(),
                value.into(),
            ])
            .await
            .map_err(git_storage("configure bare Asset repository"))?;
        }
        self.inspect_path(asset, staging).await
    }

    async fn inspect_optional(
        &self,
        asset: &Asset,
        path: &Path,
    ) -> Result<Option<AssetGitRepository>, AssetGitRepositoryError> {
        let metadata = match tokio::fs::symlink_metadata(path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(storage(format!(
                    "could not inspect hosted Git repository: {error}"
                )))
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(integrity(
                "repository path is not an owned bare repository directory",
            ));
        }
        self.inspect_path(asset, path).await.map(Some)
    }

    async fn inspect_path(
        &self,
        asset: &Asset,
        path: &Path,
    ) -> Result<AssetGitRepository, AssetGitRepositoryError> {
        let repository =
            AssetGitRepository::for_asset(asset).map_err(AssetGitRepositoryError::Invalid)?;
        let checks = [
            (
                vec![
                    git_directory(path),
                    "rev-parse".into(),
                    "--is-bare-repository".into(),
                ],
                "true".to_owned(),
            ),
            (
                vec![git_directory(path), "symbolic-ref".into(), "HEAD".into()],
                format!("refs/heads/{DEFAULT_ASSET_BRANCH}"),
            ),
            (config_get(path, "a3s.schema"), REPOSITORY_SCHEMA.to_owned()),
            (
                config_get(path, "a3s.organization-id"),
                asset.organization_id.to_string(),
            ),
            (config_get(path, "a3s.asset-id"), asset.id.to_string()),
            (
                config_get_bool(path, "receive.fsckObjects"),
                "true".to_owned(),
            ),
            (
                config_get_bool(path, "transfer.fsckObjects"),
                "true".to_owned(),
            ),
            (config_get_bool(path, "http.receivepack"), "true".to_owned()),
            (config_get_bool(path, "receive.autogc"), "false".to_owned()),
        ];
        for (command, expected) in checks {
            let actual = self
                .git(command)
                .await
                .map_err(git_integrity("inspect bare Asset repository"))?;
            if one_line(actual)? != expected {
                return Err(integrity("repository metadata changed identity"));
            }
        }
        repository
            .validate_for(asset)
            .map_err(AssetGitRepositoryError::Integrity)?;
        Ok(repository)
    }

    async fn git(&self, args: Vec<OsString>) -> Result<Vec<u8>, GitCommandError> {
        self.commands
            .run(&self.root, &self.git_home, &self.hooks, &args, None)
            .await
    }

    fn repository_path(&self, asset: &Asset) -> PathBuf {
        self.root
            .join(asset.organization_id.to_string())
            .join(format!("{}.git", asset.id))
    }

    fn organization_path(&self, asset: &Asset) -> PathBuf {
        self.root.join(asset.organization_id.to_string())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StorageIdentity {
    schema: String,
    storage_id: Uuid,
}

fn load_or_create_storage_identity(root: &Path) -> Result<Uuid, AssetGitRepositoryError> {
    let target = root.join(STORAGE_IDENTITY_DIRECTORY);
    if target.exists() {
        return load_storage_identity(&target);
    }

    let storage_id = Uuid::now_v7();
    let staging = root.join(format!(
        "{STORAGE_IDENTITY_DIRECTORY}-{}.pending",
        Uuid::now_v7()
    ));
    create_secure_directory_sync(&staging, "repository storage identity staging")?;
    let body = serde_json::to_vec(&StorageIdentity {
        schema: STORAGE_IDENTITY_SCHEMA.into(),
        storage_id,
    })
    .map_err(|error| {
        storage(format!(
            "could not encode repository storage identity: {error}"
        ))
    })?;
    if body.is_empty() || body.len() as u64 > MAX_STORAGE_IDENTITY_BYTES {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(integrity("repository storage identity is not bounded"));
    }
    let marker = staging.join(STORAGE_IDENTITY_FILE);
    let written = (|| -> Result<(), AssetGitRepositoryError> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = options.open(&marker).map_err(|error| {
            storage(format!(
                "could not create repository storage identity: {error}"
            ))
        })?;
        file.write_all(&body).map_err(|error| {
            storage(format!(
                "could not write repository storage identity: {error}"
            ))
        })?;
        file.sync_all().map_err(|error| {
            storage(format!(
                "could not sync repository storage identity: {error}"
            ))
        })?;
        sync_filesystem_directory(&staging).map_err(|error| {
            storage(format!(
                "could not sync repository storage identity directory: {error}"
            ))
        })
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }

    match std::fs::rename(&staging, &target) {
        Ok(()) => {
            sync_filesystem_directory(root).map_err(|error| {
                storage(format!(
                    "could not publish repository storage identity: {error}"
                ))
            })?;
            Ok(storage_id)
        }
        Err(_) if target.exists() => {
            let _ = std::fs::remove_dir_all(&staging);
            load_storage_identity(&target)
        }
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging);
            Err(storage(format!(
                "could not publish repository storage identity: {error}"
            )))
        }
    }
}

fn load_storage_identity(directory: &Path) -> Result<Uuid, AssetGitRepositoryError> {
    let metadata = std::fs::symlink_metadata(directory).map_err(|error| {
        storage(format!(
            "could not inspect repository storage identity: {error}"
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(integrity(
            "repository storage identity path is not an owned directory",
        ));
    }
    let entries = std::fs::read_dir(directory)
        .map_err(|error| {
            storage(format!(
                "could not list repository storage identity: {error}"
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            storage(format!(
                "could not list repository storage identity: {error}"
            ))
        })?;
    if entries.len() != 1 || entries[0].file_name() != STORAGE_IDENTITY_FILE {
        return Err(integrity("repository storage identity directory changed"));
    }
    let marker = entries[0].path();
    let marker_metadata = std::fs::symlink_metadata(&marker).map_err(|error| {
        storage(format!(
            "could not inspect repository storage identity file: {error}"
        ))
    })?;
    if marker_metadata.file_type().is_symlink()
        || !marker_metadata.is_file()
        || marker_metadata.len() == 0
        || marker_metadata.len() > MAX_STORAGE_IDENTITY_BYTES
    {
        return Err(integrity("repository storage identity file changed"));
    }
    let bytes = std::fs::read(&marker).map_err(|error| {
        storage(format!(
            "could not read repository storage identity: {error}"
        ))
    })?;
    let identity: StorageIdentity = serde_json::from_slice(&bytes)
        .map_err(|_| integrity("repository storage identity is malformed"))?;
    if identity.schema != STORAGE_IDENTITY_SCHEMA || identity.storage_id.is_nil() {
        return Err(integrity("repository storage identity is invalid"));
    }
    Ok(identity.storage_id)
}

#[async_trait]
impl IAssetGitRepository for LocalAssetGitRepository {
    async fn provision(
        &self,
        asset: &Asset,
    ) -> Result<AssetGitRepositoryWrite, AssetGitRepositoryError> {
        validate_asset_repository_mutation(asset).map_err(AssetGitRepositoryError::Invalid)?;
        let target = self.repository_path(asset);
        if let Some(repository) = self.inspect_optional(asset, &target).await? {
            return Ok(AssetGitRepositoryWrite {
                repository,
                created: false,
            });
        }

        let staging = self.staging_root.join(format!("{}.git", Uuid::now_v7()));
        let prepared = self.prepare(asset, &staging).await;
        let repository = match prepared {
            Ok(repository) => repository,
            Err(error) => {
                remove_staging(&staging).await;
                return Err(error);
            }
        };
        let organization = self.organization_path(asset);
        if let Err(error) =
            create_secure_directory(&organization, "organization repository root").await
        {
            remove_staging(&staging).await;
            return Err(error);
        }
        match tokio::fs::rename(&staging, &target).await {
            Ok(()) => {
                sync_directories([organization, self.root.clone()]).await?;
                Ok(AssetGitRepositoryWrite {
                    repository,
                    created: true,
                })
            }
            Err(_) if tokio::fs::symlink_metadata(&target).await.is_ok() => {
                remove_staging(&staging).await;
                let repository = self.inspect_path(asset, &target).await?;
                Ok(AssetGitRepositoryWrite {
                    repository,
                    created: false,
                })
            }
            Err(error) => {
                remove_staging(&staging).await;
                Err(storage(format!(
                    "could not publish hosted Git repository: {error}"
                )))
            }
        }
    }

    async fn inspect(&self, asset: &Asset) -> Result<AssetGitRepository, AssetGitRepositoryError> {
        asset.validate().map_err(AssetGitRepositoryError::Invalid)?;
        self.inspect_optional(asset, &self.repository_path(asset))
            .await?
            .ok_or(AssetGitRepositoryError::NotFound)
    }

    async fn prepare_write(
        &self,
        asset: &Asset,
        lease: &AssetGitWriteLease,
    ) -> Result<(), AssetGitRepositoryError> {
        journal::prepare(self, asset, lease).await
    }

    async fn rollback_write(
        &self,
        asset: &Asset,
        lease: &AssetGitWriteLease,
    ) -> Result<(), AssetGitRepositoryError> {
        journal::rollback(self, asset, lease).await
    }

    async fn settle_write(
        &self,
        asset: &Asset,
        journal: &AssetGitWriteJournal,
    ) -> Result<(), AssetGitRepositoryError> {
        journal::settle(self, asset, journal).await
    }

    async fn advertise(
        &self,
        asset: &Asset,
        service: AssetGitService,
    ) -> Result<Vec<u8>, AssetGitRepositoryError> {
        protocol::advertise(self, asset, service).await
    }

    async fn execute_rpc(
        &self,
        asset: &Asset,
        service: AssetGitService,
        request: Vec<u8>,
        limits: AssetGitRpcLimits,
        write_lease: Option<&AssetGitWriteLease>,
    ) -> Result<AssetGitRpcResponse, AssetGitRepositoryError> {
        protocol::execute_rpc(self, asset, service, request, limits, write_lease).await
    }

    async fn repository_bytes(&self, asset: &Asset) -> Result<u64, AssetGitRepositoryError> {
        protocol::repository_bytes(self, asset).await
    }

    async fn refs_digest(&self, asset: &Asset) -> Result<Sha256Digest, AssetGitRepositoryError> {
        protocol::refs_digest(self, asset).await
    }

    async fn create_backup(
        &self,
        asset: &Asset,
        lease: &AssetGitWriteLease,
        created_at: DateTime<Utc>,
    ) -> Result<AssetGitBackup, AssetGitRepositoryError> {
        backup::create(self, asset, lease, created_at).await
    }

    async fn restore_backup(
        &self,
        asset: &Asset,
        lease: &AssetGitWriteLease,
        backup: &AssetGitBackup,
        maximum_repository_bytes: u64,
    ) -> Result<AssetGitRpcResponse, AssetGitRepositoryError> {
        backup::restore(self, asset, lease, backup, maximum_repository_bytes).await
    }

    async fn admit_manifest(
        &self,
        asset: &Asset,
        commit_sha: &GitCommitSha,
    ) -> Result<AssetManifestAdmission, AssetGitRepositoryError> {
        manifest::admit(self, asset, commit_sha).await
    }

    async fn prepare_build_input(
        &self,
        asset: &Asset,
        commit_sha: &GitCommitSha,
        build_run_id: BuildRunId,
    ) -> Result<AssetGitBuildInput, AssetGitRepositoryError> {
        build_input::prepare(self, asset, commit_sha, build_run_id).await
    }

    async fn remove_build_input(
        &self,
        build_run_id: BuildRunId,
    ) -> Result<(), AssetGitRepositoryError> {
        build_input::remove(self, build_run_id).await
    }

    async fn prepare_release_bundle(
        &self,
        asset: &Asset,
        commit_sha: &GitCommitSha,
        asset_release_id: AssetReleaseId,
    ) -> Result<AssetGitReleaseBundle, AssetGitRepositoryError> {
        build_input::prepare_release(self, asset, commit_sha, asset_release_id).await
    }

    async fn remove_release_bundle(
        &self,
        asset_release_id: AssetReleaseId,
    ) -> Result<(), AssetGitRepositoryError> {
        build_input::remove_release(self, asset_release_id).await
    }
}

fn config_get(path: &Path, key: &str) -> Vec<OsString> {
    vec![
        git_directory(path),
        "config".into(),
        "--local".into(),
        "--get".into(),
        key.into(),
    ]
}

fn config_get_bool(path: &Path, key: &str) -> Vec<OsString> {
    vec![
        git_directory(path),
        "config".into(),
        "--local".into(),
        "--bool".into(),
        "--get".into(),
        key.into(),
    ]
}

fn git_directory(path: &Path) -> OsString {
    format!("--git-dir={}", path.display()).into()
}

fn one_line(output: Vec<u8>) -> Result<String, AssetGitRepositoryError> {
    let value = std::str::from_utf8(&output)
        .map_err(|_| integrity("Git repository metadata is not UTF-8"))?
        .trim_end_matches(['\r', '\n']);
    if value.is_empty() || value.contains(['\r', '\n']) {
        return Err(integrity("Git repository metadata is malformed"));
    }
    Ok(value.to_owned())
}

fn create_secure_directory_sync(path: &Path, label: &str) -> Result<(), AssetGitRepositoryError> {
    std::fs::create_dir_all(path)
        .map_err(|error| storage(format!("could not create {label}: {error}")))?;
    secure_directory_sync(path, label)
}

fn secure_directory_sync(path: &Path, label: &str) -> Result<(), AssetGitRepositoryError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| storage(format!("could not inspect {label}: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(integrity(format!("{label} is not an owned directory")));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| storage(format!("could not secure {label}: {error}")))?;
    }
    Ok(())
}

async fn create_secure_directory(path: &Path, label: &str) -> Result<(), AssetGitRepositoryError> {
    tokio::fs::create_dir_all(path)
        .await
        .map_err(|error| storage(format!("could not create {label}: {error}")))?;
    secure_directory(path, label).await
}

async fn secure_directory(path: &Path, label: &str) -> Result<(), AssetGitRepositoryError> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|error| storage(format!("could not inspect {label}: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(integrity(format!("{label} is not an owned directory")));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .await
            .map_err(|error| storage(format!("could not secure {label}: {error}")))?;
    }
    Ok(())
}

async fn sync_directories(
    paths: impl IntoIterator<Item = PathBuf>,
) -> Result<(), AssetGitRepositoryError> {
    let paths = paths.into_iter().collect::<Vec<_>>();
    sync_filesystem_directories(paths)
        .await
        .map_err(|error| storage(format!("repository directory sync failed: {error}")))
}

async fn remove_staging(path: &Path) {
    let _ = tokio::fs::remove_dir_all(path).await;
}

fn git_storage(action: &'static str) -> impl FnOnce(GitCommandError) -> AssetGitRepositoryError {
    move |error| storage(format!("could not {action}: {error}"))
}

fn git_integrity(action: &'static str) -> impl FnOnce(GitCommandError) -> AssetGitRepositoryError {
    move |error| match error {
        GitCommandError::Failed => integrity(format!("could not {action}")),
        other => storage(format!("could not {action}: {other}")),
    }
}

fn integrity(message: impl Into<String>) -> AssetGitRepositoryError {
    AssetGitRepositoryError::Integrity(message.into())
}

fn storage(message: impl Into<String>) -> AssetGitRepositoryError {
    AssetGitRepositoryError::Storage(message.into())
}
