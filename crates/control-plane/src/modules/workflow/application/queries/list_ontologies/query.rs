use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use crate::modules::workflow::domain::Ontology;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListOntologies {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
}

impl Query for ListOntologies {
    type Output = ApplicationResult<Vec<Ontology>>;
}
