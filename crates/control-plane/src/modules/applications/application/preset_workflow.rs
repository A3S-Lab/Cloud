use super::preset_workflow_port::{
    ApplicationPresetTarget, ApplicationPresetWorkflowRequest, ApplicationPresetWorkflowResult,
    IApplicationPresetWorkflowPort,
};
use super::resource_access::project;
use crate::modules::applications::domain::ApplicationExperience;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, OrganizationId, PrincipalId, ProjectId,
};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CompileApplicationPresetWorkflow {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_number: u64,
    pub experience: ApplicationExperience,
    pub target: ApplicationPresetTarget,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CompileApplicationPresetWorkflow {
    type Output = ApplicationResult<ApplicationPresetWorkflowResult>;
}

pub struct CompileApplicationPresetWorkflowHandler {
    workflows: Arc<dyn IApplicationPresetWorkflowPort>,
}

impl CompileApplicationPresetWorkflowHandler {
    pub fn new(workflows: Arc<dyn IApplicationPresetWorkflowPort>) -> Self {
        Self { workflows }
    }
}

impl CommandHandler<CompileApplicationPresetWorkflow> for CompileApplicationPresetWorkflowHandler {
    fn execute(
        &self,
        command: CompileApplicationPresetWorkflow,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationPresetWorkflowResult>>,
    > {
        let workflows = Arc::clone(&self.workflows);
        Box::pin(async move {
            if let Err(error) = project(command.project_id, &command.resource_access) {
                return Ok(Err(error));
            }
            let request = ApplicationPresetWorkflowRequest {
                organization_id: command.organization_id,
                project_id: command.project_id,
                application_id: command.application_id,
                application_release_number: command.application_release_number,
                experience: command.experience,
                target: command.target,
                actor_principal_id: command.actor_principal_id,
                idempotency_key: command.idempotency_key,
                request_id: command.request_id,
            };
            if let Err(error) = request.validate() {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            Ok(workflows.compile_and_publish(&request).await)
        })
    }
}
