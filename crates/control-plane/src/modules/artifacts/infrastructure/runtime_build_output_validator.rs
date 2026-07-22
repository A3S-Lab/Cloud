use super::buildkit_build_service::{validate_exported_output, ValidatedBuildkitOutput};
use crate::modules::artifacts::domain::{
    BuildArtifact, BuildOutputValidationError, BuildServiceError, IBuildOutputValidator,
    INodeArtifactStore, NodeArtifactStoreError, ValidatedOciBuildOutput,
};
use crate::modules::sources::domain::BuildPlatform;
use crate::modules::sources::domain::BuildRecipe;
use a3s_cloud_contracts::{validate_cloud_artifact, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE};
use a3s_runtime::contract::ArtifactRef;
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

const MAX_ARCHIVE_ENTRIES: usize = 2_000_000;
const MAX_OUTPUT_BYTES: u64 = 1024 * 1024 * 1024 * 1024;

pub struct RuntimeBuildOutputValidator {
    artifacts: Arc<dyn INodeArtifactStore>,
    staging_root: PathBuf,
    max_archive_bytes: u64,
    max_entries: usize,
    max_expanded_bytes: u64,
    max_blobs: usize,
    max_oci_bytes: u64,
}

pub(super) struct MaterializedRuntimeBuildOutput {
    staging_directory: PathBuf,
    pub(super) validated: ValidatedBuildkitOutput,
}

impl MaterializedRuntimeBuildOutput {
    pub(super) async fn cleanup(self) {
        let _ = tokio::fs::remove_dir_all(self.staging_directory).await;
    }
}

impl RuntimeBuildOutputValidator {
    pub fn new(
        artifacts: Arc<dyn INodeArtifactStore>,
        staging_root: impl Into<PathBuf>,
        max_archive_bytes: u64,
        max_entries: usize,
        max_expanded_bytes: u64,
        max_blobs: usize,
        max_oci_bytes: u64,
    ) -> Result<Self, String> {
        let staging_root = staging_root.into();
        validate_root(&staging_root)?;
        if max_archive_bytes == 0
            || max_archive_bytes > MAX_OUTPUT_BYTES
            || max_entries == 0
            || max_entries > MAX_ARCHIVE_ENTRIES
            || max_expanded_bytes == 0
            || max_expanded_bytes > MAX_OUTPUT_BYTES
            || max_blobs == 0
            || max_blobs > 1_000_000
            || max_oci_bytes == 0
            || max_oci_bytes > max_expanded_bytes
        {
            return Err("Runtime build output validation limits are invalid".into());
        }
        Ok(Self {
            artifacts,
            staging_root,
            max_archive_bytes,
            max_entries,
            max_expanded_bytes,
            max_blobs,
            max_oci_bytes,
        })
    }

    async fn materialize(
        &self,
        artifact: &BuildArtifact,
        staging: &Path,
    ) -> Result<PathBuf, BuildOutputValidationError> {
        artifact
            .validate()
            .map_err(BuildOutputValidationError::Invalid)?;
        let reference = ArtifactRef {
            uri: artifact.uri.clone(),
            digest: artifact.digest.clone(),
            media_type: artifact.media_type.clone(),
        };
        validate_cloud_artifact(&reference).map_err(BuildOutputValidationError::Invalid)?;
        if reference.media_type != NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE
            || artifact.size_bytes > self.max_archive_bytes
        {
            return Err(BuildOutputValidationError::Invalid(
                "Runtime build output is not a bounded directory Artifact".into(),
            ));
        }
        let mut opened = self
            .artifacts
            .open(&reference)
            .await
            .map_err(map_artifact_error)?;
        if opened.descriptor.size_bytes != artifact.size_bytes {
            return Err(BuildOutputValidationError::Integrity(
                "Runtime build output size changed after admission".into(),
            ));
        }
        let archive = staging.join("output.tar");
        let mut file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&archive)
            .await
            .map_err(storage)?;
        let mut digest = Sha256::new();
        let mut size = 0_u64;
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            let read = opened.reader.read(&mut buffer).await.map_err(storage)?;
            if read == 0 {
                break;
            }
            size = size.checked_add(read as u64).ok_or_else(|| {
                BuildOutputValidationError::Invalid("Runtime build output size overflowed".into())
            })?;
            if size > artifact.size_bytes || size > self.max_archive_bytes {
                return Err(BuildOutputValidationError::Invalid(
                    "Runtime build output exceeds its byte bound".into(),
                ));
            }
            digest.update(&buffer[..read]);
            tokio::io::AsyncWriteExt::write_all(&mut file, &buffer[..read])
                .await
                .map_err(storage)?;
        }
        tokio::io::AsyncWriteExt::flush(&mut file)
            .await
            .map_err(storage)?;
        file.sync_all().await.map_err(storage)?;
        if size != artifact.size_bytes
            || format!("sha256:{:x}", digest.finalize()) != artifact.digest
        {
            return Err(BuildOutputValidationError::Integrity(
                "Runtime build output bytes changed after admission".into(),
            ));
        }
        let extracted = staging.join("extracted");
        tokio::fs::create_dir(&extracted).await.map_err(storage)?;
        let archive_for_task = archive.clone();
        let extracted_for_task = extracted.clone();
        let max_entries = self.max_entries;
        let max_expanded_bytes = self.max_expanded_bytes;
        tokio::task::spawn_blocking(move || {
            extract_archive(
                &archive_for_task,
                &extracted_for_task,
                max_entries,
                max_expanded_bytes,
            )
        })
        .await
        .map_err(|error| {
            BuildOutputValidationError::Storage(format!(
                "Runtime build output extraction task failed: {error}"
            ))
        })??;
        locate_export_root(&extracted)
    }

    async fn materialize_validated(
        &self,
        artifact: &BuildArtifact,
        expected_platforms: &[BuildPlatform],
    ) -> Result<MaterializedRuntimeBuildOutput, BuildOutputValidationError> {
        let root = ensure_staging_root(&self.staging_root).await?;
        let staging = root.join(Uuid::now_v7().to_string());
        tokio::fs::create_dir(&staging).await.map_err(storage)?;
        let result = async {
            let exported = self.materialize(artifact, &staging).await?;
            validate_exported_output(
                &exported,
                expected_platforms,
                self.max_blobs,
                self.max_oci_bytes,
            )
            .await
            .map_err(map_build_error)
        }
        .await;
        match result {
            Ok(validated) => Ok(MaterializedRuntimeBuildOutput {
                staging_directory: staging,
                validated,
            }),
            Err(error) => {
                let _ = tokio::fs::remove_dir_all(&staging).await;
                Err(error)
            }
        }
    }

    pub(super) async fn materialize_validated_output(
        &self,
        expected: &ValidatedOciBuildOutput,
    ) -> Result<MaterializedRuntimeBuildOutput, BuildOutputValidationError> {
        expected
            .validate()
            .map_err(BuildOutputValidationError::Invalid)?;
        let materialized = self
            .materialize_validated(&expected.artifact, &expected.platforms)
            .await?;
        if validated_output(&expected.artifact, &materialized.validated) != *expected {
            materialized.cleanup().await;
            return Err(BuildOutputValidationError::Integrity(
                "Runtime OCI output changed after validation".into(),
            ));
        }
        Ok(materialized)
    }
}

#[async_trait]
impl IBuildOutputValidator for RuntimeBuildOutputValidator {
    async fn validate(
        &self,
        artifact: &BuildArtifact,
        recipe: &BuildRecipe,
    ) -> Result<ValidatedOciBuildOutput, BuildOutputValidationError> {
        let recipe = recipe
            .clone()
            .validate()
            .map_err(BuildOutputValidationError::Invalid)?;
        let materialized = self
            .materialize_validated(artifact, recipe.platforms())
            .await?;
        let output = validated_output(artifact, &materialized.validated);
        materialized.cleanup().await;
        Ok(output)
    }
}

fn validated_output(
    artifact: &BuildArtifact,
    output: &ValidatedBuildkitOutput,
) -> ValidatedOciBuildOutput {
    ValidatedOciBuildOutput {
        artifact: artifact.clone(),
        descriptor: output.descriptor.clone(),
        platforms: output.platforms.clone(),
        content_bytes: output.content_bytes,
        blob_count: output.blob_count,
    }
}

async fn ensure_staging_root(root: &Path) -> Result<PathBuf, BuildOutputValidationError> {
    tokio::fs::create_dir_all(root).await.map_err(storage)?;
    let metadata = tokio::fs::symlink_metadata(root).await.map_err(storage)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(BuildOutputValidationError::Integrity(
            "Runtime build output staging root is not an owned directory".into(),
        ));
    }
    tokio::fs::canonicalize(root).await.map_err(storage)
}

fn extract_archive(
    archive: &Path,
    destination: &Path,
    max_entries: usize,
    max_expanded_bytes: u64,
) -> Result<(), BuildOutputValidationError> {
    let file = std::fs::File::open(archive).map_err(storage)?;
    let mut archive = tar::Archive::new(file);
    let mut paths = BTreeMap::<PathBuf, bool>::new();
    let mut expanded = 0_u64;
    for (index, entry) in archive.entries().map_err(storage)?.enumerate() {
        if index >= max_entries {
            return Err(BuildOutputValidationError::Invalid(
                "Runtime build output exceeds its entry bound".into(),
            ));
        }
        let mut entry = entry.map_err(storage)?;
        let path = normalize_path(&entry.path().map_err(storage)?)?;
        let kind = entry.header().entry_type();
        let is_directory = kind.is_dir();
        if !is_directory && !kind.is_file() {
            return Err(BuildOutputValidationError::Integrity(
                "Runtime build output contains a non-file archive entry".into(),
            ));
        }
        if paths.insert(path.clone(), is_directory).is_some() {
            return Err(BuildOutputValidationError::Integrity(
                "Runtime build output contains duplicate archive paths".into(),
            ));
        }
        let mut ancestor = path.parent();
        while let Some(parent) = ancestor.filter(|parent| !parent.as_os_str().is_empty()) {
            if paths.get(parent) == Some(&false) {
                return Err(BuildOutputValidationError::Integrity(
                    "Runtime build output descends through a regular file".into(),
                ));
            }
            ancestor = parent.parent();
        }
        let target = destination.join(&path);
        if is_directory {
            std::fs::create_dir_all(&target).map_err(storage)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(storage)?;
        }
        let declared = entry.header().size().map_err(storage)?;
        expanded = expanded.checked_add(declared).ok_or_else(|| {
            BuildOutputValidationError::Invalid(
                "Runtime build output expanded size overflowed".into(),
            )
        })?;
        if expanded > max_expanded_bytes {
            return Err(BuildOutputValidationError::Invalid(
                "Runtime build output exceeds its expanded byte bound".into(),
            ));
        }
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&target)
            .map_err(storage)?;
        let copied = io::copy(&mut entry.by_ref().take(declared), &mut output).map_err(storage)?;
        if copied != declared {
            return Err(BuildOutputValidationError::Integrity(
                "Runtime build output entry changed size".into(),
            ));
        }
        output.flush().map_err(storage)?;
        output.sync_all().map_err(storage)?;
    }
    Ok(())
}

fn locate_export_root(extracted: &Path) -> Result<PathBuf, BuildOutputValidationError> {
    if has_exact_export_entries(extracted)? {
        return Ok(extracted.to_owned());
    }
    let entries = std::fs::read_dir(extracted)
        .map_err(storage)?
        .map(|entry| entry.map_err(storage))
        .collect::<Result<Vec<_>, _>>()?;
    if entries.len() != 1 {
        return Err(BuildOutputValidationError::Integrity(
            "Runtime build output has an unexpected archive root".into(),
        ));
    }
    let candidate = entries[0].path();
    let metadata = std::fs::symlink_metadata(&candidate).map_err(storage)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || !has_exact_export_entries(&candidate)?
    {
        return Err(BuildOutputValidationError::Integrity(
            "Runtime build output has an unexpected export structure".into(),
        ));
    }
    Ok(candidate)
}

fn has_exact_export_entries(root: &Path) -> Result<bool, BuildOutputValidationError> {
    let mut names = std::fs::read_dir(root)
        .map_err(storage)?
        .map(|entry| entry.map(|entry| entry.file_name()).map_err(storage))
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    if names
        != [
            std::ffi::OsString::from("buildkit-metadata.json"),
            std::ffi::OsString::from("oci"),
        ]
    {
        return Ok(false);
    }
    let metadata =
        std::fs::symlink_metadata(root.join("buildkit-metadata.json")).map_err(storage)?;
    let layout = std::fs::symlink_metadata(root.join("oci")).map_err(storage)?;
    Ok(metadata.is_file()
        && !metadata.file_type().is_symlink()
        && layout.is_dir()
        && !layout.file_type().is_symlink())
}

fn normalize_path(path: &Path) -> Result<PathBuf, BuildOutputValidationError> {
    let text = path.to_str().ok_or_else(|| {
        BuildOutputValidationError::Integrity("Runtime build output path must be UTF-8".into())
    })?;
    if text.is_empty() || text.len() > 4096 || text.contains(['\0', '\r', '\n']) {
        return Err(BuildOutputValidationError::Integrity(
            "Runtime build output path is invalid".into(),
        ));
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => normalized.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(BuildOutputValidationError::Integrity(
                    "Runtime build output path escapes its extraction root".into(),
                ))
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(BuildOutputValidationError::Integrity(
            "Runtime build output path is empty".into(),
        ));
    }
    Ok(normalized)
}

fn validate_root(path: &Path) -> Result<(), String> {
    let text = path
        .to_str()
        .ok_or_else(|| "Runtime build output staging root must be UTF-8".to_owned())?;
    if text.trim().is_empty()
        || text.len() > 4096
        || text.contains(['\0', '\r', '\n'])
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("Runtime build output staging root is invalid".into());
    }
    Ok(())
}

fn map_artifact_error(error: NodeArtifactStoreError) -> BuildOutputValidationError {
    match error {
        NodeArtifactStoreError::Invalid(message) => BuildOutputValidationError::Invalid(message),
        NodeArtifactStoreError::NotFound => {
            BuildOutputValidationError::Unavailable("Runtime build output is missing".into())
        }
        NodeArtifactStoreError::Conflict => BuildOutputValidationError::Integrity(
            "Runtime build output identity conflicts with stored content".into(),
        ),
        NodeArtifactStoreError::Integrity(message) => {
            BuildOutputValidationError::Integrity(message)
        }
        NodeArtifactStoreError::Storage(message) => BuildOutputValidationError::Storage(message),
    }
}

fn map_build_error(error: BuildServiceError) -> BuildOutputValidationError {
    match error {
        BuildServiceError::Invalid(message) => BuildOutputValidationError::Invalid(message),
        BuildServiceError::Unavailable(message) => BuildOutputValidationError::Unavailable(message),
        BuildServiceError::Storage(message) => BuildOutputValidationError::Storage(message),
        BuildServiceError::Conflict => BuildOutputValidationError::Integrity(
            "Runtime OCI output conflicts with its build identity".into(),
        ),
        BuildServiceError::BuildFailed => BuildOutputValidationError::Integrity(
            "Runtime OCI output contains a failed BuildKit result".into(),
        ),
        BuildServiceError::Integrity(message) => BuildOutputValidationError::Integrity(message),
    }
}

fn storage(error: impl std::fmt::Display) -> BuildOutputValidationError {
    BuildOutputValidationError::Storage(format!(
        "could not materialize Runtime build output: {error}"
    ))
}

#[cfg(test)]
#[path = "runtime_build_output_validator_tests.rs"]
mod tests;
