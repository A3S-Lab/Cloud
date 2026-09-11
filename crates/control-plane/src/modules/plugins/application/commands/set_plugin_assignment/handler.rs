use super::{SetPluginAssignment, SetPluginAssignmentResult};
use crate::modules::plugins::domain::entities::{NewPluginAssignment, PluginAssignment};
use crate::modules::plugins::domain::events::PluginAssignmentChanged;
use crate::modules::plugins::domain::repositories::{
    CreatePluginAssignmentWrite, IPluginAssignmentRepository, UpdatePluginAssignmentWrite,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{OperationId, PluginAssignmentId, RepositoryError};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct SetPluginAssignmentHandler {
    assignments: Arc<dyn IPluginAssignmentRepository>,
}

impl SetPluginAssignmentHandler {
    pub fn new(assignments: Arc<dyn IPluginAssignmentRepository>) -> Self {
        Self { assignments }
    }
}

impl CommandHandler<SetPluginAssignment> for SetPluginAssignmentHandler {
    fn execute(
        &self,
        command: SetPluginAssignment,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<SetPluginAssignmentResult>>>
    {
        let assignments = Arc::clone(&self.assignments);
        Box::pin(async move {
            if !command
                .access
                .environment_is_visible(command.project_id, command.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "plugin assignments not found".into(),
                )));
            }
            if let Err(error) = command.selection.validate() {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            if command.workspace_scope.validate().is_err() {
                return Ok(Err(ApplicationError::Invalid(
                    "plugin assignment workspace scope is invalid".into(),
                )));
            }

            let live = match assignments
                .find_live_for_host_package(
                    command.organization_id,
                    command.target_host_id,
                    &command.selection.package_id,
                )
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };

            let create_probe = match PluginAssignment::create(NewPluginAssignment {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                id: PluginAssignmentId::new(),
                registry_id: command.registry_id,
                target_host_id: command.target_host_id,
                workspace_scope: command.workspace_scope.clone(),
                selection: command.selection.clone(),
                policy_digest: command.policy_digest.clone(),
                desired_state: command.desired_state,
                actor_id: command.actor_id,
                request_id: command.request_id,
                operation_id: OperationId::new(),
                created_at: command.requested_at,
            }) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let create_idempotency = match CreatePluginAssignmentWrite::idempotency_for(
                &create_probe,
                command.idempotency_key.clone(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let create_event = PluginAssignmentChanged::envelope(&create_probe)
                .map_err(|error| BootError::Internal(error.to_string()))?;

            match assignments
                .create(CreatePluginAssignmentWrite {
                    assignment: create_probe,
                    event: create_event,
                    idempotency: create_idempotency,
                })
                .await
            {
                Ok(write) => {
                    return Ok(Ok(SetPluginAssignmentResult {
                        assignment: write.value,
                        replayed: write.replayed,
                    }));
                }
                Err(RepositoryError::Conflict(_)) if live.is_some() => {}
                Err(error) => return Ok(Err(error.into())),
            }

            let existing = live.expect("host package conflict requires a live assignment");
            if existing.organization_id != command.organization_id
                || existing.project_id != command.project_id
                || existing.environment_id != command.environment_id
                || existing.registry_id != command.registry_id
            {
                return Ok(Err(ApplicationError::Conflict(
                    "plugin assignment tenant or registry binding is immutable".into(),
                )));
            }
            let expected_aggregate_version = existing.aggregate_version;
            let assignment = match existing.set_desired(
                command.selection,
                command.policy_digest,
                command.desired_state,
                command.workspace_scope,
                command.actor_id,
                command.request_id,
                OperationId::new(),
                command.requested_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let idempotency = match UpdatePluginAssignmentWrite::idempotency_for(
                &assignment,
                command.idempotency_key,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = PluginAssignmentChanged::envelope(&assignment)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let write = match assignments
                .update(UpdatePluginAssignmentWrite {
                    assignment,
                    expected_aggregate_version,
                    event,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };

            Ok(Ok(SetPluginAssignmentResult {
                assignment: write.value,
                replayed: write.replayed,
            }))
        })
    }
}
