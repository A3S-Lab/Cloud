use super::CreateWorkflowDefinition;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{WorkflowDefinitionId, WorkflowRevisionId};
use crate::modules::workflow::application::{
    IWorkflowDefinitionPublicationPort, WorkflowDefinitionMutationResult,
    WorkflowDefinitionPublicationRequest,
};
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct CreateWorkflowDefinitionHandler {
    publications: Arc<dyn IWorkflowDefinitionPublicationPort>,
}

impl CreateWorkflowDefinitionHandler {
    pub fn new(publications: Arc<dyn IWorkflowDefinitionPublicationPort>) -> Self {
        Self { publications }
    }
}

impl CommandHandler<CreateWorkflowDefinition> for CreateWorkflowDefinitionHandler {
    fn execute(
        &self,
        command: CreateWorkflowDefinition,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<WorkflowDefinitionMutationResult>>,
    > {
        let publications = Arc::clone(&self.publications);
        Box::pin(async move {
            let definition_id = WorkflowDefinitionId::new();
            let revision_id = WorkflowRevisionId::new();
            Ok(publications
                .publish(WorkflowDefinitionPublicationRequest {
                    organization_id: command.organization_id,
                    project_id: command.project_id,
                    definition_id,
                    revision_id,
                    definition_acl: command.definition_acl,
                    payloads: command.payloads,
                    semantic_contracts: command.semantic_contracts,
                    actor_principal_id: command.actor_principal_id,
                    idempotency_scope: format!(
                        "organizations/{}/projects/{}/workflow-definitions",
                        command.organization_id, command.project_id
                    ),
                    idempotency_key: command.idempotency_key,
                    request_id: command.request_id,
                })
                .await)
        })
    }
}
