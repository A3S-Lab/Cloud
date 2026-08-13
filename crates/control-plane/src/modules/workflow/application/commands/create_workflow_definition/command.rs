use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, ProjectId};
use crate::modules::workflow::application::{
    WorkflowDefinitionMutationResult, WorkflowPayloadAcl, WorkflowSemanticContractAcls,
};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateWorkflowDefinition {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub definition_acl: String,
    pub payloads: Vec<WorkflowPayloadAcl>,
    pub semantic_contracts: Option<WorkflowSemanticContractAcls>,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateWorkflowDefinition {
    type Output = ApplicationResult<WorkflowDefinitionMutationResult>;
}
