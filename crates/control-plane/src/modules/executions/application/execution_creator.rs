use super::CreateExecutionResult;
use crate::modules::executions::domain::events::ExecutionRequested;
use crate::modules::executions::domain::{
    CreateExecution, Execution, ExecutionTaskPolicy, ExecutionTemplate, IExecutionRepository,
    WorkflowExecutionBinding,
};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, ExecutionId, IdempotencyRequest, NodeId, OrganizationId, ProjectId,
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub(crate) struct ExecutionCreation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub template: ExecutionTemplate,
    pub workflow: Option<WorkflowExecutionBinding>,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

/// Internal creation request for an existing Cloud owner that needs one
/// deterministic, exact-node finite Task. The caller supplies the stable
/// Execution identity; this service retains the existing Execution repository,
/// idempotency, Outbox, Operations reconciler, and Flow as the sole lifecycle.
#[derive(Debug, Clone)]
pub(crate) struct BoundExecutionCreation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub execution_id: ExecutionId,
    pub template: ExecutionTemplate,
    pub target_node_id: NodeId,
    pub task_policy: ExecutionTaskPolicy,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

#[derive(Clone)]
pub(crate) struct ExecutionCreator {
    environments: Arc<dyn IEnvironmentRepository>,
    executions: Arc<dyn IExecutionRepository>,
}

impl ExecutionCreator {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        executions: Arc<dyn IExecutionRepository>,
    ) -> Self {
        Self {
            environments,
            executions,
        }
    }

    pub async fn create(
        &self,
        request: ExecutionCreation,
    ) -> ApplicationResult<CreateExecutionResult> {
        match self
            .environments
            .find(
                request.organization_id,
                request.project_id,
                request.environment_id,
            )
            .await
        {
            Ok(Some(_)) => {}
            Ok(None) => return Err(ApplicationError::NotFound("environment not found".into())),
            Err(error) => return Err(error.into()),
        }
        request
            .template
            .validate()
            .map_err(ApplicationError::Invalid)?;
        if let Some(workflow) = &request.workflow {
            workflow.validate().map_err(ApplicationError::Invalid)?;
        }
        let canonical = serde_json::to_vec(&serde_json::json!({
            "organizationId": request.organization_id,
            "projectId": request.project_id,
            "environmentId": request.environment_id,
            "template": request.template,
            "workflow": request.workflow,
        }))
        .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/environments/{}/executions",
                request.organization_id, request.project_id, request.environment_id
            ),
            request.idempotency_key,
            &canonical,
        )
        .map_err(ApplicationError::Invalid)?;
        if let Some(replay) = self.executions.replay(&idempotency).await? {
            return Ok(CreateExecutionResult {
                execution: replay,
                replayed: true,
            });
        }
        let execution = Execution::create_with_workflow(
            request.organization_id,
            request.project_id,
            request.environment_id,
            ExecutionId::new(),
            request.template,
            request.workflow,
            request.requested_at,
        )
        .map_err(ApplicationError::Invalid)?;
        let event = ExecutionRequested::envelope(&execution, request.request_id)
            .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let write = self
            .executions
            .create(CreateExecution {
                execution,
                idempotency,
                event,
            })
            .await?;
        Ok(CreateExecutionResult {
            execution: write.execution,
            replayed: write.replayed,
        })
    }

    pub async fn create_bound_task(
        &self,
        request: BoundExecutionCreation,
    ) -> ApplicationResult<CreateExecutionResult> {
        self.require_environment(
            request.organization_id,
            request.project_id,
            request.environment_id,
        )
        .await?;
        request
            .task_policy
            .validate(request.target_node_id, &request.template)
            .map_err(ApplicationError::Invalid)?;
        let canonical = serde_json::to_vec(&serde_json::json!({
            "organizationId": request.organization_id,
            "projectId": request.project_id,
            "environmentId": request.environment_id,
            "executionId": request.execution_id,
            "template": request.template,
            "targetNodeId": request.target_node_id,
            "taskPolicy": request.task_policy,
        }))
        .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/environments/{}/internal-bound-executions",
                request.organization_id, request.project_id, request.environment_id
            ),
            request.idempotency_key.clone(),
            &canonical,
        )
        .map_err(ApplicationError::Invalid)?;
        if let Some(replay) = self.executions.replay(&idempotency).await? {
            validate_bound_replay(&request, &replay)?;
            return Ok(CreateExecutionResult {
                execution: replay,
                replayed: true,
            });
        }
        let execution = Execution::create_bound_task(
            request.organization_id,
            request.project_id,
            request.environment_id,
            request.execution_id,
            request.template.clone(),
            request.target_node_id,
            request.task_policy.clone(),
            request.requested_at,
        )
        .map_err(ApplicationError::Invalid)?;
        let event = ExecutionRequested::envelope(&execution, request.request_id)
            .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let write = self
            .executions
            .create(CreateExecution {
                execution,
                idempotency,
                event,
            })
            .await?;
        validate_bound_replay(&request, &write.execution)?;
        Ok(CreateExecutionResult {
            execution: write.execution,
            replayed: write.replayed,
        })
    }

    async fn require_environment(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> ApplicationResult<()> {
        match self
            .environments
            .find(organization_id, project_id, environment_id)
            .await
        {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(ApplicationError::NotFound("environment not found".into())),
            Err(error) => Err(error.into()),
        }
    }
}

fn validate_bound_replay(
    request: &BoundExecutionCreation,
    execution: &Execution,
) -> ApplicationResult<()> {
    if execution.organization_id != request.organization_id
        || execution.project_id != request.project_id
        || execution.environment_id != request.environment_id
        || execution.id != request.execution_id
        || execution.workflow.is_some()
        || execution.target_node_id != Some(request.target_node_id)
        || execution.task_policy.as_ref() != Some(&request.task_policy)
        || execution.template != request.template
    {
        return Err(ApplicationError::Internal(
            "bound execution replay changed its immutable identity".into(),
        ));
    }
    Ok(())
}
