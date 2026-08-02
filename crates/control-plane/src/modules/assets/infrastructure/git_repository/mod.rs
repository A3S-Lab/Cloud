mod backup;
mod journal;
mod manifest;
mod protocol;
#[cfg(test)]
mod tests;

use crate::infrastructure::{GitCommandError, GitCommandRunner, ImmutableObjectClient};
use crate::modules::assets::domain::{
    validate_asset_repository_mutation, Asset, AssetGitBackup, AssetGitRepository,
    AssetGitRepositoryError, AssetGitRepositoryWrite, AssetGitRpcLimits, AssetGitRpcResponse,
    AssetGitService, AssetGitWriteJournal, AssetGitWriteLease, AssetManifestAdmission,
    IAssetGitRepository, DEFAULT_ASSET_BRANCH,
};
use crate::modules::shared_kernel::domain::{GitCommitSha, Sha256Digest};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

const REPOSITORY_SCHEMA: &str = "a3s.cloud.asset-git-repository.v1";
const MAX_ROOT_PATH_BYTES: usize = 4096;
const MAX_GIT_COMMAND_TIMEOUT: Duration = Duration::from_secs(600);

pub struct LocalAssetGitRepository {
    root: PathBuf,
    staging_root: PathBuf,
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
        let root = std::fs::canonicalize(root)
            .map_err(|error| storage(format!("could not canonicalize repository root: {error}")))?;
        let staging_root = root.join(".repository-staging");
        let sandbox = root.join(".git-command-sandbox");
        let git_home = sandbox.join("home");
        let hooks = sandbox.join("hooks");
        for (path, label) in [
            (&staging_root, "staging root"),
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
            staging_root,
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
    tokio::task::spawn_blocking(move || {
        for path in paths {
            std::fs::File::open(&path)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| {
                    storage(format!(
                        "could not sync repository directory {}: {error}",
                        path.display()
                    ))
                })?;
        }
        Ok(())
    })
    .await
    .map_err(|error| storage(format!("repository directory sync failed: {error}")))?
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
