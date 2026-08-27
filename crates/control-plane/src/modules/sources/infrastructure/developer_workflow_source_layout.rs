use crate::modules::developer_workflows::application::{
    BuildPlanSourceLayoutError, BuildPlanSourceLayoutRequest, IBuildPlanSourceLayoutPort,
};
use crate::modules::developer_workflows::domain::{
    SourceLayoutEntry, SourceLayoutEntryKind, SourceLayoutIdentity, SourceLayoutSnapshot,
    ASSET_ACL_EVIDENCE_PATH, MAX_SOURCE_LAYOUT_CONTENT_BYTES, MAX_SOURCE_LAYOUT_ENTRIES,
    MAX_SOURCE_LAYOUT_INSPECTED_FILE_BYTES,
};
use crate::modules::shared_kernel::domain::Sha256Digest;
use crate::modules::sources::application::{
    IAuthorizedSourceCheckout, ISourceBuildInputQueryPort, SourceBuildInputQueryError,
};
use crate::modules::sources::domain::{
    CheckedOutSource, CheckedOutSourceEntry, CheckedOutSourceEntryKind, SourceCheckoutError,
    SourceCheckoutRequest,
};
use async_trait::async_trait;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

/// Sources-owned anti-corruption adapter for trusted BuildPlan layout input.
///
/// The accepted revision query, provider credential, checkout receipt, local
/// directory, drift replay, and cleanup stay in Sources. Developer Workflows
/// receives only its bounded canonical snapshot value.
pub struct DeveloperWorkflowSourceLayoutAdapter {
    inputs: Arc<dyn ISourceBuildInputQueryPort>,
    checkout: Arc<dyn IAuthorizedSourceCheckout>,
}

impl DeveloperWorkflowSourceLayoutAdapter {
    pub fn new(
        inputs: Arc<dyn ISourceBuildInputQueryPort>,
        checkout: Arc<dyn IAuthorizedSourceCheckout>,
    ) -> Self {
        Self { inputs, checkout }
    }

    async fn snapshot(
        input: &crate::modules::sources::published::SourceBuildInputSnapshot,
        request: &SourceCheckoutRequest,
        source: &CheckedOutSource,
    ) -> Result<SourceLayoutSnapshot, BuildPlanSourceLayoutError> {
        source
            .validate_for(request)
            .map_err(BuildPlanSourceLayoutError::Integrity)?;
        if source.file_count > MAX_SOURCE_LAYOUT_ENTRIES
            || source.content_bytes > MAX_SOURCE_LAYOUT_CONTENT_BYTES
        {
            return Err(BuildPlanSourceLayoutError::Invalid(
                "source layout exceeds the bounded BuildPlan inspection contract".into(),
            ));
        }

        let mut entries = Vec::with_capacity(source.entries.len());
        for entry in &source.entries {
            entries.push(Self::entry(source, entry).await?);
        }
        let identity = SourceLayoutIdentity::new(
            Sha256Digest::parse(
                input
                    .repository()
                    .source_identity_digest(input.commit_sha()),
            )
            .map_err(BuildPlanSourceLayoutError::Integrity)?,
            input.commit_sha().clone(),
            Sha256Digest::parse(source.content_digest.as_str())
                .map_err(BuildPlanSourceLayoutError::Integrity)?,
        )
        .map_err(BuildPlanSourceLayoutError::Integrity)?;
        SourceLayoutSnapshot::new(identity, entries).map_err(BuildPlanSourceLayoutError::Invalid)
    }

    async fn entry(
        source: &CheckedOutSource,
        entry: &CheckedOutSourceEntry,
    ) -> Result<SourceLayoutEntry, BuildPlanSourceLayoutError> {
        entry
            .validate()
            .map_err(BuildPlanSourceLayoutError::Integrity)?;
        let kind = match entry.kind() {
            CheckedOutSourceEntryKind::Regular => SourceLayoutEntryKind::Regular,
            CheckedOutSourceEntryKind::Symlink => SourceLayoutEntryKind::Symlink,
        };
        if kind != SourceLayoutEntryKind::Regular
            || entry.path() != ASSET_ACL_EVIDENCE_PATH
            || entry.size_bytes() > MAX_SOURCE_LAYOUT_INSPECTED_FILE_BYTES as u64
        {
            return SourceLayoutEntry::metadata(
                entry.path(),
                kind,
                entry.size_bytes(),
                entry.content_digest().clone(),
            )
            .map_err(BuildPlanSourceLayoutError::Integrity);
        }

        let path = repository_path(&source.directory, entry.path());
        let metadata = tokio::fs::symlink_metadata(&path)
            .await
            .map_err(|error| evidence_io(error, "inspect"))?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() != entry.size_bytes()
        {
            return Err(BuildPlanSourceLayoutError::Integrity(
                "BuildPlan source evidence changed after checkout".into(),
            ));
        }
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|error| evidence_io(error, "read"))?;
        let mut content = Vec::with_capacity(entry.size_bytes() as usize);
        file.take(MAX_SOURCE_LAYOUT_INSPECTED_FILE_BYTES as u64 + 1)
            .read_to_end(&mut content)
            .await
            .map_err(|_| {
                BuildPlanSourceLayoutError::Storage(
                    "could not read BuildPlan source evidence".into(),
                )
            })?;
        let inspected = SourceLayoutEntry::inspected_regular(entry.path(), content)
            .map_err(BuildPlanSourceLayoutError::Integrity)?;
        if inspected.size_bytes() != entry.size_bytes()
            || inspected.content_digest() != entry.content_digest()
        {
            return Err(BuildPlanSourceLayoutError::Integrity(
                "BuildPlan source evidence differs from the checkout receipt".into(),
            ));
        }
        Ok(inspected)
    }
}

#[async_trait]
impl IBuildPlanSourceLayoutPort for DeveloperWorkflowSourceLayoutAdapter {
    async fn acquire(
        &self,
        request: BuildPlanSourceLayoutRequest,
    ) -> Result<Option<SourceLayoutSnapshot>, BuildPlanSourceLayoutError> {
        request
            .validate()
            .map_err(BuildPlanSourceLayoutError::Invalid)?;
        let input = self
            .inputs
            .find_source_build_input(
                request.organization_id,
                request.project_id,
                request.environment_id,
                request.source_revision_id,
            )
            .await
            .map_err(map_input_error)?;
        let Some(input) = input else {
            return Ok(None);
        };
        if input.organization_id() != request.organization_id
            || input.project_id() != request.project_id
            || input.environment_id() != request.environment_id
            || input.source_revision_id() != request.source_revision_id
        {
            return Err(BuildPlanSourceLayoutError::Conflict);
        }

        let checkout_request = SourceCheckoutRequest::new(
            Uuid::now_v7(),
            input.repository().clone(),
            input.commit_sha().clone(),
        )
        .map_err(BuildPlanSourceLayoutError::Invalid)?;
        let checked_out = self
            .checkout
            .checkout(request.organization_id, &checkout_request)
            .await
            .map_err(map_checkout_error)?;
        let outcome = async {
            let snapshot = Self::snapshot(&input, &checkout_request, &checked_out).await?;
            let replay = self
                .checkout
                .replay(&checkout_request)
                .await
                .map_err(map_checkout_error)?;
            if replay != checked_out {
                return Err(BuildPlanSourceLayoutError::Integrity(
                    "source checkout changed while acquiring the BuildPlan layout".into(),
                ));
            }
            Ok(snapshot)
        }
        .await;
        let cleanup = self
            .checkout
            .remove(checkout_request.checkout_id)
            .await
            .map_err(map_checkout_error);
        if let Err(error) = &cleanup {
            tracing::warn!(
                checkout_id = %checkout_request.checkout_id,
                error = %error,
                "BuildPlan source-layout checkout cleanup failed"
            );
        }
        match (outcome, cleanup) {
            (Ok(snapshot), Ok(())) => Ok(Some(snapshot)),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }
}

fn repository_path(root: &std::path::Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(root.to_path_buf(), |path, segment| path.join(segment))
}

fn evidence_io(error: io::Error, operation: &str) -> BuildPlanSourceLayoutError {
    if error.kind() == io::ErrorKind::NotFound {
        BuildPlanSourceLayoutError::Integrity(
            "BuildPlan source evidence changed after checkout".into(),
        )
    } else {
        BuildPlanSourceLayoutError::Storage(format!(
            "could not {operation} BuildPlan source evidence"
        ))
    }
}

fn map_input_error(error: SourceBuildInputQueryError) -> BuildPlanSourceLayoutError {
    match error {
        SourceBuildInputQueryError::Invalid(message) => {
            BuildPlanSourceLayoutError::Invalid(message)
        }
        SourceBuildInputQueryError::Conflict => BuildPlanSourceLayoutError::Conflict,
        SourceBuildInputQueryError::Integrity(message) => {
            BuildPlanSourceLayoutError::Integrity(message)
        }
        SourceBuildInputQueryError::Storage(message) => {
            BuildPlanSourceLayoutError::Storage(message)
        }
    }
}

fn map_checkout_error(error: SourceCheckoutError) -> BuildPlanSourceLayoutError {
    match error {
        SourceCheckoutError::Invalid(message) => BuildPlanSourceLayoutError::Invalid(message),
        SourceCheckoutError::Conflict => BuildPlanSourceLayoutError::Conflict,
        SourceCheckoutError::Unavailable(message) => {
            BuildPlanSourceLayoutError::Unavailable(message)
        }
        SourceCheckoutError::Integrity(message) => BuildPlanSourceLayoutError::Integrity(message),
        SourceCheckoutError::Storage(message) => BuildPlanSourceLayoutError::Storage(message),
    }
}

#[cfg(test)]
#[path = "developer_workflow_source_layout_tests.rs"]
mod tests;
