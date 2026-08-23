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
    WorkflowStepKind, WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA,
    WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V2, WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3,
    WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5, WORKFLOW_RUN_FLOW_VERSION_V10,
    WORKFLOW_RUN_FLOW_VERSION_V11, WORKFLOW_RUN_FLOW_VERSION_V12, WORKFLOW_RUN_FLOW_VERSION_V13,
    WORKFLOW_RUN_FLOW_VERSION_V14, WORKFLOW_RUN_INPUT_SCHEMA_V10, WORKFLOW_RUN_INPUT_SCHEMA_V11,
    WORKFLOW_RUN_INPUT_SCHEMA_V12, WORKFLOW_RUN_INPUT_SCHEMA_V13, WORKFLOW_RUN_INPUT_SCHEMA_V14,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V11,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V12, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V13,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V14,
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
        let [input_step] = inputs.as_slice() else {
            return Err(ApplicationError::Internal(
                "Application WorkflowRun lost its single Input step".into(),
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
        let output_step = plan
            .steps
            .iter()
            .find(|step| step.id == application_projection.final_output_step_id)
            .ok_or_else(|| {
                ApplicationError::Internal(
                    "Application WorkflowRun lost its final Output step".into(),
                )
            })?;
        let version_matches = matches!(
            (
                application_projection.schema.as_str(),
                input.schema.as_str(),
                input.runtime_contract_revision.as_str(),
                input.flow_workflow_version.as_str(),
            ),
            (
                WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA,
                WORKFLOW_RUN_INPUT_SCHEMA_V10,
                WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10,
                WORKFLOW_RUN_FLOW_VERSION_V10,
            ) | (
                WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V2,
                WORKFLOW_RUN_INPUT_SCHEMA_V11,
                WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V11,
                WORKFLOW_RUN_FLOW_VERSION_V11,
            ) | (
                WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3,
                WORKFLOW_RUN_INPUT_SCHEMA_V12,
                WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V12,
                WORKFLOW_RUN_FLOW_VERSION_V12,
            ) | (
                WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5,
                WORKFLOW_RUN_INPUT_SCHEMA_V13,
                WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V13,
                WORKFLOW_RUN_FLOW_VERSION_V13,
            ) | (
                WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3,
                WORKFLOW_RUN_INPUT_SCHEMA_V14,
                WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V14,
                WORKFLOW_RUN_FLOW_VERSION_V14,
            ) | (
                WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5,
                WORKFLOW_RUN_INPUT_SCHEMA_V14,
                WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V14,
                WORKFLOW_RUN_FLOW_VERSION_V14,
            )
        );
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
            || !version_matches
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::applications::ApplicationWorkflowBinding;
    use crate::modules::shared_kernel::domain::{
        canonical_timestamp, ApplicationId, ApplicationInvocationId, ApplicationReleaseId,
        ApplicationSessionId, PrincipalId,
    };
    use crate::modules::workflow::domain::{WorkflowRun, WorkflowRunRecord};
    use crate::modules::workflow::test_support::{
        application_answer_workflow_run_input, application_frame_answer_workflow_run_input,
        application_variable_workflow_run_input,
    };
    use chrono::Duration;

    #[test]
    fn production_adapter_accepts_the_exact_v11_answer_record_and_rejects_version_aliases() {
        let mut input =
            application_answer_workflow_run_input().expect("Application Answer WorkflowRun input");
        let requested_at = canonical_timestamp(Utc::now());
        let requested_by = PrincipalId::new();
        let final_output_step_id = input
            .application_projection
            .as_ref()
            .expect("Application projection")
            .final_output_step_id
            .clone();
        let input_schema_digest = input
            .plan
            .steps
            .iter()
            .find(|step| step.kind == WorkflowStepKind::Input)
            .expect("Input step")
            .output_schema_digest
            .clone();
        let output_schema_digest = input
            .plan
            .steps
            .iter()
            .find(|step| step.id == final_output_step_id)
            .expect("final Output step")
            .output_schema_digest
            .clone();
        let request = ApplicationWorkflowRunRequest {
            organization_id: input.organization_id,
            project_id: input.project_id,
            application_id: ApplicationId::new(),
            application_release_id: ApplicationReleaseId::new(),
            application_release_digest: Sha256Digest::from_bytes(b"application-release"),
            session_id: ApplicationSessionId::new(),
            invocation_id: ApplicationInvocationId::new(),
            workflow: ApplicationWorkflowBinding {
                workflow_definition_id: input.plan.workflow_definition_id,
                workflow_revision_id: input.plan.workflow_revision_id,
                workflow_contract_digest: input.plan.workflow_digest.clone(),
                workflow_payload_set_digest: input.plan.workflow_payload_set_digest.clone(),
                workflow_semantic_contract_set_digest: input
                    .plan
                    .semantic_contract_set_digest
                    .clone()
                    .expect("semantic contract set digest"),
                input_schema_digest,
                output_schema_digest,
            },
            ontology_id: input.plan.ontology_id,
            ontology_revision_id: input.plan.ontology_revision_id,
            ontology_digest: input.plan.ontology_digest.clone(),
            environment_id: input.plan.environment_id,
            input: input.goal_input.clone(),
            input_digest: input.plan.input_digest.clone(),
            requested_by,
            requested_at,
            timeout_seconds: 3_600,
        };
        request.validate().expect("valid Application request");
        input.workflow_run_id = request.workflow_run_id();
        input.workflow_goal_id = request.workflow_goal_id();
        input.plan_revision_id = request.plan_revision_id();
        input.requested_at = requested_at;
        input.deadline_at = requested_at + Duration::hours(1);
        let (run, steps) = WorkflowRun::create(input, requested_by).expect("v11 WorkflowRun");
        let record = WorkflowRunRecord { run, steps };

        WorkflowApplicationRunService::validate_record(&request, &record)
            .expect("production adapter accepts v11 Answer authority");

        let mut aliased = record;
        aliased.run.execution_input.schema = WORKFLOW_RUN_INPUT_SCHEMA_V10.into();
        assert!(WorkflowApplicationRunService::validate_record(&request, &aliased).is_err());
    }

    #[test]
    fn production_adapter_accepts_exact_v12_variable_authority_and_rejects_v11_alias() {
        let mut input = application_variable_workflow_run_input()
            .expect("Application variable WorkflowRun input");
        let requested_at = canonical_timestamp(Utc::now());
        let requested_by = PrincipalId::new();
        let final_output_step_id = input
            .application_projection
            .as_ref()
            .expect("Application projection")
            .final_output_step_id
            .clone();
        let input_schema_digest = input
            .plan
            .steps
            .iter()
            .find(|step| step.kind == WorkflowStepKind::Input)
            .expect("Input step")
            .output_schema_digest
            .clone();
        let output_schema_digest = input
            .plan
            .steps
            .iter()
            .find(|step| step.id == final_output_step_id)
            .expect("final Output step")
            .output_schema_digest
            .clone();
        let request = ApplicationWorkflowRunRequest {
            organization_id: input.organization_id,
            project_id: input.project_id,
            application_id: ApplicationId::new(),
            application_release_id: ApplicationReleaseId::new(),
            application_release_digest: Sha256Digest::from_bytes(b"application-release"),
            session_id: ApplicationSessionId::new(),
            invocation_id: ApplicationInvocationId::new(),
            workflow: ApplicationWorkflowBinding {
                workflow_definition_id: input.plan.workflow_definition_id,
                workflow_revision_id: input.plan.workflow_revision_id,
                workflow_contract_digest: input.plan.workflow_digest.clone(),
                workflow_payload_set_digest: input.plan.workflow_payload_set_digest.clone(),
                workflow_semantic_contract_set_digest: input
                    .plan
                    .semantic_contract_set_digest
                    .clone()
                    .expect("semantic contract set digest"),
                input_schema_digest,
                output_schema_digest,
            },
            ontology_id: input.plan.ontology_id,
            ontology_revision_id: input.plan.ontology_revision_id,
            ontology_digest: input.plan.ontology_digest.clone(),
            environment_id: input.plan.environment_id,
            input: input.goal_input.clone(),
            input_digest: input.plan.input_digest.clone(),
            requested_by,
            requested_at,
            timeout_seconds: 3_600,
        };
        request.validate().expect("valid Application request");
        input.workflow_run_id = request.workflow_run_id();
        input.workflow_goal_id = request.workflow_goal_id();
        input.plan_revision_id = request.plan_revision_id();
        input.requested_at = requested_at;
        input.deadline_at = requested_at + Duration::hours(1);
        let (run, steps) = WorkflowRun::create(input, requested_by).expect("v12 WorkflowRun");
        let record = WorkflowRunRecord { run, steps };

        WorkflowApplicationRunService::validate_record(&request, &record)
            .expect("production adapter accepts v12 Application variable authority");

        let mut aliased = record;
        aliased.run.execution_input.schema = WORKFLOW_RUN_INPUT_SCHEMA_V11.into();
        aliased.run.execution_input.runtime_contract_revision =
            WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V11.into();
        aliased.run.execution_input.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V11.into();
        assert!(WorkflowApplicationRunService::validate_record(&request, &aliased).is_err());
    }

    #[test]
    fn production_adapter_accepts_exact_v13_composite_authority_and_rejects_v12_alias() {
        let (mut input, _, _) = application_frame_answer_workflow_run_input(0)
            .expect("Application composite WorkflowRun input");
        let requested_at = canonical_timestamp(Utc::now());
        let requested_by = PrincipalId::new();
        let final_output_step_id = input
            .application_projection
            .as_ref()
            .expect("Application projection")
            .final_output_step_id
            .clone();
        let input_schema_digest = input
            .plan
            .steps
            .iter()
            .find(|step| step.kind == WorkflowStepKind::Input)
            .expect("Input step")
            .output_schema_digest
            .clone();
        let output_schema_digest = input
            .plan
            .steps
            .iter()
            .find(|step| step.id == final_output_step_id)
            .expect("final Output step")
            .output_schema_digest
            .clone();
        let request = ApplicationWorkflowRunRequest {
            organization_id: input.organization_id,
            project_id: input.project_id,
            application_id: ApplicationId::new(),
            application_release_id: ApplicationReleaseId::new(),
            application_release_digest: Sha256Digest::from_bytes(b"application-release"),
            session_id: ApplicationSessionId::new(),
            invocation_id: ApplicationInvocationId::new(),
            workflow: ApplicationWorkflowBinding {
                workflow_definition_id: input.plan.workflow_definition_id,
                workflow_revision_id: input.plan.workflow_revision_id,
                workflow_contract_digest: input.plan.workflow_digest.clone(),
                workflow_payload_set_digest: input.plan.workflow_payload_set_digest.clone(),
                workflow_semantic_contract_set_digest: input
                    .plan
                    .semantic_contract_set_digest
                    .clone()
                    .expect("semantic contract set digest"),
                input_schema_digest,
                output_schema_digest,
            },
            ontology_id: input.plan.ontology_id,
            ontology_revision_id: input.plan.ontology_revision_id,
            ontology_digest: input.plan.ontology_digest.clone(),
            environment_id: input.plan.environment_id,
            input: input.goal_input.clone(),
            input_digest: input.plan.input_digest.clone(),
            requested_by,
            requested_at,
            timeout_seconds: 3_600,
        };
        request.validate().expect("valid Application request");
        input.workflow_run_id = request.workflow_run_id();
        input.workflow_goal_id = request.workflow_goal_id();
        input.plan_revision_id = request.plan_revision_id();
        input.requested_at = requested_at;
        input.deadline_at = requested_at + Duration::hours(1);
        let (run, steps) = WorkflowRun::create(input, requested_by).expect("v13 WorkflowRun");
        let record = WorkflowRunRecord { run, steps };

        WorkflowApplicationRunService::validate_record(&request, &record)
            .expect("production adapter accepts v13 composite authority");

        let mut aliased = record;
        aliased.run.execution_input.schema = WORKFLOW_RUN_INPUT_SCHEMA_V12.into();
        aliased.run.execution_input.runtime_contract_revision =
            WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V12.into();
        aliased.run.execution_input.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V12.into();
        assert!(WorkflowApplicationRunService::validate_record(&request, &aliased).is_err());
    }
}
