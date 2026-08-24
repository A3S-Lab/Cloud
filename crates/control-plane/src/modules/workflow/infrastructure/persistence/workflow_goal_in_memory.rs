use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, PlanRevisionId, ProjectId,
    RepositoryError, WorkflowGoalId,
};
use crate::modules::workflow::domain::repositories::WorkflowGoalWriteReference;
use crate::modules::workflow::domain::{
    CreateWorkflowGoalWrite, IWorkflowGoalRepository, PlanRevision, WorkflowGoalRecord,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryWorkflowGoalRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    records: BTreeMap<(OrganizationId, WorkflowGoalId), WorkflowGoalRecord>,
    plans: BTreeMap<(OrganizationId, WorkflowGoalId, PlanRevisionId), PlanRevision>,
    idempotency: BTreeMap<(String, String), (String, WorkflowGoalWriteReference)>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryWorkflowGoalRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IWorkflowGoalRepository for InMemoryWorkflowGoalRepository {
    async fn create(
        &self,
        write: CreateWorkflowGoalWrite,
    ) -> Result<IdempotentWrite<WorkflowGoalRecord>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(record) = replay(&state, &write.idempotency)? {
            return Ok(IdempotentWrite {
                value: record,
                replayed: true,
            });
        }
        let key = (
            write.idempotency.storage_key().0.to_owned(),
            write.idempotency.storage_key().1.to_owned(),
        );
        write
            .record
            .goal
            .validate(&write.record.plan_revision)
            .map_err(RepositoryError::Storage)?;
        let record_key = (write.record.goal.organization_id, write.record.goal.id);
        if state.records.contains_key(&record_key) {
            return Err(RepositoryError::Conflict(
                "WorkflowGoal identity already exists".into(),
            ));
        }
        state.plans.insert(
            (
                write.record.goal.organization_id,
                write.record.goal.id,
                write.record.plan_revision.id,
            ),
            write.record.plan_revision.clone(),
        );
        state.records.insert(record_key, write.record.clone());
        state.idempotency.insert(
            key,
            (
                write.idempotency.request_digest.clone(),
                WorkflowGoalWriteReference {
                    organization_id: write.record.goal.organization_id,
                    workflow_goal_id: write.record.goal.id,
                    plan_revision_id: write.record.plan_revision.id,
                },
            ),
        );
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.record,
            replayed: false,
        })
    }

    async fn replay(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<WorkflowGoalRecord>, RepositoryError> {
        let state = self.state.read().await;
        replay(&state, idempotency)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        goal_id: WorkflowGoalId,
    ) -> Result<Option<WorkflowGoalRecord>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .records
            .get(&(organization_id, goal_id))
            .cloned())
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<WorkflowGoalRecord>, RepositoryError> {
        let mut values = self
            .state
            .read()
            .await
            .records
            .values()
            .filter(|value| {
                value.goal.organization_id == organization_id && value.goal.project_id == project_id
            })
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|left, right| {
            right
                .goal
                .created_at
                .cmp(&left.goal.created_at)
                .then_with(|| left.goal.id.cmp(&right.goal.id))
        });
        Ok(values)
    }

    async fn find_plan_revision(
        &self,
        organization_id: OrganizationId,
        goal_id: WorkflowGoalId,
        plan_revision_id: PlanRevisionId,
    ) -> Result<Option<PlanRevision>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .plans
            .get(&(organization_id, goal_id, plan_revision_id))
            .cloned())
    }
}

fn replay(
    state: &State,
    idempotency: &IdempotencyRequest,
) -> Result<Option<WorkflowGoalRecord>, RepositoryError> {
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
    let record = state
        .records
        .get(&(reference.organization_id, reference.workflow_goal_id))
        .cloned()
        .ok_or_else(|| RepositoryError::Storage("WorkflowGoal replay target is missing".into()))?;
    if record.plan_revision.id != reference.plan_revision_id {
        return Err(RepositoryError::Storage(
            "WorkflowGoal replay plan target is invalid".into(),
        ));
    }
    Ok(Some(record))
}
