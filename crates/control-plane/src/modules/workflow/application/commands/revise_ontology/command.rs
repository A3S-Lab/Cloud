use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OntologyId, OrganizationId, PrincipalId};
use crate::modules::workflow::application::OntologyMutationResult;
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ReviseOntology {
    pub organization_id: OrganizationId,
    pub ontology_id: OntologyId,
    pub acl: String,
    pub expected_version: u64,
    pub migration_rule_id: Option<String>,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for ReviseOntology {
    type Output = ApplicationResult<OntologyMutationResult>;
}
