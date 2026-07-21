mod command;
mod receipt;
mod tree;

use self::command::{GitCommandError, GitCommandRunner};
use self::receipt::{read_receipt, write_receipt, CheckoutReceipt, RECEIPT_SCHEMA};
use self::tree::{digest_worktree, valid_object_id, valid_sha256, GitTreeManifest};
use crate::modules::sources::domain::{
    CheckedOutSource, GitProvider, ISourceCheckout, SourceCheckoutError, SourceCheckoutRequest,
    SourceProviderCredential,
};
use async_trait::async_trait;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

const MAX_CHECKOUT_BYTES: u64 = 1024 * 1024 * 1024 * 1024;

pub struct GitSourceCheckout {
    root: PathBuf,
    timeout: Duration,
    max_files: usize,
    max_bytes: u64,
    commands: GitCommandRunner,
    #[cfg(test)]
    test_remote: Option<OsString>,
}

impl GitSourceCheckout {
    pub fn new(
        root: impl Into<PathBuf>,
        timeout: Duration,
        max_files: usize,
        max_bytes: u64,
    ) -> Result<Self, String> {
        let root = root.into();
        let root_text = root
            .to_str()
            .ok_or_else(|| "Git source checkout root must be UTF-8".to_owned())?;
        if root_text.is_empty()
            || root_text.len() > 4096
            || root_text.contains('\0')
            || timeout.is_zero()
            || timeout > Duration::from_secs(600)
            || max_files == 0
            || max_files > 1_000_000
            || max_bytes == 0
            || max_bytes > MAX_CHECKOUT_BYTES
        {
            return Err("Git source checkout options are invalid".into());
        }
        let commands = GitCommandRunner::discover(timeout, false, false)
            .map_err(|error| format!("could not initialize Git source checkout: {error}"))?;
        Ok(Self {
            root,
            timeout,
            max_files,
            max_bytes,
            commands,
            #[cfg(test)]
            test_remote: None,
        })
    }

    #[cfg(test)]
    fn for_test(
        root: impl Into<PathBuf>,
        timeout: Duration,
        max_files: usize,
        max_bytes: u64,
        remote: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        let mut checkout = Self::new(root, timeout, max_files, max_bytes)?;
        checkout.commands = GitCommandRunner::discover(timeout, true, false)
            .map_err(|error| format!("could not initialize test Git source checkout: {error}"))?;
        checkout.test_remote = Some(
            remote
                .into()
                .canonicalize()
                .map_err(|_| "test Git remote is unavailable".to_owned())?
                .into_os_string(),
        );
        Ok(checkout)
    }

    #[cfg(test)]
    fn for_http_test(
        root: impl Into<PathBuf>,
        timeout: Duration,
        max_files: usize,
        max_bytes: u64,
        remote: &str,
    ) -> Result<Self, String> {
        let remote_url =
            url::Url::parse(remote).map_err(|_| "test Git HTTP remote is invalid".to_owned())?;
        if remote_url.scheme() != "http"
            || remote_url.host_str() != Some("127.0.0.1")
            || remote_url.port().is_none()
            || !remote_url.username().is_empty()
            || remote_url.password().is_some()
        {
            return Err("test Git HTTP remote is invalid".into());
        }
        let mut checkout = Self::new(root, timeout, max_files, max_bytes)?;
        checkout.commands = GitCommandRunner::discover(timeout, false, true)
            .map_err(|error| format!("could not initialize test Git HTTP checkout: {error}"))?;
        checkout.test_remote = Some(remote.into());
        Ok(checkout)
    }

    async fn prepare(
        &self,
        request: &SourceCheckoutRequest,
        staging: &Path,
        credential: Option<&SourceProviderCredential>,
    ) -> Result<CheckoutReceipt, SourceCheckoutError> {
        let source = staging.join("source");
        let sandbox = staging.join("sandbox");
        let home = sandbox.join("home");
        let hooks = sandbox.join("hooks");
        for directory in [&source, &home, &hooks] {
            tokio::fs::create_dir_all(directory)
                .await
                .map_err(|_| storage("could not create source checkout directory"))?;
        }

        self.git(
            staging,
            &home,
            &hooks,
            vec![
                "init".into(),
                "--quiet".into(),
                "--initial-branch=a3s-checkout".into(),
                source.as_os_str().to_owned(),
            ],
            false,
            None,
        )
        .await?;
        self.git(
            &source,
            &home,
            &hooks,
            vec![
                "fetch".into(),
                "--quiet".into(),
                "--depth=1".into(),
                "--no-tags".into(),
                "--no-recurse-submodules".into(),
                self.remote(request),
                request.commit_sha.as_str().into(),
            ],
            true,
            credential,
        )
        .await?;
        self.git(
            &source,
            &home,
            &hooks,
            vec![
                "checkout".into(),
                "--quiet".into(),
                "--detach".into(),
                "FETCH_HEAD".into(),
            ],
            false,
            None,
        )
        .await?;
        let commit_sha = one_line(
            self.git(
                &source,
                &home,
                &hooks,
                vec![
                    "rev-parse".into(),
                    "--verify".into(),
                    "HEAD^{commit}".into(),
                ],
                false,
                None,
            )
            .await?,
            "Git returned an invalid checked-out commit",
        )?;
        if commit_sha != request.commit_sha.as_str() {
            return Err(integrity(
                "checked-out commit does not match the accepted commit",
            ));
        }
        let git_tree_id = one_line(
            self.git(
                &source,
                &home,
                &hooks,
                vec!["rev-parse".into(), "--verify".into(), "HEAD^{tree}".into()],
                false,
                None,
            )
            .await?,
            "Git returned an invalid tree ID",
        )?;
        if !valid_object_id(&git_tree_id) {
            return Err(integrity("Git returned an invalid tree ID"));
        }
        let tree = self
            .git(
                &source,
                &home,
                &hooks,
                vec![
                    "ls-tree".into(),
                    "-rlz".into(),
                    "--full-tree".into(),
                    "HEAD".into(),
                ],
                false,
                None,
            )
            .await?;
        let manifest = GitTreeManifest::parse(&tree, self.max_files, self.max_bytes)?;
        let status = self
            .git(
                &source,
                &home,
                &hooks,
                vec![
                    "status".into(),
                    "--porcelain=v1".into(),
                    "--untracked-files=all".into(),
                ],
                false,
                None,
            )
            .await?;
        if !status.is_empty() {
            return Err(integrity(
                "checked-out files differ from the accepted Git tree",
            ));
        }
        tokio::fs::remove_dir_all(source.join(".git"))
            .await
            .map_err(|_| storage("could not remove Git metadata from checked-out source"))?;
        tokio::fs::remove_dir_all(&sandbox)
            .await
            .map_err(|_| storage("could not remove source checkout sandbox"))?;
        let (digest, scanned) = digest_worktree(&source, self.max_files, self.max_bytes).await?;
        manifest.validate_worktree(&scanned)?;
        let receipt = CheckoutReceipt {
            schema: RECEIPT_SCHEMA.into(),
            checkout_id: request.checkout_id,
            repository_identity: request.repository.identity().into(),
            repository_url: request.repository.canonical_url().into(),
            commit_sha,
            git_tree_id,
            content_digest: digest.digest,
            file_count: digest.file_count,
            content_bytes: digest.content_bytes,
        };
        write_receipt(staging, &receipt).await?;
        Ok(receipt)
    }

    async fn existing(
        &self,
        request: &SourceCheckoutRequest,
        checkout: &Path,
    ) -> Result<CheckedOutSource, SourceCheckoutError> {
        require_directory(checkout, "source checkout path").await?;
        let receipt = read_receipt(checkout).await?;
        if receipt.checkout_id != request.checkout_id
            || receipt.repository_identity != request.repository.identity()
            || receipt.repository_url != request.repository.canonical_url()
            || receipt.commit_sha != request.commit_sha.as_str()
        {
            return Err(SourceCheckoutError::Conflict);
        }
        if receipt.schema != RECEIPT_SCHEMA
            || !valid_object_id(&receipt.git_tree_id)
            || !valid_sha256(&receipt.content_digest)
            || receipt.file_count > self.max_files
            || receipt.content_bytes > self.max_bytes
        {
            return Err(integrity("source checkout receipt is invalid"));
        }
        let source = checkout.join("source");
        require_directory(&source, "checked-out source path").await?;
        if tokio::fs::symlink_metadata(source.join(".git"))
            .await
            .is_ok()
        {
            return Err(integrity("checked-out source contains Git metadata"));
        }
        let (digest, _) = digest_worktree(&source, self.max_files, self.max_bytes).await?;
        if digest.digest != receipt.content_digest
            || digest.file_count != receipt.file_count
            || digest.content_bytes != receipt.content_bytes
        {
            return Err(integrity(
                "checked-out source no longer matches its immutable receipt",
            ));
        }
        Ok(receipt.checked_out_source(source, request))
    }

    async fn replay(
        &self,
        request: &SourceCheckoutRequest,
        checkout: &Path,
    ) -> Result<CheckedOutSource, SourceCheckoutError> {
        tokio::time::timeout(self.timeout, self.existing(request, checkout))
            .await
            .map_err(|_| {
                SourceCheckoutError::Unavailable(
                    "source checkout replay exceeded its deadline".into(),
                )
            })?
    }

    async fn git(
        &self,
        working_directory: &Path,
        home: &Path,
        hooks: &Path,
        args: Vec<OsString>,
        provider_access: bool,
        credential: Option<&SourceProviderCredential>,
    ) -> Result<Vec<u8>, SourceCheckoutError> {
        self.commands
            .run(
                working_directory,
                home,
                hooks,
                &args,
                credential.map(SourceProviderCredential::expose_token),
            )
            .await
            .map_err(|error| map_git_error(error, provider_access))
    }

    fn remote(&self, request: &SourceCheckoutRequest) -> OsString {
        #[cfg(test)]
        if let Some(remote) = &self.test_remote {
            return remote.clone();
        }
        format!("{}.git", request.repository.canonical_url()).into()
    }
}

#[async_trait]
impl ISourceCheckout for GitSourceCheckout {
    async fn checkout(
        &self,
        request: &SourceCheckoutRequest,
        credential: Option<&SourceProviderCredential>,
    ) -> Result<CheckedOutSource, SourceCheckoutError> {
        validate_request(request)?;
        let root = ensure_root(&self.root).await?;
        let checkout = root.join(request.checkout_id.to_string());
        match tokio::fs::symlink_metadata(&checkout).await {
            Ok(_) => return self.replay(request, &checkout).await,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(storage("could not inspect source checkout path")),
        }
        validate_credential(request, credential, self.timeout)?;
        let staging = root.join(format!(".{}-{}.tmp", request.checkout_id, Uuid::now_v7()));
        tokio::fs::create_dir(&staging)
            .await
            .map_err(|_| storage("could not create source checkout staging directory"))?;
        let prepared =
            tokio::time::timeout(self.timeout, self.prepare(request, &staging, credential)).await;
        match prepared {
            Err(_) => {
                remove_staging(&staging).await;
                return Err(SourceCheckoutError::Unavailable(
                    "Git checkout exceeded its deadline".into(),
                ));
            }
            Ok(Err(error)) => {
                remove_staging(&staging).await;
                return Err(error);
            }
            Ok(Ok(_)) => {}
        }
        match tokio::fs::rename(&staging, &checkout).await {
            Ok(()) => self.replay(request, &checkout).await,
            Err(_) if tokio::fs::symlink_metadata(&checkout).await.is_ok() => {
                remove_staging(&staging).await;
                self.replay(request, &checkout).await
            }
            Err(_) => {
                remove_staging(&staging).await;
                Err(storage("could not commit source checkout"))
            }
        }
    }

    async fn remove(&self, checkout_id: Uuid) -> Result<(), SourceCheckoutError> {
        if checkout_id.is_nil() {
            return Err(SourceCheckoutError::Invalid(
                "source checkout ID cannot be nil".into(),
            ));
        }
        let root_metadata = match tokio::fs::symlink_metadata(&self.root).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(storage("could not inspect source checkout root")),
        };
        if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
            return Err(integrity("source checkout root is not an owned directory"));
        }
        let root = match tokio::fs::canonicalize(&self.root).await {
            Ok(root) => root,
            Err(_) => return Err(storage("could not inspect source checkout root")),
        };
        let checkout = root.join(checkout_id.to_string());
        match tokio::fs::symlink_metadata(&checkout).await {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(storage("could not inspect source checkout path")),
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                Err(integrity("source checkout path is not an owned directory"))
            }
            Ok(_) => tokio::fs::remove_dir_all(checkout)
                .await
                .map_err(|_| storage("could not remove source checkout")),
        }
    }
}

async fn ensure_root(root: &Path) -> Result<PathBuf, SourceCheckoutError> {
    tokio::fs::create_dir_all(root)
        .await
        .map_err(|_| storage("could not create source checkout root"))?;
    let metadata = tokio::fs::symlink_metadata(root)
        .await
        .map_err(|_| storage("could not inspect source checkout root"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(integrity("source checkout root is not an owned directory"));
    }
    tokio::fs::canonicalize(root)
        .await
        .map_err(|_| storage("could not canonicalize source checkout root"))
}

async fn require_directory(path: &Path, label: &str) -> Result<(), SourceCheckoutError> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|_| integrity(format!("{label} is unavailable")))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(integrity(format!("{label} is not an owned directory")));
    }
    Ok(())
}

async fn remove_staging(path: &Path) {
    let _ = tokio::fs::remove_dir_all(path).await;
}

fn one_line(output: Vec<u8>, message: &str) -> Result<String, SourceCheckoutError> {
    let value = std::str::from_utf8(&output)
        .map_err(|_| integrity(message))?
        .trim_end_matches(['\r', '\n']);
    if value.is_empty() || value.contains(['\r', '\n']) {
        return Err(integrity(message));
    }
    Ok(value.to_owned())
}

fn validate_request(request: &SourceCheckoutRequest) -> Result<(), SourceCheckoutError> {
    if request.checkout_id.is_nil() {
        return Err(SourceCheckoutError::Invalid(
            "source checkout ID cannot be nil".into(),
        ));
    }
    if request.repository.provider() != GitProvider::Github {
        return Err(SourceCheckoutError::Invalid(
            "source checkout provider is unsupported".into(),
        ));
    }
    Ok(())
}

fn validate_credential(
    request: &SourceCheckoutRequest,
    credential: Option<&SourceProviderCredential>,
    timeout: Duration,
) -> Result<(), SourceCheckoutError> {
    if credential.is_some_and(|credential| {
        !credential.authorizes(
            &request.repository,
            chrono::Utc::now(),
            chrono::Duration::from_std(timeout).unwrap_or_else(|_| chrono::Duration::minutes(10)),
        )
    }) {
        return Err(SourceCheckoutError::Unavailable(
            "source provider credential is unavailable".into(),
        ));
    }
    Ok(())
}

fn map_git_error(error: GitCommandError, provider_access: bool) -> SourceCheckoutError {
    if provider_access {
        SourceCheckoutError::Unavailable(
            match error {
                GitCommandError::Timeout => "Git provider request exceeded its deadline",
                GitCommandError::OutputLimit => "Git provider response exceeded its bound",
                GitCommandError::ExecutableUnavailable
                | GitCommandError::Spawn
                | GitCommandError::Failed => "Git provider did not supply the accepted commit",
            }
            .into(),
        )
    } else {
        SourceCheckoutError::Integrity(
            match error {
                GitCommandError::Timeout => "local Git validation exceeded its deadline",
                GitCommandError::OutputLimit => "local Git validation output exceeded its bound",
                GitCommandError::ExecutableUnavailable
                | GitCommandError::Spawn
                | GitCommandError::Failed => "local Git validation failed",
            }
            .into(),
        )
    }
}

fn integrity(message: impl Into<String>) -> SourceCheckoutError {
    SourceCheckoutError::Integrity(message.into())
}

fn storage(message: impl Into<String>) -> SourceCheckoutError {
    SourceCheckoutError::Storage(message.into())
}

#[cfg(test)]
#[path = "git_source_checkout_auth_tests.rs"]
mod auth_tests;

#[cfg(test)]
#[path = "git_source_checkout_tests.rs"]
mod tests;
