use super::CreateWorkflowDefinition;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{WorkflowDefinitionId, WorkflowRevisionId};
use crate::modules::workflow::application::{
    IWorkflowDefinitionPublicationPort, WorkflowDefinitionMutationResult,
    WorkflowDefinitionPublicationProvenance, WorkflowDefinitionPublicationRequest,
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
            if !command.access.project_is_visible(command.project_id) {
                return Ok(Err(ApplicationError::NotFound("project not found".into())));
            }
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
                    provenance: WorkflowDefinitionPublicationProvenance::UserAuthored,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, ProjectId};
    use crate::modules::workflow::application::{WorkflowAccess, WorkflowAccessScope};
    use a3s_boot::ModuleRef;
    use async_trait::async_trait;
    use uuid::Uuid;

    struct RejectPublication;

    #[async_trait]
    impl IWorkflowDefinitionPublicationPort for RejectPublication {
        async fn publish(
            &self,
            _request: WorkflowDefinitionPublicationRequest,
        ) -> ApplicationResult<WorkflowDefinitionMutationResult> {
            Err(ApplicationError::Internal(
                "publication must not run for denied creates".into(),
            ))
        }
    }

    #[tokio::test]
    async fn create_workflow_definition_fails_closed_before_publishing_in_an_ungranted_project() {
        let handler = CreateWorkflowDefinitionHandler::new(Arc::new(RejectPublication));
        let result = handler
            .execute(
                CreateWorkflowDefinition {
                    organization_id: OrganizationId::new(),
                    project_id: ProjectId::new(),
                    access: WorkflowAccess::restricted([WorkflowAccessScope::Project {
                        project_id: ProjectId::new(),
                    }]),
                    definition_acl: "workflow {}".into(),
                    payloads: Vec::new(),
                    semantic_contracts: None,
                    actor_principal_id: PrincipalId::new(),
                    idempotency_key: "deny-create".into(),
                    request_id: Uuid::now_v7(),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("handler");
        assert_eq!(
            result,
            Err(ApplicationError::NotFound("project not found".into()))
        );
    }
}
