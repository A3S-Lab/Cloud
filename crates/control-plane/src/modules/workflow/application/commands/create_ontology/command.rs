use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, ProjectId};
use crate::modules::workflow::application::OntologyMutationResult;
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateOntology {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub acl: String,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateOntology {
    type Output = ApplicationResult<OntologyMutationResult>;
}
