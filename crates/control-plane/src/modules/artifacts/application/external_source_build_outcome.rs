use crate::modules::artifacts::domain::{
    BuildRun, BuildRunStatus, BuildSubject, IBuildRunRepository,
};
use crate::modules::artifacts::published::{
    ExternalSourceBuildOutcome, ValidatedExternalSourceBuildOutcomeProjection,
};
use crate::modules::shared_kernel::domain::{BuildRunId, OrganizationId, RepositoryError};
use async_trait::async_trait;
use std::sync::Arc;

/// Project the terminal Artifacts fact available to external-source build
/// consumers. Non-external or non-successful BuildRuns publish no outcome.
pub(crate) fn project_external_source_build_outcome(
    build: &BuildRun,
) -> Result<Option<ExternalSourceBuildOutcome>, String> {
    build.validate()?;
    let BuildSubject::ExternalSourceRevision {
        project_id,
        environment_id,
        source_revision_id,
    } = build.subject
    else {
        return Ok(None);
    };
    if build.status != BuildRunStatus::Succeeded {
        return Ok(None);
    }
    let evidence = build
        .evidence
        .as_deref()
        .ok_or_else(|| "successful external BuildRun omitted verified evidence".to_owned())?;
    let artifact = build
        .published_artifact
        .as_ref()
        .ok_or_else(|| "successful external BuildRun omitted its OCI publication".to_owned())?;
    let completed_at = build
        .finished_at
        .ok_or_else(|| "successful external BuildRun omitted its finish time".to_owned())?;

    ExternalSourceBuildOutcome::from_validated_build(
        ValidatedExternalSourceBuildOutcomeProjection {
            organization_id: build.organization_id,
            project_id,
            environment_id,
            source_revision_id,
            build_run_id: build.id,
            build_run_version: build.aggregate_version,
            attempt: build.attempt,
            operation_id: build.operation_id,
            commit_sha: evidence.commit_sha.clone(),
            source_content_digest: evidence.source_content_digest.clone(),
            recipe: evidence.recipe.clone(),
            artifact_uri: artifact.uri.clone(),
            artifact_digest: artifact.digest.clone(),
            artifact_media_type: artifact.media_type.clone(),
            artifact_size_bytes: artifact.size_bytes,
            provenance_digest: evidence.provenance_digest.clone(),
            requested_at: build.requested_at,
            attested_at: evidence.attested_at,
            completed_at,
        },
    )
    .map(Some)
}

#[async_trait]
pub trait IExternalSourceBuildOutcomeQueryPort: Send + Sync {
    async fn find_external_source_build_outcome(
        &self,
        organization_id: OrganizationId,
        build_run_id: BuildRunId,
    ) -> Result<Option<ExternalSourceBuildOutcome>, RepositoryError>;
}

/// Owner-side query service that keeps BuildRun aggregate loading and terminal
/// state interpretation inside Artifacts.
pub struct ExternalSourceBuildOutcomeQueryService {
    builds: Arc<dyn IBuildRunRepository>,
}

impl ExternalSourceBuildOutcomeQueryService {
    pub fn new(builds: Arc<dyn IBuildRunRepository>) -> Self {
        Self { builds }
    }
}

#[async_trait]
impl IExternalSourceBuildOutcomeQueryPort for ExternalSourceBuildOutcomeQueryService {
    async fn find_external_source_build_outcome(
        &self,
        organization_id: OrganizationId,
        build_run_id: BuildRunId,
    ) -> Result<Option<ExternalSourceBuildOutcome>, RepositoryError> {
        let build = match self.builds.find(organization_id, build_run_id).await {
            Ok(build) => build,
            Err(RepositoryError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        project_external_source_build_outcome(&build).map_err(|error| {
            RepositoryError::Storage(format!(
                "invalid external source build outcome projection: {error}"
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::artifacts::domain::test_support::{
        succeeded_external_build_with_output, succeeded_hosted_build, typed_build_output,
    };
    use crate::modules::artifacts::domain::BuildRun;
    use crate::modules::artifacts::infrastructure::InMemoryBuildRunRepository;
    use crate::modules::shared_kernel::domain::{
        AssetId, AssetReleaseId, EnvironmentId, ProjectId, SourceRevisionId,
    };
    use a3s_cloud_contracts::DURABLE_CELL_BUNDLE_MEDIA_TYPE;
    use chrono::Utc;

    #[tokio::test]
    async fn query_projects_only_verified_successful_external_builds() {
        let repository = Arc::new(InMemoryBuildRunRepository::new());
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let source_revision_id = SourceRevisionId::new();
        let requested_at = Utc::now();
        let external = succeeded_external_build_with_output(
            organization_id,
            project_id,
            environment_id,
            source_revision_id,
            typed_build_output(
                &format!("sha256:{}", "d".repeat(64)),
                DURABLE_CELL_BUNDLE_MEDIA_TYPE,
                512,
            ),
            requested_at,
        );
        let hosted = succeeded_hosted_build(
            organization_id,
            AssetId::new(),
            AssetReleaseId::new(),
            requested_at,
        );
        let queued = BuildRun::reserve(
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            SourceRevisionId::new(),
            requested_at,
        );
        repository.seed_build(external.clone()).await;
        repository.seed_build(hosted.clone()).await;
        repository.seed_build(queued.clone()).await;
        let service = ExternalSourceBuildOutcomeQueryService::new(repository);

        let outcome = service
            .find_external_source_build_outcome(organization_id, external.id)
            .await
            .expect("external outcome query")
            .expect("successful external outcome");
        outcome.validate().expect("valid owner fact");
        assert_eq!(outcome.organization_id(), organization_id);
        assert_eq!(outcome.project_id(), project_id);
        assert_eq!(outcome.environment_id(), environment_id);
        assert_eq!(outcome.source_revision_id(), source_revision_id);
        assert_eq!(outcome.build_run_id(), external.id);
        assert_eq!(outcome.operation_id(), external.operation_id);
        assert_eq!(outcome.build_run_version(), external.aggregate_version);
        assert_eq!(outcome.attempt(), external.attempt);
        assert_eq!(outcome.requested_at(), external.requested_at);
        assert_eq!(
            outcome.completed_at(),
            external.finished_at.expect("finish")
        );
        let encoded = serde_json::to_string(&outcome).expect("owner fact JSON");
        for forbidden in ["nodeId", "commandId", "cleanupCommandId", "failure"] {
            assert!(!encoded.contains(forbidden));
        }
        let mut forged = serde_json::to_value(&outcome).expect("owner fact value");
        forged["commitSha"] = serde_json::Value::String("0".repeat(40));
        let forged: ExternalSourceBuildOutcome =
            serde_json::from_value(forged).expect("syntactically valid forged owner fact");
        assert!(forged.validate().is_err());

        assert_eq!(
            service
                .find_external_source_build_outcome(organization_id, hosted.id)
                .await
                .expect("hosted outcome query"),
            None
        );
        assert_eq!(
            service
                .find_external_source_build_outcome(organization_id, queued.id)
                .await
                .expect("queued outcome query"),
            None
        );
    }
}
