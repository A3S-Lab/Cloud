use crate::modules::executions::domain::Execution;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, ExecutionId, IdempotencyRequest, OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;

#[derive(Clone)]
pub struct CreateExecution {
    pub execution: Execution,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionWrite {
    pub execution: Execution,
    pub replayed: bool,
}

#[async_trait]
pub trait IExecutionRepository: Send + Sync {
    async fn create(&self, request: CreateExecution) -> Result<ExecutionWrite, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        execution_id: ExecutionId,
    ) -> Result<Option<Execution>, RepositoryError>;

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        limit: usize,
    ) -> Result<Vec<Execution>, RepositoryError>;

    async fn replay(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<Execution>, RepositoryError>;

    async fn request_cancellation(
        &self,
        request: TransitionExecution,
    ) -> Result<ExecutionWrite, RepositoryError>;

    async fn pending_operation_starts(
        &self,
        limit: usize,
    ) -> Result<Vec<Execution>, RepositoryError>;

    async fn save(
        &self,
        execution: Execution,
        expected_version: u64,
    ) -> Result<Execution, RepositoryError>;
}

#[derive(Clone)]
pub struct TransitionExecution {
    pub execution: Execution,
    pub expected_version: u64,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

pub fn validate_execution_transition(
    existing: &Execution,
    next: &Execution,
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
    let valid = next
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
        || matches_transition(existing, next, |candidate| {
            candidate.request_cancellation(at)
        })
        || next.outcome.as_ref().is_some_and(|outcome| {
            matches_transition(existing, next, |candidate| {
                candidate.begin_cleanup(outcome.clone(), at)
            })
        })
        || next.cleanup_command_id.is_some_and(|command_id| {
            matches_transition(existing, next, |candidate| {
                candidate.record_cleanup_command(command_id, at)
            })
        })
        || matches_transition(existing, next, |candidate| candidate.complete_cleanup(at));
    if valid {
        Ok(())
    } else {
        Err(transition_conflict())
    }
}

fn matches_transition(
    existing: &Execution,
    next: &Execution,
    mutate: impl FnOnce(&mut Execution) -> Result<(), String>,
) -> bool {
    let mut candidate = existing.clone();
    mutate(&mut candidate).is_ok() && candidate == *next
}

fn transition_conflict() -> RepositoryError {
    RepositoryError::Conflict("execution changed while applying its transition".into())
}
