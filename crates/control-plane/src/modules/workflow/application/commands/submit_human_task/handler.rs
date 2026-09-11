use super::SubmitHumanTask;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    FormSubmissionId, IdempotencyRequest, WorkflowDecisionId,
};
use crate::modules::workflow::application::{
    HumanTaskFormEvaluation, HumanTaskMutationResult, HumanTaskSubmissionAuthorization,
    IHumanTaskAuthorizationPort, IHumanTaskFormPort, human_task_access, resource_access,
};
use crate::modules::workflow::domain::{
    AcceptedHumanTaskSubmission, DecideHumanTaskWrite, FlowResumePayload, HumanTaskDecisionRecord,
    HumanTaskStateChanged, HumanTaskSubmission, IHumanTaskRepository, WorkflowDecision,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;
use uuid::Uuid;

pub struct SubmitHumanTaskHandler {
    human_tasks: Arc<dyn IHumanTaskRepository>,
    forms: Arc<dyn IHumanTaskFormPort>,
    authorization: Arc<dyn IHumanTaskAuthorizationPort>,
}

impl SubmitHumanTaskHandler {
    pub fn new(
        human_tasks: Arc<dyn IHumanTaskRepository>,
        forms: Arc<dyn IHumanTaskFormPort>,
        authorization: Arc<dyn IHumanTaskAuthorizationPort>,
    ) -> Self {
        Self {
            human_tasks,
            forms,
            authorization,
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
        let authorization = Arc::clone(&self.authorization);
        Box::pin(async move {
            let mut record = match resource_access::human_task(
                human_tasks.as_ref(),
                command.organization_id,
                command.human_task_id,
                &command.access,
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
                    )));
                }
            };
            let accepted_value = match forms
                .evaluate_submission(&HumanTaskFormEvaluation {
                    organization_id: command.organization_id,
                    project_id: record.task.project_id,
                    form_release: record.task.form_release.clone(),
                    candidate: command.submission.value.clone(),
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let authorization_reference = match authorization
                .authorize_submission(HumanTaskSubmissionAuthorization {
                    organization_id: command.organization_id,
                    project_id: record.task.project_id,
                    principal_id: command.actor_principal_id,
                    credential_id: command.credential_id,
                    request_id: command.request_id,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let submission_id = match Uuid::parse_str(&command.submission.submission_id) {
                Ok(value) => FormSubmissionId::from_uuid(value),
                Err(error) => {
                    return Ok(Err(ApplicationError::Invalid(format!(
                        "Form submission ID is invalid: {error}"
                    ))));
                }
            };
            let submission = match HumanTaskSubmission::accept(AcceptedHumanTaskSubmission {
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
