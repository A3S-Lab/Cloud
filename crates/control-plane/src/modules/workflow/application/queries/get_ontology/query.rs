use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OntologyId, OrganizationId};
use crate::modules::workflow::domain::Ontology;
use a3s_boot::Query;
use crate::modules::workflow::application::WorkflowAccess;

#[derive(Debug, Clone)]
pub struct GetOntology {
    pub organization_id: OrganizationId,
    pub ontology_id: OntologyId,
    pub access: WorkflowAccess,
}

impl Query for GetOntology {
    type Output = ApplicationResult<Ontology>;
}
