use crate::modules::artifacts::domain::BuildRun;
use crate::modules::shared_kernel::domain::{
    BuildRunId, EnvironmentId, OrganizationId, ProjectId, RepositoryError, SourceRevisionId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
pub trait IBuildRunRepository: Send + Sync {
    async fn reserve_pending(
        &self,
        limit: usize,
        reserved_at: DateTime<Utc>,
    ) -> Result<Vec<BuildRun>, RepositoryError>;

    async fn pending_operation_starts(
        &self,
        limit: usize,
    ) -> Result<Vec<BuildRun>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        build_run_id: BuildRunId,
    ) -> Result<BuildRun, RepositoryError>;

    async fn find_by_source_revision(
        &self,
        organization_id: OrganizationId,
        source_revision_id: SourceRevisionId,
    ) -> Result<Option<BuildRun>, RepositoryError>;

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<BuildRun>, RepositoryError>;

    async fn save(
        &self,
        build_run: BuildRun,
        expected_version: u64,
    ) -> Result<BuildRun, RepositoryError>;
}

pub(crate) fn validate_build_run_transition(
    existing: &BuildRun,
    next: &BuildRun,
    expected_version: u64,
) -> Result<(), RepositoryError> {
    if existing.aggregate_version != expected_version
        || expected_version
            .checked_add(1)
            .is_none_or(|version| next.aggregate_version != version)
    {
        return Err(transition_conflict());
    }

    let at = next.updated_at;
    let valid = matches_transition(existing, next, |candidate| candidate.begin_preparation(at))
        || next
            .source_content_digest
            .as_ref()
            .zip(next.input_artifact.as_ref())
            .is_some_and(|(digest, artifact)| {
                matches_transition(existing, next, |candidate| {
                    candidate.record_input(digest.clone(), artifact.clone(), at)
                })
            })
        || next
            .node_id
            .zip(next.runtime_spec_digest.as_ref())
            .is_some_and(|(node_id, digest)| {
                matches_transition(existing, next, |candidate| {
                    candidate.schedule(node_id, digest.clone(), at)
                })
            })
        || next.command_id.is_some_and(|command_id| {
            matches_transition(existing, next, |candidate| {
                candidate.dispatch(command_id, at)
            })
        })
        || next
            .runtime_output_artifact
            .as_ref()
            .is_some_and(|artifact| {
                matches_transition(existing, next, |candidate| {
                    candidate.begin_validation(artifact.clone(), at)
                })
            })
        || next.output.as_ref().is_some_and(|output| {
            matches_transition(existing, next, |candidate| {
                candidate.record_validated_output(output.clone(), at)
            })
        })
        || next.failure.as_ref().is_some_and(|failure| {
            matches_transition(existing, next, |candidate| {
                candidate.record_failure(failure.clone(), at)
            })
        })
        || matches_transition(existing, next, |candidate| {
            candidate.request_cancellation(at)
        })
        || next.cleanup_command_id.is_some_and(|command_id| {
            matches_transition(existing, next, |candidate| {
                candidate.begin_cleanup(command_id, at)
            })
        })
        || matches_transition(existing, next, |candidate| candidate.complete(at));

    if valid {
        Ok(())
    } else {
        Err(transition_conflict())
    }
}

fn matches_transition(
    existing: &BuildRun,
    next: &BuildRun,
    mutate: impl FnOnce(&mut BuildRun) -> Result<(), String>,
) -> bool {
    let mut candidate = existing.clone();
    mutate(&mut candidate).is_ok() && candidate == *next
}

fn transition_conflict() -> RepositoryError {
    RepositoryError::Conflict("build run changed while applying its transition".into())
}
