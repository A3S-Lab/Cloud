use super::{ChangeHumanTaskAssignment, HumanTaskAssignmentAction};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use crate::modules::workflow::application::{
    human_task_access, resource_access, HumanTaskMutationResult,
};
use crate::modules::workflow::domain::{
    ChangeHumanTaskWrite, HumanTaskStateChanged, IHumanTaskRepository,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct ChangeHumanTaskAssignmentHandler {
    human_tasks: Arc<dyn IHumanTaskRepository>,
}

impl ChangeHumanTaskAssignmentHandler {
    pub fn new(human_tasks: Arc<dyn IHumanTaskRepository>) -> Self {
        Self { human_tasks }
    }
}

impl CommandHandler<ChangeHumanTaskAssignment> for ChangeHumanTaskAssignmentHandler {
    fn execute(
        &self,
        command: ChangeHumanTaskAssignment,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<HumanTaskMutationResult>>>
    {
        let human_tasks = Arc::clone(&self.human_tasks);
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
            if command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "HumanTask expected version must be positive".into(),
                )));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "humanTaskId": command.human_task_id,
                "action": command.action.as_str(),
                "expectedVersion": command.expected_version,
                "actorPrincipalId": command.actor_principal_id,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/human-tasks/{}/{}",
                    command.organization_id,
                    command.human_task_id,
                    command.action.as_str()
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if let Some(record) = match human_tasks.replay_change(&idempotency).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            } {
                return Ok(human_task_access::public_record(
                    record,
                    Some(command.actor_principal_id),
                )
                .map(|record| HumanTaskMutationResult {
                    record,
                    replayed: true,
                }));
            }
            let transition = match command.action {
                HumanTaskAssignmentAction::Claim => record.claim(
                    command.expected_version,
                    command.actor_principal_id,
                    command.requested_at,
                ),
                HumanTaskAssignmentAction::Release => record.release(
                    command.expected_version,
                    command.actor_principal_id,
                    command.requested_at,
                ),
            };
            if let Err(error) = transition {
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            let event = HumanTaskStateChanged::envelope(&record, Some(command.request_id))
                .map_err(|error| BootError::Internal(error.to_string()))?;
            match human_tasks
                .change_task(ChangeHumanTaskWrite {
                    record,
                    expected_version: command.expected_version,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(write) => Ok(human_task_access::public_record(
                    write.value,
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
