use crate::modules::executions::domain::{
    validate_execution_transition, CreateExecution, Execution, ExecutionWrite,
    IExecutionRepository, TransitionExecution,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, ExecutionId, OrganizationId, ProjectId, RepositoryError, WorkflowRunId,
};
use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryExecutionRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    executions: BTreeMap<(OrganizationId, ExecutionId), Execution>,
    workflow_steps: BTreeMap<(OrganizationId, WorkflowRunId, String, u64), ExecutionId>,
    idempotency: BTreeMap<(String, String), (String, Execution)>,
    operation_starts: BTreeSet<ExecutionId>,
    outbox: Vec<a3s_cloud_contracts::DomainEventEnvelope>,
}

impl InMemoryExecutionRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn mark_operation_started(&self, execution_id: ExecutionId) {
        self.state
            .write()
            .await
            .operation_starts
            .insert(execution_id);
    }

    pub async fn outbox_events(&self) -> Vec<a3s_cloud_contracts::DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IExecutionRepository for InMemoryExecutionRepository {
    async fn create(&self, request: CreateExecution) -> Result<ExecutionWrite, RepositoryError> {
        request
            .execution
            .validate()
            .map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let key = (
            request.idempotency.storage_key().0.to_owned(),
            request.idempotency.storage_key().1.to_owned(),
        );
        if let Some((digest, execution)) = state.idempotency.get(&key) {
            if digest != &request.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(ExecutionWrite {
                execution: execution.clone(),
                replayed: true,
            });
        }
        let identity = (request.execution.organization_id, request.execution.id);
        if state.executions.contains_key(&identity) {
            return Err(RepositoryError::Conflict(
                "execution identity is already in use".into(),
            ));
        }
        let workflow_step = request.execution.workflow.as_ref().map(|binding| {
            (
                request.execution.organization_id,
                binding.workflow_run_id,
                binding.step_id.clone(),
                binding.step_attempt,
            )
        });
        if workflow_step
            .as_ref()
            .is_some_and(|key| state.workflow_steps.contains_key(key))
        {
            return Err(RepositoryError::Conflict(
                "Workflow step attempt is already bound to an execution".into(),
            ));
        }
        state.executions.insert(identity, request.execution.clone());
        if let Some(workflow_step) = workflow_step {
            state
                .workflow_steps
                .insert(workflow_step, request.execution.id);
        }
        state.idempotency.insert(
            key,
            (
                request.idempotency.request_digest,
                request.execution.clone(),
            ),
        );
        state.outbox.push(request.event);
        Ok(ExecutionWrite {
            execution: request.execution,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        execution_id: ExecutionId,
    ) -> Result<Option<Execution>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .executions
            .get(&(organization_id, execution_id))
            .cloned())
    }

    async fn find_for_workflow(
        &self,
        organization_id: OrganizationId,
        workflow_run_id: WorkflowRunId,
        step_id: &str,
        step_attempt: u64,
    ) -> Result<Option<Execution>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .executions
            .values()
            .find(|execution| {
                execution.organization_id == organization_id
                    && execution.workflow.as_ref().is_some_and(|binding| {
                        binding.workflow_run_id == workflow_run_id
                            && binding.step_id == step_id
                            && binding.step_attempt == step_attempt
                    })
            })
            .cloned())
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        limit: usize,
    ) -> Result<Vec<Execution>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut executions = self
            .state
            .read()
            .await
            .executions
            .values()
            .filter(|execution| {
                execution.organization_id == organization_id
                    && execution.project_id == project_id
                    && execution.environment_id == environment_id
                    && !execution.is_bound_task()
            })
            .cloned()
            .collect::<Vec<_>>();
        executions
            .sort_by_key(|execution| std::cmp::Reverse((execution.requested_at, execution.id)));
        executions.truncate(limit);
        Ok(executions)
    }

    async fn replay(
        &self,
        idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
    ) -> Result<Option<Execution>, RepositoryError> {
        let state = self.state.read().await;
        let key = (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        );
        match state.idempotency.get(&key) {
            Some((digest, _)) if digest != &idempotency.request_digest => {
                Err(RepositoryError::IdempotencyConflict)
            }
            Some((_, execution)) => Ok(Some(execution.clone())),
            None => Ok(None),
        }
    }

    async fn request_cancellation(
        &self,
        request: TransitionExecution,
    ) -> Result<ExecutionWrite, RepositoryError> {
        request
            .execution
            .validate()
            .map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let idempotency_key = (
            request.idempotency.storage_key().0.to_owned(),
            request.idempotency.storage_key().1.to_owned(),
        );
        if let Some((digest, execution)) = state.idempotency.get(&idempotency_key) {
            if digest != &request.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(ExecutionWrite {
                execution: execution.clone(),
                replayed: true,
            });
        }
        let key = (request.execution.organization_id, request.execution.id);
        let existing = state
            .executions
            .get(&key)
            .ok_or(RepositoryError::NotFound)?;
        validate_execution_transition(existing, &request.execution, request.expected_version)?;
        state.executions.insert(key, request.execution.clone());
        state.idempotency.insert(
            idempotency_key,
            (
                request.idempotency.request_digest,
                request.execution.clone(),
            ),
        );
        state.outbox.push(request.event);
        Ok(ExecutionWrite {
            execution: request.execution,
            replayed: false,
        })
    }

    async fn pending_operation_starts(
        &self,
        limit: usize,
    ) -> Result<Vec<Execution>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let state = self.state.read().await;
        let mut executions = state
            .executions
            .values()
            .filter(|execution| {
                !execution.status.is_terminal() && !state.operation_starts.contains(&execution.id)
            })
            .cloned()
            .collect::<Vec<_>>();
        executions.sort_by_key(|execution| (execution.requested_at, execution.id));
        executions.truncate(limit);
        Ok(executions)
    }

    async fn save(
        &self,
        execution: Execution,
        expected_version: u64,
    ) -> Result<Execution, RepositoryError> {
        execution.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let key = (execution.organization_id, execution.id);
        let existing = state
            .executions
            .get(&key)
            .ok_or(RepositoryError::NotFound)?;
        validate_execution_transition(existing, &execution, expected_version)?;
        state.executions.insert(key, execution.clone());
        Ok(execution)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::executions::domain::{
        ExecutionArtifact, ExecutionProcess, ExecutionResources, ExecutionTemplate,
        WorkflowExecutionBinding,
    };
    use crate::modules::shared_kernel::domain::{
        ExecutionTemplateId, ExecutionTemplateRevisionId, IdempotencyRequest, NodeId,
        PlanRevisionId, Sha256Digest,
    };
    use chrono::Utc;
    use std::collections::BTreeMap;
    use uuid::Uuid;

    fn execution() -> Execution {
        let digest = format!("sha256:{}", "a".repeat(64));
        Execution::create(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            ExecutionId::new(),
            ExecutionTemplate {
                artifact: ExecutionArtifact {
                    uri: format!("oci://registry.example/tasks/echo@{digest}"),
                    digest,
                    media_type: "application/vnd.oci.image.manifest.v1+json".into(),
                },
                process: ExecutionProcess {
                    command: vec!["/bin/echo".into()],
                    args: Vec::new(),
                    working_directory: None,
                    environment: BTreeMap::new(),
                },
                input: serde_json::json!({"hello": "world"}),
                resources: ExecutionResources {
                    cpu_millis: 100,
                    memory_bytes: 64 * 1024 * 1024,
                    pids: 32,
                    ephemeral_storage_bytes: None,
                    timeout_ms: 1_000,
                },
            },
            Utc::now(),
        )
        .expect("execution")
    }

    fn create_request(execution: Execution, body: &[u8]) -> CreateExecution {
        create_request_with_key(execution, "request-1", body)
    }

    fn create_request_with_key(
        execution: Execution,
        idempotency_key: &str,
        body: &[u8],
    ) -> CreateExecution {
        CreateExecution {
            event: crate::modules::executions::domain::events::ExecutionRequested::envelope(
                &execution,
                Uuid::now_v7(),
            )
            .expect("event"),
            idempotency: IdempotencyRequest::new(
                format!("organizations/{}/executions", execution.organization_id),
                idempotency_key,
                body,
            )
            .expect("idempotency"),
            execution,
        }
    }

    #[tokio::test]
    async fn create_replays_exact_request_and_rejects_changed_body() {
        let repository = InMemoryExecutionRepository::new();
        let execution = execution();
        let created = repository
            .create(create_request(execution.clone(), b"same"))
            .await
            .expect("create");
        assert!(!created.replayed);
        let replay = repository
            .create(create_request(execution.clone(), b"same"))
            .await
            .expect("replay");
        assert!(replay.replayed);
        assert_eq!(replay.execution, execution);
        assert!(matches!(
            repository
                .create(create_request(execution, b"changed"))
                .await,
            Err(RepositoryError::IdempotencyConflict)
        ));
        assert_eq!(repository.outbox_events().await.len(), 1);
    }

    #[tokio::test]
    async fn repository_accepts_only_proven_aggregate_transitions() {
        let repository = InMemoryExecutionRepository::new();
        let execution = execution();
        repository
            .create(create_request(execution.clone(), b"create"))
            .await
            .expect("create");
        let mut scheduled = execution.clone();
        scheduled
            .schedule(
                NodeId::new(),
                format!("sha256:{}", "b".repeat(64)),
                execution.updated_at,
            )
            .expect("schedule");
        repository
            .save(scheduled.clone(), execution.aggregate_version)
            .await
            .expect("save schedule");

        let mut forged = scheduled.clone();
        forged.aggregate_version += 1;
        forged.template.input = serde_json::json!({"forged": true});
        forged.template_digest = forged.template.digest().expect("forged template digest");
        assert!(matches!(
            repository.save(forged, scheduled.aggregate_version).await,
            Err(RepositoryError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn workflow_step_attempt_has_one_execution_across_idempotency_keys() {
        let repository = InMemoryExecutionRepository::new();
        let base = execution();
        let binding = WorkflowExecutionBinding {
            workflow_run_id: WorkflowRunId::new(),
            plan_revision_id: PlanRevisionId::new(),
            plan_digest: Sha256Digest::parse(format!("sha256:{}", "b".repeat(64)))
                .expect("plan digest"),
            step_id: "run_task".into(),
            step_attempt: 1,
            execution_template_id: ExecutionTemplateId::new(),
            execution_template_revision_id: ExecutionTemplateRevisionId::new(),
            execution_template_digest: Sha256Digest::parse(format!("sha256:{}", "c".repeat(64)))
                .expect("template digest"),
        };
        let first = Execution::create_with_workflow(
            base.organization_id,
            base.project_id,
            base.environment_id,
            ExecutionId::new(),
            base.template.clone(),
            Some(binding.clone()),
            base.requested_at,
        )
        .expect("first Workflow execution");
        repository
            .create(create_request_with_key(first.clone(), "first", b"first"))
            .await
            .expect("create first Workflow execution");
        let second = Execution::create_with_workflow(
            base.organization_id,
            base.project_id,
            base.environment_id,
            ExecutionId::new(),
            base.template,
            Some(binding.clone()),
            base.requested_at,
        )
        .expect("second Workflow execution");
        assert!(matches!(
            repository
                .create(create_request_with_key(second, "second", b"second"))
                .await,
            Err(RepositoryError::Conflict(_))
        ));
        assert_eq!(
            repository
                .find_for_workflow(
                    first.organization_id,
                    binding.workflow_run_id,
                    &binding.step_id,
                    binding.step_attempt,
                )
                .await
                .expect("find Workflow execution"),
            Some(first)
        );
    }
}
