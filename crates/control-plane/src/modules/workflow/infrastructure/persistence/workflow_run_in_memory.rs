use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OrganizationId, ProjectId, RepositoryError, WorkflowRunId,
};
use crate::modules::workflow::application::workflow_run_operation;
use crate::modules::workflow::domain::repositories::WorkflowRunWriteReference;
use crate::modules::workflow::domain::{
    IWorkflowRunRepository, StartWorkflowRunWrite, WorkflowRun,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct InMemoryWorkflowRunRepository {
    operations: Arc<dyn IOperationRepository>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    runs: BTreeMap<(OrganizationId, WorkflowRunId), WorkflowRun>,
    idempotency: BTreeMap<(String, String), (String, WorkflowRunWriteReference)>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryWorkflowRunRepository {
    pub fn new(operations: Arc<dyn IOperationRepository>) -> Self {
        Self {
            operations,
            state: Mutex::new(State::default()),
        }
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.lock().await.outbox.clone()
    }
}

#[async_trait]
impl IWorkflowRunRepository for InMemoryWorkflowRunRepository {
    async fn start(
        &self,
        write: StartWorkflowRunWrite,
    ) -> Result<IdempotentWrite<WorkflowRun>, RepositoryError> {
        let mut state = self.state.lock().await;
        let key = (
            write.idempotency.storage_key().0.to_owned(),
            write.idempotency.storage_key().1.to_owned(),
        );
        if let Some((digest, reference)) = state.idempotency.get(&key) {
            if digest != &write.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            let run = state
                .runs
                .get(&(reference.organization_id, reference.workflow_run_id))
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage("WorkflowRun replay target is missing".into())
                })?;
            return Ok(IdempotentWrite {
                value: run,
                replayed: true,
            });
        }
        write
            .run
            .validate_identity()
            .map_err(RepositoryError::Storage)?;
        let expected_operation =
            workflow_run_operation(&write.run).map_err(RepositoryError::Storage)?;
        if !write.operation.has_same_definition(&expected_operation) {
            return Err(RepositoryError::Storage(
                "WorkflowRun write contains another Operation request".into(),
            ));
        }
        let run_key = (write.run.organization_id, write.run.id);
        if state.runs.contains_key(&run_key) {
            return Err(RepositoryError::Conflict(
                "WorkflowRun identity already exists".into(),
            ));
        }
        self.operations.enqueue(write.operation).await?;
        state.runs.insert(run_key, write.run.clone());
        state.idempotency.insert(
            key,
            (
                write.idempotency.request_digest.clone(),
                WorkflowRunWriteReference {
                    organization_id: write.run.organization_id,
                    workflow_run_id: write.run.id,
                },
            ),
        );
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.run,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        workflow_run_id: WorkflowRunId,
    ) -> Result<Option<WorkflowRun>, RepositoryError> {
        Ok(self
            .state
            .lock()
            .await
            .runs
            .get(&(organization_id, workflow_run_id))
            .cloned())
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<WorkflowRun>, RepositoryError> {
        let mut values = self
            .state
            .lock()
            .await
            .runs
            .values()
            .filter(|run| run.organization_id == organization_id && run.project_id == project_id)
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|left, right| {
            right
                .requested_at
                .cmp(&left.requested_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(values)
    }
}
