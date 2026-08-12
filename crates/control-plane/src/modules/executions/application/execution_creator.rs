use super::CreateExecutionResult;
use crate::modules::executions::domain::events::ExecutionRequested;
use crate::modules::executions::domain::{
    CreateExecution, Execution, ExecutionTemplate, IExecutionRepository, WorkflowExecutionBinding,
};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, ExecutionId, IdempotencyRequest, OrganizationId, ProjectId,
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
}
