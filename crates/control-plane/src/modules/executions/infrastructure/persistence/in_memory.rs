use crate::modules::executions::domain::{
    validate_execution_transition, CreateExecution, Execution, ExecutionWrite,
    IExecutionRepository, TransitionExecution,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, ExecutionId, OrganizationId, ProjectId, RepositoryError,
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
        state.executions.insert(identity, request.execution.clone());
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
    };
    use crate::modules::shared_kernel::domain::{IdempotencyRequest, NodeId};
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
        CreateExecution {
            event: crate::modules::executions::domain::events::ExecutionRequested::envelope(
                &execution,
                Uuid::now_v7(),
            )
            .expect("event"),
            idempotency: IdempotencyRequest::new(
                format!("organizations/{}/executions", execution.organization_id),
                "request-1",
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
}
