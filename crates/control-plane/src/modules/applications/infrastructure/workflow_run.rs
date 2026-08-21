use super::workflow_revision::application_workflow_revision_evidence;
use crate::modules::applications::application::{
    ApplicationWorkflowRunEvidence, ApplicationWorkflowRunRequest, IApplicationWorkflowRunPort,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, IdempotencyRequest, Sha256Digest,
};
use crate::modules::workflow::domain::{
    workflow_run_timeout_seconds, CancelWorkflowRunWrite, CreateWorkflowGoalWrite,
    CreateWorkflowRunWrite, IOntologyRepository, IWorkflowDefinitionRepository,
    IWorkflowGoalRepository, IWorkflowRunRepository, WorkflowGoalCompiled, WorkflowGoalContract,
    WorkflowGoalRecord, WorkflowGoalSpec, WorkflowPlanCompiler, WorkflowRunCancellationRequested,
    WorkflowRunCompiler, WorkflowRunRecord, WorkflowRunRequested, WorkflowRunStatus,
    WorkflowStepKind, WORKFLOW_RUN_FLOW_VERSION_V10, WORKFLOW_RUN_INPUT_SCHEMA_V10,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Production adapter from Applications invocation intent to the existing
/// Workflow Goal/Plan/Run repositories.
///
/// Stable Application-invocation-derived identities and repository
/// idempotency make the three writes adoptable after a process failure. No
/// Flow state is copied to Applications and this adapter never dispatches a
/// provider directly.
pub struct WorkflowApplicationRunService {
    workflows: Arc<dyn IWorkflowDefinitionRepository>,
    ontologies: Arc<dyn IOntologyRepository>,
    goals: Arc<dyn IWorkflowGoalRepository>,
    runs: Arc<dyn IWorkflowRunRepository>,
}

impl WorkflowApplicationRunService {
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

    fn run_idempotency(
        request: &ApplicationWorkflowRunRequest,
    ) -> ApplicationResult<IdempotencyRequest> {
        IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/applications/{}/workflow-runs",
                request.organization_id, request.project_id, request.application_id
            ),
            request.invocation_id.to_string(),
            &request
                .canonical_bytes()
                .map_err(ApplicationError::Invalid)?,
        )
        .map_err(ApplicationError::Invalid)
    }

    fn goal_idempotency(
        request: &ApplicationWorkflowRunRequest,
    ) -> ApplicationResult<IdempotencyRequest> {
        IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/applications/{}/workflow-goals",
                request.organization_id, request.project_id, request.application_id
            ),
            request.invocation_id.to_string(),
            &request
                .canonical_bytes()
                .map_err(ApplicationError::Invalid)?,
        )
        .map_err(ApplicationError::Invalid)
    }

    async fn adopt_record(
        &self,
        request: &ApplicationWorkflowRunRequest,
    ) -> ApplicationResult<Option<WorkflowRunRecord>> {
        let idempotency = Self::run_idempotency(request)?;
        if let Some(record) = self.runs.replay(&idempotency).await? {
            Self::validate_record(request, &record)?;
            return Ok(Some(record));
        }
        if self
            .runs
            .find(request.organization_id, request.workflow_run_id())
            .await?
            .is_some()
        {
            return Err(ApplicationError::Conflict(
                "Application invocation WorkflowRun identity is already owned by another request"
                    .into(),
            ));
        }
        Ok(None)
    }

    async fn compile_goal(
        &self,
        request: &ApplicationWorkflowRunRequest,
    ) -> ApplicationResult<(
        WorkflowGoalRecord,
        crate::modules::workflow::domain::WorkflowRevision,
    )> {
        let definition = self
            .workflows
            .find(
                request.organization_id,
                request.workflow.workflow_definition_id,
            )
            .await?
            .filter(|definition| definition.project_id == request.project_id)
            .ok_or_else(|| {
                ApplicationError::NotFound("Application WorkflowDefinition not found".into())
            })?;
        let revision = self
            .workflows
            .find_revision(
                request.organization_id,
                request.workflow.workflow_definition_id,
                request.workflow.workflow_revision_id,
            )
            .await?
            .filter(|revision| revision.project_id == request.project_id)
            .ok_or_else(|| {
                ApplicationError::NotFound("Application WorkflowRevision not found".into())
            })?;
        let revision_evidence = application_workflow_revision_evidence(&revision)?;
        request
            .workflow
            .validate_evidence(
                request.organization_id,
                request.project_id,
                &revision_evidence,
            )
            .map_err(ApplicationError::Conflict)?;
        let ontology = self
            .ontologies
            .find_revision(
                request.organization_id,
                request.ontology_id,
                request.ontology_revision_id,
            )
            .await?
            .filter(|ontology| ontology.project_id == request.project_id)
            .ok_or_else(|| {
                ApplicationError::NotFound("Application OntologyRevision not found".into())
            })?;
        if ontology.contract.digest() != &request.ontology_digest {
            return Err(ApplicationError::Conflict(
                "Application OntologyRevision digest drifted".into(),
            ));
        }
        let definition = definition
            .at_revision(&revision)
            .map_err(ApplicationError::Conflict)?;
        let contract = WorkflowGoalContract::from_spec(WorkflowGoalSpec {
            name: format!("Application invocation {}", request.invocation_id),
            workflow_definition_id: request.workflow.workflow_definition_id,
            workflow_revision_id: request.workflow.workflow_revision_id,
            workflow_digest: request.workflow.workflow_contract_digest.clone(),
            ontology_id: request.ontology_id,
            ontology_revision_id: request.ontology_revision_id,
            ontology_digest: request.ontology_digest.clone(),
            environment_id: request.environment_id,
            input: request.input.clone(),
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
        let expected = WorkflowGoalRecord {
            goal: compiled.goal,
            plan_revision: compiled.plan_revision,
        };
        let request_id = request.request_id(b"goal-request");
        let event =
            WorkflowGoalCompiled::envelope(&expected.goal, &expected.plan_revision, request_id)
                .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let write = self
            .goals
            .create(CreateWorkflowGoalWrite {
                record: expected.clone(),
                event,
                actor_principal_id: request.requested_by,
                request_id,
                idempotency: Self::goal_idempotency(request)?,
            })
            .await?;
        if write.value != expected {
            return Err(ApplicationError::Conflict(
                "Application WorkflowGoal replay authority drifted".into(),
            ));
        }
        Ok((write.value, revision))
    }

    fn validate_record(
        request: &ApplicationWorkflowRunRequest,
        record: &WorkflowRunRecord,
    ) -> ApplicationResult<()> {
        record.validate().map_err(ApplicationError::Internal)?;
        let run = &record.run;
        let input = &run.execution_input;
        let plan = &input.plan;
        let timeout_seconds = workflow_run_timeout_seconds(Some(request.timeout_seconds))
            .map_err(ApplicationError::Invalid)?;
        let timeout = chrono::Duration::seconds(i64::try_from(timeout_seconds).map_err(|_| {
            ApplicationError::Invalid("Application WorkflowRun timeout is unsupported".into())
        })?);
        let deadline_at = request
            .requested_at
            .checked_add_signed(timeout)
            .ok_or_else(|| {
                ApplicationError::Invalid("Application WorkflowRun deadline overflowed".into())
            })?;
        let inputs = plan
            .steps
            .iter()
            .filter(|step| step.kind == WorkflowStepKind::Input)
            .collect::<Vec<_>>();
        let outputs = plan
            .steps
            .iter()
            .filter(|step| step.kind == WorkflowStepKind::Output)
            .collect::<Vec<_>>();
        let ([input_step], [output_step]) = (inputs.as_slice(), outputs.as_slice()) else {
            return Err(ApplicationError::Internal(
                "Application WorkflowRun lost its single Input or Output step".into(),
            ));
        };
        let application_projection = input.application_projection.as_ref().ok_or_else(|| {
            ApplicationError::Internal(
                "Application WorkflowRun lost its immutable projection contract".into(),
            )
        })?;
        application_projection
            .validate(plan)
            .map_err(ApplicationError::Internal)?;
        if run.organization_id != request.organization_id
            || run.project_id != request.project_id
            || run.id != request.workflow_run_id()
            || run.workflow_goal_id != request.workflow_goal_id()
            || run.plan_revision_id != request.plan_revision_id()
            || run.requested_by != request.requested_by
            || run.requested_at != request.requested_at
            || input.workflow_run_id != request.workflow_run_id()
            || input.workflow_goal_id != request.workflow_goal_id()
            || input.plan_revision_id != request.plan_revision_id()
            || input.deadline_at != deadline_at
            || input.schema != WORKFLOW_RUN_INPUT_SCHEMA_V10
            || input.runtime_contract_revision != WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10
            || input.flow_workflow_version != WORKFLOW_RUN_FLOW_VERSION_V10
            || input.goal_input != request.input
            || plan.input_digest != request.input_digest
            || plan.workflow_definition_id != request.workflow.workflow_definition_id
            || plan.workflow_revision_id != request.workflow.workflow_revision_id
            || plan.workflow_digest != request.workflow.workflow_contract_digest
            || plan.workflow_payload_set_digest != request.workflow.workflow_payload_set_digest
            || plan.semantic_contract_set_digest.as_ref()
                != Some(&request.workflow.workflow_semantic_contract_set_digest)
            || input_step.output_schema_digest != request.workflow.input_schema_digest
            || output_step.output_schema_digest != request.workflow.output_schema_digest
            || application_projection.final_output_step_id != output_step.id
            || plan.ontology_id != request.ontology_id
            || plan.ontology_revision_id != request.ontology_revision_id
            || plan.ontology_digest != request.ontology_digest
            || plan.environment_id != request.environment_id
        {
            return Err(ApplicationError::Conflict(
                "Application WorkflowRun authority drifted".into(),
            ));
        }
        Ok(())
    }

    fn evidence(
        request: &ApplicationWorkflowRunRequest,
        record: &WorkflowRunRecord,
    ) -> ApplicationResult<ApplicationWorkflowRunEvidence> {
        Self::validate_record(request, record)?;
        let evidence = ApplicationWorkflowRunEvidence {
            organization_id: request.organization_id,
            project_id: request.project_id,
            application_id: request.application_id,
            application_release_id: request.application_release_id,
            application_release_digest: request.application_release_digest.clone(),
            session_id: request.session_id,
            invocation_id: request.invocation_id,
            workflow_run_id: record.run.id,
            workflow_goal_id: record.run.workflow_goal_id,
            plan_revision_id: record.run.plan_revision_id,
            plan_digest: record.run.plan_digest.clone(),
            workflow: request.workflow.clone(),
            ontology_id: request.ontology_id,
            ontology_revision_id: request.ontology_revision_id,
            ontology_digest: request.ontology_digest.clone(),
            environment_id: request.environment_id,
            input_digest: request.input_digest.clone(),
            requested_by: record.run.requested_by,
            requested_at: record.run.requested_at,
            deadline_at: record.run.execution_input.deadline_at,
        };
        evidence
            .validate_against(request)
            .map_err(ApplicationError::Internal)?;
        Ok(evidence)
    }
}

#[async_trait]
impl IApplicationWorkflowRunPort for WorkflowApplicationRunService {
    async fn start_or_adopt(
        &self,
        request: &ApplicationWorkflowRunRequest,
    ) -> ApplicationResult<ApplicationWorkflowRunEvidence> {
        request.validate().map_err(ApplicationError::Invalid)?;
        workflow_run_timeout_seconds(Some(request.timeout_seconds))
            .map_err(ApplicationError::Invalid)?;
        if let Some(record) = self.adopt_record(request).await? {
            return Self::evidence(request, &record);
        }
        let (goal, revision) = self.compile_goal(request).await?;
        let compiled = WorkflowRunCompiler::compile_for_application(
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
        Self::validate_record(request, &record)?;
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
                idempotency: Self::run_idempotency(request)?,
            })
            .await?;
        Self::evidence(request, &write.value)
    }

    async fn request_cancellation(
        &self,
        request: &ApplicationWorkflowRunRequest,
        reason: &str,
        requested_at: DateTime<Utc>,
    ) -> ApplicationResult<Option<ApplicationWorkflowRunEvidence>> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let Some(mut record) = self.adopt_record(request).await? else {
            return Ok(None);
        };
        if record.run.status.is_terminal() {
            return Self::evidence(request, &record).map(Some);
        }
        if record.run.status == WorkflowRunStatus::Cancelling {
            if record.run.cancellation_reason.as_deref() == Some(reason)
                && record.run.cancellation_requested_by == Some(request.requested_by)
            {
                return Self::evidence(request, &record).map(Some);
            }
            return Err(ApplicationError::Conflict(
                "Application WorkflowRun cancellation authority drifted".into(),
            ));
        }
        let cancellation_body = canonical_json_bounded(
            &serde_json::json!({
                "applicationRequestDigest": Sha256Digest::from_bytes(&request.canonical_bytes().map_err(ApplicationError::Invalid)?),
                "reason": reason,
                "requestedBy": request.requested_by,
            }),
            32 * 1024,
            "Application WorkflowRun cancellation",
        )
        .map_err(ApplicationError::Invalid)?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/workflow-runs/{}/cancellation",
                request.organization_id,
                request.workflow_run_id()
            ),
            format!("application:{}", request.invocation_id),
            &cancellation_body,
        )
        .map_err(ApplicationError::Invalid)?;
        if let Some(replayed) = self.runs.replay(&idempotency).await? {
            return Self::evidence(request, &replayed).map(Some);
        }
        let effective_at = std::cmp::max(requested_at, record.run.updated_at);
        let expected_version = record.run.aggregate_version;
        record
            .run
            .request_cancellation(Some(reason.to_owned()), request.requested_by, effective_at)
            .map_err(ApplicationError::Conflict)?;
        let request_id = request.request_id(b"cancel-request");
        let event = WorkflowRunCancellationRequested::envelope(&record.run, request_id)
            .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let write = self
            .runs
            .request_cancellation(CancelWorkflowRunWrite {
                record,
                expected_version,
                event,
                actor_principal_id: request.requested_by,
                request_id,
                idempotency,
            })
            .await?;
        Self::evidence(request, &write.value).map(Some)
    }
}
