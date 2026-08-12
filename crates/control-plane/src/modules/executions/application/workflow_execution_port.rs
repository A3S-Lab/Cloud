use super::execution_cancellation::{ExecutionCancellation, ExecutionCancellationService};
use super::execution_creator::{ExecutionCreation, ExecutionCreator};
use crate::modules::executions::domain::{
    Execution, ExecutionStatus, IExecutionRepository, IExecutionTemplateRepository,
    WorkflowExecutionBinding, EXECUTION_TEMPLATE_CAPABILITY,
};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, ExecutionTemplateId, ExecutionTemplateRevisionId,
    OrganizationId, PlanRevisionId, ProjectId, Sha256Digest, WorkflowRunId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowExecutionRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub step_id: String,
    pub step_attempt: u64,
    pub execution_template_id: ExecutionTemplateId,
    pub execution_template_revision_id: ExecutionTemplateRevisionId,
    pub execution_template_digest: Sha256Digest,
    pub capability: String,
    pub input: serde_json::Value,
    pub requested_at: DateTime<Utc>,
}

impl WorkflowExecutionRequest {
    pub fn validate(&self) -> Result<(), String> {
        WorkflowExecutionBinding::from(self).validate()?;
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.capability != EXECUTION_TEMPLATE_CAPABILITY
            || self.requested_at != canonical_timestamp(self.requested_at)
        {
            return Err("Workflow Execution request authority is invalid".into());
        }
        Ok(())
    }

    fn idempotency_key(&self) -> String {
        format!(
            "workflow:{}:step:{}:attempt:{}",
            self.workflow_run_id, self.step_id, self.step_attempt
        )
    }
}

impl From<&WorkflowExecutionRequest> for WorkflowExecutionBinding {
    fn from(request: &WorkflowExecutionRequest) -> Self {
        Self {
            workflow_run_id: request.workflow_run_id,
            plan_revision_id: request.plan_revision_id,
            plan_digest: request.plan_digest.clone(),
            step_id: request.step_id.clone(),
            step_attempt: request.step_attempt,
            execution_template_id: request.execution_template_id,
            execution_template_revision_id: request.execution_template_revision_id,
            execution_template_digest: request.execution_template_digest.clone(),
        }
    }
}

#[async_trait]
pub trait IWorkflowExecutionPort: Send + Sync {
    async fn start_or_adopt(
        &self,
        request: &WorkflowExecutionRequest,
    ) -> ApplicationResult<Execution>;

    async fn adopt(
        &self,
        request: &WorkflowExecutionRequest,
    ) -> ApplicationResult<Option<Execution>>;

    async fn request_cancellation(
        &self,
        request: &WorkflowExecutionRequest,
        requested_at: DateTime<Utc>,
    ) -> ApplicationResult<Option<Execution>>;
}

#[derive(Clone)]
pub struct WorkflowExecutionApplicationService {
    templates: Arc<dyn IExecutionTemplateRepository>,
    executions: Arc<dyn IExecutionRepository>,
    creator: ExecutionCreator,
    cancellation: ExecutionCancellationService,
}

impl WorkflowExecutionApplicationService {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        templates: Arc<dyn IExecutionTemplateRepository>,
        executions: Arc<dyn IExecutionRepository>,
    ) -> Self {
        Self {
            templates,
            creator: ExecutionCreator::new(environments, Arc::clone(&executions)),
            cancellation: ExecutionCancellationService::new(Arc::clone(&executions)),
            executions,
        }
    }

    async fn resolve_template(
        &self,
        request: &WorkflowExecutionRequest,
    ) -> ApplicationResult<crate::modules::executions::domain::ExecutionTemplate> {
        let revision = self
            .templates
            .find(
                request.organization_id,
                request.project_id,
                request.execution_template_id,
                request.execution_template_revision_id,
            )
            .await?
            .ok_or_else(|| {
                ApplicationError::NotFound("execution template revision not found".into())
            })?;
        if revision.definition.digest() != &request.execution_template_digest {
            return Err(ApplicationError::Conflict(
                "Workflow ExecutionTemplate digest does not match its exact revision".into(),
            ));
        }
        revision
            .definition
            .materialize(request.input.clone())
            .map_err(ApplicationError::Invalid)
    }

    async fn validate_adopted(
        &self,
        request: &WorkflowExecutionRequest,
        execution: Execution,
    ) -> ApplicationResult<Execution> {
        let template = self.resolve_template(request).await?;
        let template_digest = template.digest().map_err(ApplicationError::Internal)?;
        if execution.organization_id != request.organization_id
            || execution.project_id != request.project_id
            || execution.environment_id != request.environment_id
            || execution.workflow.as_ref() != Some(&WorkflowExecutionBinding::from(request))
            || execution.template != template
            || execution.template_digest != template_digest
            || execution.requested_at != request.requested_at
        {
            return Err(ApplicationError::Conflict(
                "adopted Workflow child Execution changed its immutable authority".into(),
            ));
        }
        Ok(execution)
    }
}

#[async_trait]
impl IWorkflowExecutionPort for WorkflowExecutionApplicationService {
    async fn start_or_adopt(
        &self,
        request: &WorkflowExecutionRequest,
    ) -> ApplicationResult<Execution> {
        request.validate().map_err(ApplicationError::Invalid)?;
        if let Some(execution) = self.adopt(request).await? {
            return Ok(execution);
        }
        let template = self.resolve_template(request).await?;
        let creation = ExecutionCreation {
            organization_id: request.organization_id,
            project_id: request.project_id,
            environment_id: request.environment_id,
            template,
            workflow: Some(WorkflowExecutionBinding::from(request)),
            idempotency_key: request.idempotency_key(),
            request_id: request.workflow_run_id.as_uuid(),
            requested_at: request.requested_at,
        };
        match self.creator.create(creation).await {
            Ok(result) => self.validate_adopted(request, result.execution).await,
            Err(error @ ApplicationError::Conflict(_)) => match self.adopt(request).await? {
                Some(execution) => Ok(execution),
                None => Err(error),
            },
            Err(error) => Err(error),
        }
    }

    async fn adopt(
        &self,
        request: &WorkflowExecutionRequest,
    ) -> ApplicationResult<Option<Execution>> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let Some(execution) = self
            .executions
            .find_for_workflow(
                request.organization_id,
                request.workflow_run_id,
                &request.step_id,
                request.step_attempt,
            )
            .await?
        else {
            return Ok(None);
        };
        self.validate_adopted(request, execution).await.map(Some)
    }

    async fn request_cancellation(
        &self,
        request: &WorkflowExecutionRequest,
        requested_at: DateTime<Utc>,
    ) -> ApplicationResult<Option<Execution>> {
        let Some(execution) = self.adopt(request).await? else {
            return Ok(None);
        };
        if execution.status.is_terminal()
            || matches!(
                execution.status,
                ExecutionStatus::Cancelling | ExecutionStatus::CleanupPending
            )
        {
            return Ok(Some(execution));
        }
        let result = self
            .cancellation
            .cancel(ExecutionCancellation {
                execution,
                idempotency_key: format!("{}:cancel", request.idempotency_key()),
                request_id: request.workflow_run_id.as_uuid(),
                requested_at,
            })
            .await?;
        self.validate_adopted(request, result.execution)
            .await
            .map(Some)
    }
}
