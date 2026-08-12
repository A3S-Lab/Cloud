use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId, RepositoryError, WorkflowRunId,
};
use crate::modules::workflow::domain::repositories::WorkflowRunWriteReference;
use crate::modules::workflow::domain::{
    CancelWorkflowRunWrite, CreateWorkflowRunWrite, IWorkflowRunRepository, WorkflowRun,
    WorkflowRunRecord,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryWorkflowRunRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    records: BTreeMap<(OrganizationId, WorkflowRunId), WorkflowRunRecord>,
    idempotency: BTreeMap<(String, String), (String, WorkflowRunWriteReference)>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryWorkflowRunRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IWorkflowRunRepository for InMemoryWorkflowRunRepository {
    async fn create(
        &self,
        write: CreateWorkflowRunWrite,
    ) -> Result<IdempotentWrite<WorkflowRunRecord>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(record) = replay(&state, &write.idempotency)? {
            return Ok(IdempotentWrite {
                value: record,
                replayed: true,
            });
        }
        write.record.validate().map_err(RepositoryError::Storage)?;
        let key = (write.record.run.organization_id, write.record.run.id);
        if state.records.contains_key(&key) {
            return Err(RepositoryError::Conflict(
                "WorkflowRun identity already exists".into(),
            ));
        }
        state.records.insert(key, write.record.clone());
        store_replay(&mut state, &write.idempotency, &write.record);
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.record,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        workflow_run_id: WorkflowRunId,
    ) -> Result<Option<WorkflowRunRecord>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .records
            .get(&(organization_id, workflow_run_id))
            .cloned())
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        limit: usize,
    ) -> Result<Vec<WorkflowRunRecord>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut values = self
            .state
            .read()
            .await
            .records
            .values()
            .filter(|record| {
                record.run.organization_id == organization_id && record.run.project_id == project_id
            })
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|left, right| {
            right
                .run
                .requested_at
                .cmp(&left.run.requested_at)
                .then_with(|| right.run.id.cmp(&left.run.id))
        });
        values.truncate(limit);
        Ok(values)
    }

    async fn request_cancellation(
        &self,
        write: CancelWorkflowRunWrite,
    ) -> Result<IdempotentWrite<WorkflowRunRecord>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(record) = replay(&state, &write.idempotency)? {
            return Ok(IdempotentWrite {
                value: record,
                replayed: true,
            });
        }
        write.record.validate().map_err(RepositoryError::Storage)?;
        let key = (write.record.run.organization_id, write.record.run.id);
        let existing = state
            .records
            .get(&key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        validate_cancellation_transition(
            &existing,
            &write.record,
            write.expected_version,
            write.actor_principal_id,
        )?;
        state.records.insert(key, write.record.clone());
        store_replay(&mut state, &write.idempotency, &write.record);
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.record,
            replayed: false,
        })
    }

    async fn replay(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<WorkflowRunRecord>, RepositoryError> {
        let state = self.state.read().await;
        replay(&state, idempotency)
    }

    async fn pending_reconciliation(
        &self,
        limit: usize,
    ) -> Result<Vec<WorkflowRunRecord>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut values = self
            .state
            .read()
            .await
            .records
            .values()
            .filter(|record| !record.run.status.is_terminal())
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|left, right| {
            left.run
                .requested_at
                .cmp(&right.run.requested_at)
                .then_with(|| left.run.id.cmp(&right.run.id))
        });
        values.truncate(limit);
        Ok(values)
    }

    async fn save_projection(
        &self,
        record: WorkflowRunRecord,
        expected_version: u64,
    ) -> Result<WorkflowRunRecord, RepositoryError> {
        record.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let key = (record.run.organization_id, record.run.id);
        let existing = state
            .records
            .get(&key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        validate_projection_transition(&existing, &record, expected_version)?;
        state.records.insert(key, record.clone());
        Ok(record)
    }
}

fn replay(
    state: &State,
    idempotency: &IdempotencyRequest,
) -> Result<Option<WorkflowRunRecord>, RepositoryError> {
    let key = (
        idempotency.storage_key().0.to_owned(),
        idempotency.storage_key().1.to_owned(),
    );
    let Some((digest, reference)) = state.idempotency.get(&key) else {
        return Ok(None);
    };
    if digest != &idempotency.request_digest {
        return Err(RepositoryError::IdempotencyConflict);
    }
    state
        .records
        .get(&(reference.organization_id, reference.workflow_run_id))
        .cloned()
        .map(Some)
        .ok_or_else(|| RepositoryError::Storage("WorkflowRun replay target is missing".into()))
}

fn store_replay(state: &mut State, idempotency: &IdempotencyRequest, record: &WorkflowRunRecord) {
    state.idempotency.insert(
        (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        ),
        (
            idempotency.request_digest.clone(),
            WorkflowRunWriteReference {
                organization_id: record.run.organization_id,
                workflow_run_id: record.run.id,
            },
        ),
    );
}

fn validate_cancellation_transition(
    existing: &WorkflowRunRecord,
    next: &WorkflowRunRecord,
    expected_version: u64,
    actor_principal_id: crate::modules::shared_kernel::domain::PrincipalId,
) -> Result<(), RepositoryError> {
    if existing.run.aggregate_version != expected_version || existing.steps != next.steps {
        return Err(RepositoryError::Conflict(
            "WorkflowRun changed while cancellation was requested".into(),
        ));
    }
    let mut candidate = existing.run.clone();
    let requested_at = next.run.cancellation_requested_at.ok_or_else(|| {
        RepositoryError::Storage("WorkflowRun cancellation is missing its request time".into())
    })?;
    candidate
        .request_cancellation(
            next.run.cancellation_reason.clone(),
            actor_principal_id,
            requested_at,
        )
        .map_err(RepositoryError::Storage)?;
    if candidate != next.run {
        return Err(RepositoryError::Conflict(
            "WorkflowRun cancellation transition drifted".into(),
        ));
    }
    Ok(())
}

fn validate_projection_transition(
    existing: &WorkflowRunRecord,
    next: &WorkflowRunRecord,
    expected_version: u64,
) -> Result<(), RepositoryError> {
    if existing.run.aggregate_version != expected_version
        || next.run.aggregate_version
            != expected_version.checked_add(1).ok_or_else(|| {
                RepositoryError::Storage("WorkflowRun aggregate version overflowed".into())
            })?
        || next.run.last_flow_sequence <= existing.run.last_flow_sequence
        || !same_run_authority(&existing.run, &next.run)
        || existing.steps.len() != next.steps.len()
    {
        return Err(RepositoryError::Conflict(
            "WorkflowRun projection transition conflicts with stored state".into(),
        ));
    }
    for current in &existing.steps {
        let projected = next
            .steps
            .iter()
            .find(|step| step.step_id == current.step_id)
            .ok_or_else(|| RepositoryError::Storage("WorkflowRun projection lost a step".into()))?;
        if current.organization_id != projected.organization_id
            || current.project_id != projected.project_id
            || current.workflow_run_id != projected.workflow_run_id
            || current.kind != projected.kind
            || current.flow_step_id != projected.flow_step_id
            || projected.last_flow_sequence < current.last_flow_sequence
            || (current.status.is_terminal() && current != projected)
        {
            return Err(RepositoryError::Conflict(format!(
                "Workflow step {:?} projection transition conflicts with stored state",
                current.step_id
            )));
        }
    }
    Ok(())
}

fn same_run_authority(left: &WorkflowRun, right: &WorkflowRun) -> bool {
    left.organization_id == right.organization_id
        && left.project_id == right.project_id
        && left.id == right.id
        && left.workflow_goal_id == right.workflow_goal_id
        && left.plan_revision_id == right.plan_revision_id
        && left.plan_digest == right.plan_digest
        && left.operation_id == right.operation_id
        && left.flow_run_id == right.flow_run_id
        && left.execution_input == right.execution_input
        && left.execution_input_digest == right.execution_input_digest
        && left.requested_by == right.requested_by
        && left.requested_at == right.requested_at
        && left.cancellation_requested_at == right.cancellation_requested_at
        && left.cancellation_requested_by == right.cancellation_requested_by
        && left.cancellation_reason == right.cancellation_reason
}
