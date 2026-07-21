mod command;
mod connection;
mod metadata;
mod oci_layout;
mod receipt;

use self::command::{BuildkitCommand, BuildkitCommandError, BuildkitCommandInput};
use self::metadata::read_buildkit_descriptor;
use self::oci_layout::{normalize_buildctl_layout, validate_oci_layout, OciLayoutLimits};
use self::receipt::{read_receipt, write_receipt, BuildReceipt, RECEIPT_SCHEMA};
use crate::modules::artifacts::domain::{
    BuildServiceError, BuiltOciArtifact, IBuildService, OciBuildRequest, OciDescriptor,
};
use crate::modules::sources::domain::BuildPlatform;
use async_trait::async_trait;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

pub(super) use self::oci_layout::OciLayoutBlob;
pub use connection::BuildkitConnection;

pub(super) struct ValidatedBuildkitOutput {
    pub descriptor: OciDescriptor,
    pub platforms: Vec<BuildPlatform>,
    pub content_bytes: u64,
    pub blob_count: usize,
    pub layout_directory: PathBuf,
    pub blobs: Vec<OciLayoutBlob>,
}

pub(super) async fn validate_exported_output(
    root: &Path,
    expected_platforms: &[BuildPlatform],
    max_blobs: usize,
    max_bytes: u64,
) -> Result<ValidatedBuildkitOutput, BuildServiceError> {
    let metadata = root.join("buildkit-metadata.json");
    let layout = root.join("oci");
    let descriptor = read_buildkit_descriptor(&metadata).await?;
    normalize_buildctl_layout(&layout).await?;
    let validated = validate_oci_layout(
        &layout,
        &descriptor,
        expected_platforms,
        OciLayoutLimits::new(max_blobs, max_bytes).map_err(BuildServiceError::Invalid)?,
    )
    .await?;
    Ok(ValidatedBuildkitOutput {
        descriptor,
        platforms: validated.platforms,
        content_bytes: validated.content_bytes,
        blob_count: validated.blob_count,
        layout_directory: layout,
        blobs: validated.blobs,
    })
}

pub struct BuildkitBuildService {
    root: PathBuf,
    timeout: Duration,
    limits: OciLayoutLimits,
    command: BuildkitCommand,
}

impl BuildkitBuildService {
    pub fn new(
        buildctl: impl Into<PathBuf>,
        connection: BuildkitConnection,
        root: impl Into<PathBuf>,
        timeout: Duration,
        max_blobs: usize,
        max_bytes: u64,
    ) -> Result<Self, String> {
        if timeout.is_zero() || timeout > Duration::from_secs(2 * 60 * 60) {
            return Err("BuildKit build timeout is invalid".into());
        }
        let root = root.into();
        validate_root_path(&root)?;
        let limits = OciLayoutLimits::new(max_blobs, max_bytes)?;
        let command = BuildkitCommand::new(buildctl, connection)
            .map_err(|_| "BuildKit client executable is unavailable".to_owned())?;
        Ok(Self {
            root,
            timeout,
            limits,
            command,
        })
    }

    async fn prepare(
        &self,
        request: &OciBuildRequest,
        recipe_digest: &str,
        staging: &Path,
    ) -> Result<(), BuildServiceError> {
        let (source, context) = resolve_source_paths(request).await?;
        let layout = staging.join("oci");
        let metadata = staging.join("buildkit-metadata.json");
        let home = staging.join("home");
        tokio::fs::create_dir(&home)
            .await
            .map_err(|_| storage("could not create isolated BuildKit client home"))?;
        self.command
            .run(BuildkitCommandInput {
                source: &source,
                context: &context,
                recipe: request.recipe(),
                layout: &layout,
                metadata: &metadata,
                home: &home,
            })
            .await
            .map_err(map_command_error)?;
        normalize_buildctl_layout(&layout).await?;
        let descriptor = read_buildkit_descriptor(&metadata).await?;
        let validated = validate_oci_layout(
            &layout,
            &descriptor,
            request.recipe().platforms(),
            self.limits,
        )
        .await?;
        tokio::fs::remove_file(&metadata)
            .await
            .map_err(|_| storage("could not remove raw BuildKit metadata"))?;
        tokio::fs::remove_dir_all(&home)
            .await
            .map_err(|_| storage("could not remove isolated BuildKit client home"))?;
        write_receipt(
            staging,
            &BuildReceipt {
                schema: RECEIPT_SCHEMA.into(),
                build_id: request.build_id(),
                source_content_digest: request.source_content_digest().into(),
                recipe_digest: recipe_digest.into(),
                descriptor,
                platforms: validated.platforms,
                content_bytes: validated.content_bytes,
                blob_count: validated.blob_count,
            },
        )
        .await
    }

    async fn existing(
        &self,
        request: &OciBuildRequest,
        recipe_digest: &str,
        output: &Path,
    ) -> Result<BuiltOciArtifact, BuildServiceError> {
        require_owned_directory(output, "OCI build output directory").await?;
        let receipt = read_receipt(output).await?;
        if !receipt.matches(request, recipe_digest) {
            return Err(BuildServiceError::Conflict);
        }
        validate_receipt(&receipt)?;
        let layout = output.join("oci");
        let validated = validate_oci_layout(
            &layout,
            &receipt.descriptor,
            &receipt.platforms,
            self.limits,
        )
        .await?;
        if validated.platforms != receipt.platforms
            || validated.content_bytes != receipt.content_bytes
            || validated.blob_count != receipt.blob_count
        {
            return Err(integrity(
                "OCI build output no longer matches its immutable receipt",
            ));
        }
        Ok(receipt.built_artifact(layout))
    }

    async fn replay(
        &self,
        request: &OciBuildRequest,
        recipe_digest: &str,
        output: &Path,
    ) -> Result<BuiltOciArtifact, BuildServiceError> {
        tokio::time::timeout(self.timeout, self.existing(request, recipe_digest, output))
            .await
            .map_err(|_| {
                BuildServiceError::Unavailable("OCI build replay exceeded its deadline".into())
            })?
    }
}

#[async_trait]
impl IBuildService for BuildkitBuildService {
    async fn build(
        &self,
        request: &OciBuildRequest,
    ) -> Result<BuiltOciArtifact, BuildServiceError> {
        let recipe_digest = request
            .recipe_digest()
            .map_err(BuildServiceError::Invalid)?;
        let root = ensure_root(&self.root).await?;
        let output = root.join(request.build_id().to_string());
        match tokio::fs::symlink_metadata(&output).await {
            Ok(_) => return self.replay(request, &recipe_digest, &output).await,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(storage("could not inspect OCI build output path")),
        }
        let staging = root.join(format!(".{}-{}.tmp", request.build_id(), Uuid::now_v7()));
        tokio::fs::create_dir(&staging)
            .await
            .map_err(|_| storage("could not create OCI build staging directory"))?;
        let prepared = tokio::time::timeout(
            self.timeout,
            self.prepare(request, &recipe_digest, &staging),
        )
        .await;
        match prepared {
            Err(_) => {
                remove_staging(&staging).await;
                return Err(BuildServiceError::Unavailable(
                    "BuildKit build exceeded its deadline".into(),
                ));
            }
            Ok(Err(error)) => {
                remove_staging(&staging).await;
                return Err(error);
            }
            Ok(Ok(())) => {}
        }
        match tokio::fs::rename(&staging, &output).await {
            Ok(()) => self.replay(request, &recipe_digest, &output).await,
            Err(_) if tokio::fs::symlink_metadata(&output).await.is_ok() => {
                remove_staging(&staging).await;
                self.replay(request, &recipe_digest, &output).await
            }
            Err(_) => {
                remove_staging(&staging).await;
                Err(storage("could not commit OCI build output"))
            }
        }
    }

    async fn remove(&self, build_id: Uuid) -> Result<(), BuildServiceError> {
        if build_id.is_nil() {
            return Err(BuildServiceError::Invalid(
                "OCI build ID cannot be nil".into(),
            ));
        }
        let root_metadata = match tokio::fs::symlink_metadata(&self.root).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(storage("could not inspect OCI build output root")),
        };
        if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
            return Err(integrity("OCI build output root is not an owned directory"));
        }
        let root = tokio::fs::canonicalize(&self.root)
            .await
            .map_err(|_| storage("could not canonicalize OCI build output root"))?;
        let output = root.join(build_id.to_string());
        match tokio::fs::symlink_metadata(&output).await {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(storage("could not inspect OCI build output path")),
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                Err(integrity("OCI build output path is not an owned directory"))
            }
            Ok(_) => tokio::fs::remove_dir_all(output)
                .await
                .map_err(|_| storage("could not remove OCI build output")),
        }
    }
}

async fn resolve_source_paths(
    request: &OciBuildRequest,
) -> Result<(PathBuf, PathBuf), BuildServiceError> {
    require_owned_directory(request.source_directory(), "OCI build source directory").await?;
    let source = tokio::fs::canonicalize(request.source_directory())
        .await
        .map_err(|_| storage("could not canonicalize OCI build source directory"))?;
    let context = if request.recipe().context_path() == "." {
        source.clone()
    } else {
        source.join(request.recipe().context_path())
    };
    require_owned_directory(&context, "OCI build context directory").await?;
    let context = tokio::fs::canonicalize(context)
        .await
        .map_err(|_| storage("could not canonicalize OCI build context directory"))?;
    if !context.starts_with(&source) {
        return Err(integrity("OCI build context escapes the source directory"));
    }
    let dockerfile = source.join(request.recipe().dockerfile_path());
    let dockerfile_metadata = tokio::fs::symlink_metadata(&dockerfile)
        .await
        .map_err(|_| BuildServiceError::Invalid("Dockerfile is unavailable".into()))?;
    if dockerfile_metadata.file_type().is_symlink() || !dockerfile_metadata.is_file() {
        return Err(BuildServiceError::Invalid(
            "Dockerfile must be an owned regular file".into(),
        ));
    }
    let dockerfile = tokio::fs::canonicalize(dockerfile)
        .await
        .map_err(|_| storage("could not canonicalize Dockerfile"))?;
    if !dockerfile.starts_with(&source) {
        return Err(integrity("Dockerfile escapes the source directory"));
    }
    Ok((source, context))
}

async fn ensure_root(root: &Path) -> Result<PathBuf, BuildServiceError> {
    tokio::fs::create_dir_all(root)
        .await
        .map_err(|_| storage("could not create OCI build output root"))?;
    require_owned_directory(root, "OCI build output root").await?;
    tokio::fs::canonicalize(root)
        .await
        .map_err(|_| storage("could not canonicalize OCI build output root"))
}

async fn require_owned_directory(path: &Path, label: &str) -> Result<(), BuildServiceError> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|_| BuildServiceError::Invalid(format!("{label} is unavailable")))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(BuildServiceError::Invalid(format!(
            "{label} is not an owned directory"
        )));
    }
    Ok(())
}

fn validate_receipt(receipt: &BuildReceipt) -> Result<(), BuildServiceError> {
    let descriptor = OciDescriptor::new(
        receipt.descriptor.media_type(),
        receipt.descriptor.digest(),
        receipt.descriptor.size(),
    )
    .map_err(integrity)?;
    let mut platforms = BTreeSet::new();
    for platform in &receipt.platforms {
        let validated = BuildPlatform::parse(platform.as_str()).map_err(integrity)?;
        if &validated != platform || !platforms.insert(validated) {
            return Err(integrity("OCI build receipt platforms are invalid"));
        }
    }
    if receipt.schema != RECEIPT_SCHEMA
        || descriptor != receipt.descriptor
        || platforms.is_empty()
        || receipt.content_bytes == 0
        || receipt.blob_count == 0
    {
        return Err(integrity("OCI build receipt is invalid"));
    }
    Ok(())
}

fn validate_root_path(root: &Path) -> Result<(), String> {
    let value = root
        .to_str()
        .ok_or_else(|| "OCI build output root must be UTF-8".to_owned())?;
    if !root.is_absolute()
        || value.is_empty()
        || value.len() > 4096
        || value.contains([',', '\0', '\r', '\n'])
    {
        return Err("OCI build output root must be a bounded absolute path without commas".into());
    }
    Ok(())
}

fn map_command_error(error: BuildkitCommandError) -> BuildServiceError {
    match error {
        BuildkitCommandError::ExecutableUnavailable | BuildkitCommandError::Spawn => {
            BuildServiceError::Unavailable("BuildKit client could not be started".into())
        }
        BuildkitCommandError::Failed => BuildServiceError::BuildFailed,
    }
}

async fn remove_staging(path: &Path) {
    let _ = tokio::fs::remove_dir_all(path).await;
}

fn integrity(message: impl Into<String>) -> BuildServiceError {
    BuildServiceError::Integrity(message.into())
}

fn storage(message: impl Into<String>) -> BuildServiceError {
    BuildServiceError::Storage(message.into())
}

#[cfg(test)]
#[path = "buildkit_build_service_tests.rs"]
mod tests;
