use super::SubmitHumanTask;
use crate::modules::forms::domain::{
    AcceptedFormSubmission, FormSubmission, IFormRepository, IFormSemanticCore,
};
use crate::modules::identity::domain::repositories::IResourceAuthorizationDecisionRepository;
use crate::modules::identity::domain::services::ResourceAuthorizationDecisionRequest;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    FormId, FormReleaseId, FormSubmissionId, IdempotencyRequest, WorkflowDecisionId,
};
use crate::modules::workflow::application::{
    human_task_access, resource_access, HumanTaskMutationResult,
};
use crate::modules::workflow::domain::{
    DecideHumanTaskWrite, FlowResumePayload, HumanTaskDecisionRecord, HumanTaskStateChanged,
    IHumanTaskRepository, WorkflowDecision,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use a3s_form_core::{
    canonicalize_json, parse_json, EvaluateRequest, EvaluationOptions,
    EVALUATE_REQUEST_API_VERSION, EVALUATE_RESPONSE_API_VERSION,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EvaluationResponseEnvelope {
    api_version: String,
    compiler_revision: String,
    ok: bool,
    value: Option<a3s_form_core::CanonicalValue>,
    #[serde(rename = "trace")]
    _trace: Vec<serde_json::Value>,
    errors: Vec<EvaluationErrorEnvelope>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvaluationErrorEnvelope {
    path: String,
    code: String,
    message: String,
}

pub struct SubmitHumanTaskHandler {
    human_tasks: Arc<dyn IHumanTaskRepository>,
    forms: Arc<dyn IFormRepository>,
    semantic_core: Arc<dyn IFormSemanticCore>,
    authorization_decisions: Arc<dyn IResourceAuthorizationDecisionRepository>,
}

impl SubmitHumanTaskHandler {
    pub fn new(
        human_tasks: Arc<dyn IHumanTaskRepository>,
        forms: Arc<dyn IFormRepository>,
        semantic_core: Arc<dyn IFormSemanticCore>,
        authorization_decisions: Arc<dyn IResourceAuthorizationDecisionRepository>,
    ) -> Self {
        Self {
            human_tasks,
            forms,
            semantic_core,
            authorization_decisions,
        }
    }
}

impl CommandHandler<SubmitHumanTask> for SubmitHumanTaskHandler {
    fn execute(
        &self,
        command: SubmitHumanTask,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<HumanTaskMutationResult>>>
    {
        let human_tasks = Arc::clone(&self.human_tasks);
        let forms = Arc::clone(&self.forms);
        let semantic_core = Arc::clone(&self.semantic_core);
        let authorization_decisions = Arc::clone(&self.authorization_decisions);
        Box::pin(async move {
            let mut record = match resource_access::human_task(
                human_tasks.as_ref(),
                command.organization_id,
                command.human_task_id,
                &command.resource_access,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            if let Err(error) = human_task_access::ensure_supported_assignment_policy(&record) {
                return Ok(Err(error));
            }
            if command.submission.principal_id != command.actor_principal_id.to_string() {
                return Ok(Err(ApplicationError::Forbidden(
                    "Form submission principal does not match the authenticated principal".into(),
                )));
            }
            if let Err(error) = command.submission.validate() {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "Form interaction submission is invalid: {error}"
                ))));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "humanTaskId": command.human_task_id,
                "submission": &command.submission,
                "actorPrincipalId": command.actor_principal_id,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/human-tasks/{}/submission",
                    command.organization_id, command.human_task_id
                ),
                command.submission.idempotency_key.clone(),
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if let Some(replay) = match human_tasks.replay_decision(&idempotency).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            } {
                return Ok(human_task_access::public_record(
                    replay.task,
                    Some(command.actor_principal_id),
                )
                .map(|record| HumanTaskMutationResult {
                    record,
                    replayed: true,
                }));
            }
            if let Err(error) =
                human_task_access::ensure_current_claimant(&record, command.actor_principal_id)
            {
                return Ok(Err(error));
            }
            let request = match record.interaction_request.clone() {
                Some(request) => request,
                None => {
                    return Ok(Err(ApplicationError::Conflict(
                        "HumanTask has no active Form interaction request".into(),
                    )))
                }
            };
            let form_release = match find_exact_form_release(forms.as_ref(), &record).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let accepted_value = match evaluate_submission(
                semantic_core,
                form_release.content.form_plan_json(),
                &command.submission.value,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let authorization_reference = match authorization_decisions
                .authorize_resource(ResourceAuthorizationDecisionRequest {
                    organization_id: command.organization_id,
                    principal_id: command.actor_principal_id,
                    credential_id: command.credential_id,
                    actor_is_platform_admin: command.actor_is_platform_admin,
                    required_scope: crate::modules::identity::domain::value_objects::ApiTokenScope::parse(
                        crate::modules::identity::domain::value_objects::ApiTokenScope::WORKFLOW_WRITE,
                    )
                    .map_err(BootError::Internal)?,
                    action: "workflow.human-task.submit".into(),
                    resource: ResourceGrantScope::Project {
                        project_id: record.task.project_id,
                    },
                    request_id: command.request_id,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            let submission_id = match Uuid::parse_str(&command.submission.submission_id) {
                Ok(value) => FormSubmissionId::from_uuid(value),
                Err(error) => {
                    return Ok(Err(ApplicationError::Invalid(format!(
                        "Form submission ID is invalid: {error}"
                    ))))
                }
            };
            let submission = match FormSubmission::accept(AcceptedFormSubmission {
                organization_id: command.organization_id,
                project_id: record.task.project_id,
                id: submission_id,
                workflow_run_id: record.task.workflow_run_id,
                human_task_id: record.task.id,
                principal_id: command.actor_principal_id,
                authorization_decision: authorization_reference,
                request,
                submission: command.submission,
                accepted_value,
                accepted_at: command.requested_at,
            }) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let output = match submission.accepted_output() {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Internal(error))),
            };
            let decision = match WorkflowDecision::from_submission(
                WorkflowDecisionId::new(),
                &record.task,
                &submission,
                output,
                command.requested_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            let expected_version = record.task.aggregate_version;
            if let Err(error) = record.complete(expected_version, &decision) {
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            let resume_payload =
                FlowResumePayload::from_decision(&decision).map_err(BootError::Internal)?;
            let event = HumanTaskStateChanged::envelope(&record, Some(command.request_id))
                .map_err(|error| BootError::Internal(error.to_string()))?;
            match human_tasks
                .decide_task(DecideHumanTaskWrite {
                    record: HumanTaskDecisionRecord {
                        task: record,
                        submission: Some(submission),
                        decision,
                        resume_payload,
                        resume_receipt: None,
                    },
                    expected_version,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(write) => Ok(human_task_access::public_record(
                    write.value.task,
                    Some(command.actor_principal_id),
                )
                .map(|record| HumanTaskMutationResult {
                    record,
                    replayed: write.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

async fn find_exact_form_release(
    forms: &dyn IFormRepository,
    record: &crate::modules::workflow::domain::HumanTaskRecord,
) -> ApplicationResult<crate::modules::forms::domain::FormRelease> {
    let form_id = Uuid::parse_str(&record.task.form_release.form_id)
        .map(FormId::from_uuid)
        .map_err(|_| ApplicationError::Internal("HumanTask Form identity is invalid".into()))?;
    let release_id = Uuid::parse_str(&record.task.form_release.release_id)
        .map(FormReleaseId::from_uuid)
        .map_err(|_| {
            ApplicationError::Internal("HumanTask Form release identity is invalid".into())
        })?;
    let release = forms
        .find_release(record.task.organization_id, form_id, release_id)
        .await
        .map_err(ApplicationError::from)?
        .ok_or_else(|| {
            ApplicationError::Conflict("HumanTask Form release is unavailable".into())
        })?;
    if release.project_id != record.task.project_id
        || release.release_ref().map_err(ApplicationError::Internal)? != record.task.form_release
    {
        return Err(ApplicationError::Conflict(
            "HumanTask Form release authority drifted".into(),
        ));
    }
    Ok(release)
}

async fn evaluate_submission(
    semantic_core: Arc<dyn IFormSemanticCore>,
    form_plan_json: &str,
    candidate: &a3s_form_core::CanonicalValue,
) -> ApplicationResult<a3s_form_core::CanonicalValue> {
    let form_plan = parse_json(form_plan_json.as_bytes()).map_err(|error| {
        ApplicationError::Internal(format!("stored Form plan could not be decoded: {error}"))
    })?;
    let request = EvaluateRequest {
        api_version: EVALUATE_REQUEST_API_VERSION.into(),
        form_plan,
        value: candidate.clone(),
        options: EvaluationOptions::default(),
    };
    let request = serde_json::to_vec(&request).map_err(|error| {
        ApplicationError::Internal(format!("Form evaluation request failed: {error}"))
    })?;
    let request = canonicalize_json(&request).map_err(|error| {
        ApplicationError::Internal(format!("Form evaluation request is not canonical: {error}"))
    })?;
    let expected_compiler_revision = semantic_core.compiler_revision();
    let response = tokio::task::spawn_blocking(move || semantic_core.evaluate(&request))
        .await
        .map_err(|error| {
            ApplicationError::Internal(format!("Form evaluator task failed: {error}"))
        })?
        .map_err(|error| ApplicationError::Unavailable(error.to_string()))?;
    let response: EvaluationResponseEnvelope =
        serde_json::from_slice(&response).map_err(|error| {
            ApplicationError::Internal(format!("Form evaluator response is invalid JSON: {error}"))
        })?;
    if response.api_version != EVALUATE_RESPONSE_API_VERSION
        || response.compiler_revision != expected_compiler_revision
    {
        return Err(ApplicationError::Internal(
            "Form evaluator returned an incompatible protocol identity".into(),
        ));
    }
    if !response.ok {
        return Err(ApplicationError::Invalid(evaluation_failure(&response)));
    }
    response.value.ok_or_else(|| {
        ApplicationError::Internal("successful Form evaluation omitted the accepted value".into())
    })
}

fn evaluation_failure(response: &EvaluationResponseEnvelope) -> String {
    let Some(error) = response.errors.first() else {
        return "Form submission evaluation failed without a diagnostic".into();
    };
    format!(
        "Form submission evaluation failed ({}) at {}: {}",
        error.code, error.path, error.message
    )
}
