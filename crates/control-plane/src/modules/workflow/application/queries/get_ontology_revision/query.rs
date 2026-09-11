use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OntologyId, OntologyRevisionId, OrganizationId};
use crate::modules::workflow::domain::OntologyRevision;
use a3s_boot::Query;
use crate::modules::workflow::application::WorkflowAccess;

#[derive(Debug, Clone)]
pub struct GetOntologyRevision {
    pub organization_id: OrganizationId,
    pub ontology_id: OntologyId,
    pub revision_id: OntologyRevisionId,
    pub access: WorkflowAccess,
}

impl Query for GetOntologyRevision {
    type Output = ApplicationResult<OntologyRevision>;
}
