use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, EnvironmentId, IdempotencyRequest, OntologyId,
    OntologyRevisionId, PlanRevisionId, PrincipalId, Sha256Digest, WorkflowGoalId, WorkflowRunId,
};
use crate::modules::workflow::domain::{
    workflow_run_timeout_seconds, CancelWorkflowRunWrite, CreateWorkflowGoalWrite,
    CreateWorkflowRunWrite, IOntologyRepository, IWorkflowDefinitionRepository,
    IWorkflowGoalRepository, IWorkflowRunRepository, WorkflowCompositeFrame, WorkflowGoalCompiled,
    WorkflowGoalContract, WorkflowGoalRecord, WorkflowGoalSpec, WorkflowPlanCompiler,
    WorkflowRunCancellationRequested, WorkflowRunCompiler, WorkflowRunRecord, WorkflowRunRequested,
    WORKFLOW_COMPOSITE_FRAME_MAX_BYTES,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowCompositeExecutionRequest {
    pub frame: WorkflowCompositeFrame,
    pub ontology_id: OntologyId,
    pub ontology_revision_id: OntologyRevisionId,
    pub ontology_digest: Sha256Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<EnvironmentId>,
    pub requested_by: PrincipalId,
    pub requested_at: DateTime<Utc>,
    pub timeout_seconds: u64,
}

impl WorkflowCompositeExecutionRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.frame.organization_id.as_uuid().is_nil()
            || self.frame.project_id.as_uuid().is_nil()
            || self.frame.workflow_run_id.as_uuid().is_nil()
            || self.frame.child_workflow_definition_id.as_uuid().is_nil()
            || self.frame.child_workflow_revision_id.as_uuid().is_nil()
            || self.ontology_id.as_uuid().is_nil()
            || self.ontology_revision_id.as_uuid().is_nil()
            || self.requested_by.as_uuid().is_nil()
            || self
                .environment_id
                .is_some_and(|environment_id| environment_id.as_uuid().is_nil())
            || self.requested_at != canonical_timestamp(self.requested_at)
        {
            return Err("Workflow composite execution request authority is invalid".into());
        }
        workflow_run_timeout_seconds(Some(self.timeout_seconds))?;
        canonical_json_bounded(
            self,
            WORKFLOW_COMPOSITE_FRAME_MAX_BYTES,
            "Workflow composite execution request",
        )?;
        Ok(())
    }

    pub fn workflow_run_id(&self) -> WorkflowRunId {
        self.frame.child_workflow_run_id()
    }

    pub fn workflow_goal_id(&self) -> WorkflowGoalId {
        self.frame.child_workflow_goal_id()
    }

    pub fn plan_revision_id(&self) -> PlanRevisionId {
        self.frame.child_plan_revision_id()
    }

    fn request_id(&self, purpose: &[u8]) -> Uuid {
        Uuid::new_v5(&self.workflow_run_id().as_uuid(), purpose)
    }

    fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_json_bounded(
            self,
            WORKFLOW_COMPOSITE_FRAME_MAX_BYTES,
            "Workflow composite execution request",
        )
    }
}

#[async_trait]
pub trait IWorkflowCompositeExecutionPort: Send + Sync {
    async fn start_or_adopt(
        &self,
        request: &WorkflowCompositeExecutionRequest,
    ) -> ApplicationResult<WorkflowRunRecord>;

    async fn adopt(
        &self,
        request: &WorkflowCompositeExecutionRequest,
    ) -> ApplicationResult<Option<WorkflowRunRecord>>;

    async fn request_cancellation(
        &self,
        request: &WorkflowCompositeExecutionRequest,
        reason: Option<String>,
        requested_by: PrincipalId,
        requested_at: DateTime<Utc>,
    ) -> ApplicationResult<Option<WorkflowRunRecord>>;
}

pub struct WorkflowCompositeExecutionApplicationService {
    workflows: Arc<dyn IWorkflowDefinitionRepository>,
    ontologies: Arc<dyn IOntologyRepository>,
    goals: Arc<dyn IWorkflowGoalRepository>,
    runs: Arc<dyn IWorkflowRunRepository>,
}

impl WorkflowCompositeExecutionApplicationService {
    pub fn new(
        workflows: Arc<dyn IWorkflowDefinitionRepository>,
        ontologies: Arc<dyn IOntologyRepository>,
        goals: Arc<dyn IWorkflowGoalRepository>,
        runs: Arc<dyn IWorkflowRunRepository>,
    ) -> Self {
        Self {
            workflows,
            ontologies,
            goals,
            runs,
        }
    }

    async fn compile_goal(
        &self,
        request: &WorkflowCompositeExecutionRequest,
    ) -> ApplicationResult<(
        WorkflowGoalRecord,
        crate::modules::workflow::domain::WorkflowRevision,
    )> {
        let frame = &request.frame;
        let definition = self
            .workflows
            .find(frame.organization_id, frame.child_workflow_definition_id)
            .await?
            .ok_or_else(|| {
                ApplicationError::NotFound("composite child WorkflowDefinition not found".into())
            })?;
        let revision = self
            .workflows
            .find_revision(
                frame.organization_id,
                frame.child_workflow_definition_id,
                frame.child_workflow_revision_id,
            )
            .await?
            .ok_or_else(|| {
                ApplicationError::NotFound("composite child WorkflowRevision not found".into())
            })?;
        let ontology = self
            .ontologies
            .find_revision(
                frame.organization_id,
                request.ontology_id,
                request.ontology_revision_id,
            )
            .await?
            .ok_or_else(|| {
                ApplicationError::NotFound("composite child OntologyRevision not found".into())
            })?;
        if definition.organization_id != frame.organization_id
            || definition.project_id != frame.project_id
            || revision.organization_id != frame.organization_id
            || revision.project_id != frame.project_id
            || revision.workflow_definition_id != frame.child_workflow_definition_id
            || revision.id != frame.child_workflow_revision_id
            || revision.contract.digest() != &frame.child_workflow_digest
            || ontology.organization_id != frame.organization_id
            || ontology.project_id != frame.project_id
            || ontology.ontology_id != request.ontology_id
            || ontology.id != request.ontology_revision_id
            || ontology.contract.digest() != &request.ontology_digest
        {
            return Err(ApplicationError::Conflict(
                "composite child immutable authority drifted".into(),
            ));
        }
        let definition = definition
            .at_revision(&revision)
            .map_err(ApplicationError::Invalid)?;
        let contract = WorkflowGoalContract::from_spec(WorkflowGoalSpec {
            name: format!("Composite {} frame {}", frame.region_step_id, frame.ordinal),
            workflow_definition_id: frame.child_workflow_definition_id,
            workflow_revision_id: frame.child_workflow_revision_id,
            workflow_digest: frame.child_workflow_digest.clone(),
            ontology_id: request.ontology_id,
            ontology_revision_id: request.ontology_revision_id,
            ontology_digest: request.ontology_digest.clone(),
            environment_id: request.environment_id,
            input: frame.child_input.clone(),
        })
        .map_err(ApplicationError::Invalid)?;
        let compiled = WorkflowPlanCompiler::compile_goal(
            request.workflow_goal_id(),
            request.plan_revision_id(),
            contract,
            &definition,
            &revision,
            &ontology,
            request.requested_by,
            request.requested_at,
        )
        .map_err(ApplicationError::Invalid)?;
        let canonical = request
            .canonical_bytes()
            .map_err(ApplicationError::Invalid)?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/workflow-composite-goals",
                frame.organization_id
            ),
            frame.frame_digest.to_string(),
            &canonical,
        )
        .map_err(ApplicationError::Invalid)?;
        let request_id = request.request_id(b"goal-request");
        let event =
            WorkflowGoalCompiled::envelope(&compiled.goal, &compiled.plan_revision, request_id)
                .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let expected = WorkflowGoalRecord {
            goal: compiled.goal,
            plan_revision: compiled.plan_revision,
        };
        let write = self
            .goals
            .create(CreateWorkflowGoalWrite {
                record: expected.clone(),
                event,
                actor_principal_id: request.requested_by,
                request_id,
                idempotency,
            })
            .await?;
        if write.value != expected {
            return Err(ApplicationError::Conflict(
                "composite child WorkflowGoal replay authority drifted".into(),
            ));
        }
        Ok((write.value, revision))
    }

    fn validate_child(
        request: &WorkflowCompositeExecutionRequest,
        record: &WorkflowRunRecord,
    ) -> Result<(), ApplicationError> {
        record.validate().map_err(ApplicationError::Invalid)?;
        let run = &record.run;
        let input = &run.execution_input;
        let timeout =
            chrono::Duration::seconds(i64::try_from(request.timeout_seconds).map_err(|_| {
                ApplicationError::Invalid(
                    "composite child timeout exceeds the supported duration".into(),
                )
            })?);
        let deadline_at = request
            .requested_at
            .checked_add_signed(timeout)
            .ok_or_else(|| {
                ApplicationError::Invalid("composite child deadline overflowed".into())
            })?;
        if run.id != request.workflow_run_id()
            || run.workflow_goal_id != request.workflow_goal_id()
            || run.plan_revision_id != request.plan_revision_id()
            || run.requested_by != request.requested_by
            || run.requested_at != request.requested_at
            || input.deadline_at != deadline_at
            || input.plan.workflow_definition_id != request.frame.child_workflow_definition_id
            || input.plan.workflow_revision_id != request.frame.child_workflow_revision_id
            || input.plan.workflow_digest != request.frame.child_workflow_digest
            || input.plan.ontology_id != request.ontology_id
            || input.plan.ontology_revision_id != request.ontology_revision_id
            || input.plan.ontology_digest != request.ontology_digest
            || input.plan.environment_id != request.environment_id
            || input.goal_input != request.frame.child_input
            || input.plan.input_digest != request.frame.child_input_digest
        {
            return Err(ApplicationError::Conflict(
                "composite child WorkflowRun authority drifted".into(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl IWorkflowCompositeExecutionPort for WorkflowCompositeExecutionApplicationService {
    async fn start_or_adopt(
        &self,
        request: &WorkflowCompositeExecutionRequest,
    ) -> ApplicationResult<WorkflowRunRecord> {
        request.validate().map_err(ApplicationError::Invalid)?;
        if let Some(record) = self.adopt(request).await? {
            return Ok(record);
        }
        let (goal, revision) = self.compile_goal(request).await?;
        let compiled = WorkflowRunCompiler::compile(
            request.workflow_run_id(),
            &goal.goal,
            &goal.plan_revision,
            &revision,
            Some(request.timeout_seconds),
            request.requested_by,
            request.requested_at,
        )
        .map_err(ApplicationError::Invalid)?;
        let record = WorkflowRunRecord {
            run: compiled.run,
            steps: compiled.steps,
        };
        Self::validate_child(request, &record)?;
        let canonical = request
            .canonical_bytes()
            .map_err(ApplicationError::Invalid)?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/workflow-composite-runs",
                request.frame.organization_id
            ),
            request.frame.frame_digest.to_string(),
            &canonical,
        )
        .map_err(ApplicationError::Invalid)?;
        let request_id = request.request_id(b"run-request");
        let event = WorkflowRunRequested::envelope(&record.run, request_id)
            .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let write = self
            .runs
            .create(CreateWorkflowRunWrite {
                record,
                event,
                actor_principal_id: request.requested_by,
                request_id,
                idempotency,
            })
            .await?;
        Self::validate_child(request, &write.value)?;
        Ok(write.value)
    }

    async fn adopt(
        &self,
        request: &WorkflowCompositeExecutionRequest,
    ) -> ApplicationResult<Option<WorkflowRunRecord>> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let Some(record) = self
            .runs
            .find(request.frame.organization_id, request.workflow_run_id())
            .await?
        else {
            return Ok(None);
        };
        Self::validate_child(request, &record)?;
        Ok(Some(record))
    }

    async fn request_cancellation(
        &self,
        request: &WorkflowCompositeExecutionRequest,
        reason: Option<String>,
        requested_by: PrincipalId,
        requested_at: DateTime<Utc>,
    ) -> ApplicationResult<Option<WorkflowRunRecord>> {
        request.validate().map_err(ApplicationError::Invalid)?;
        if requested_by.as_uuid().is_nil() {
            return Err(ApplicationError::Invalid(
                "composite child cancellation principal is invalid".into(),
            ));
        }
        let requested_at = canonical_timestamp(requested_at);
        let Some(mut record) = self.adopt(request).await? else {
            return Ok(None);
        };
        if record.run.status.is_terminal() {
            return Ok(Some(record));
        }
        if record.run.status == crate::modules::workflow::domain::WorkflowRunStatus::Cancelling {
            return if record.run.cancellation_reason == reason
                && record.run.cancellation_requested_by == Some(requested_by)
            {
                Ok(Some(record))
            } else {
                Err(ApplicationError::Conflict(
                    "composite child cancellation authority drifted".into(),
                ))
            };
        }
        let canonical = canonical_json_bounded(
            &serde_json::json!({
                "frameDigest": request.frame.frame_digest,
                "reason": reason,
                "requestedBy": requested_by,
                "requestedAt": requested_at,
            }),
            32 * 1024,
            "Workflow composite child cancellation request",
        )
        .map_err(ApplicationError::Invalid)?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/workflow-runs/{}/cancellation",
                request.frame.organization_id,
                request.workflow_run_id()
            ),
            format!("composite:{}", request.frame.frame_digest),
            &canonical,
        )
        .map_err(ApplicationError::Invalid)?;
        if let Some(replayed) = self.runs.replay(&idempotency).await? {
            Self::validate_child(request, &replayed)?;
            return Ok(Some(replayed));
        }
        let expected_version = record.run.aggregate_version;
        record
            .run
            .request_cancellation(reason, requested_by, requested_at)
            .map_err(ApplicationError::Conflict)?;
        let request_id = request.request_id(b"cancel-request");
        let event = WorkflowRunCancellationRequested::envelope(&record.run, request_id)
            .map_err(ApplicationError::Internal)?;
        let write = self
            .runs
            .request_cancellation(CancelWorkflowRunWrite {
                record,
                expected_version,
                event,
                actor_principal_id: requested_by,
                request_id,
                idempotency,
            })
            .await?;
        Self::validate_child(request, &write.value)?;
        Ok(Some(write.value))
    }
}
