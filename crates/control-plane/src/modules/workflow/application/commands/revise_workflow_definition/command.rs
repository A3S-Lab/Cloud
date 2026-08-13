use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, WorkflowDefinitionId};
use crate::modules::workflow::application::{
    WorkflowDefinitionMutationResult, WorkflowPayloadAcl, WorkflowSemanticContractAcls,
};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ReviseWorkflowDefinition {
    pub organization_id: OrganizationId,
    pub workflow_definition_id: WorkflowDefinitionId,
    pub resource_access: ResourceAccessEvaluator,
    pub expected_version: u64,
    pub definition_acl: String,
    pub payloads: Vec<WorkflowPayloadAcl>,
    pub semantic_contracts: Option<WorkflowSemanticContractAcls>,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for ReviseWorkflowDefinition {
    type Output = ApplicationResult<WorkflowDefinitionMutationResult>;
}
