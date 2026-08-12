use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OntologyId, OrganizationId};
use crate::modules::workflow::domain::OntologyRevision;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListOntologyRevisions {
    pub organization_id: OrganizationId,
    pub ontology_id: OntologyId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for ListOntologyRevisions {
    type Output = ApplicationResult<Vec<OntologyRevision>>;
}
