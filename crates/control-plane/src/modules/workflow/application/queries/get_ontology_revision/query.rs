use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OntologyId, OntologyRevisionId, OrganizationId};
use crate::modules::workflow::domain::OntologyRevision;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetOntologyRevision {
    pub organization_id: OrganizationId,
    pub ontology_id: OntologyId,
    pub revision_id: OntologyRevisionId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetOntologyRevision {
    type Output = ApplicationResult<OntologyRevision>;
}
