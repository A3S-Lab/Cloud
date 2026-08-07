use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OntologyId, OntologyRevisionId, OrganizationId};
use crate::modules::workflow::domain::OntologyDiff;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct DiffOntologyRevisions {
    pub organization_id: OrganizationId,
    pub ontology_id: OntologyId,
    pub from_revision_id: OntologyRevisionId,
    pub to_revision_id: OntologyRevisionId,
}

impl Query for DiffOntologyRevisions {
    type Output = ApplicationResult<OntologyRevisionDiff>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OntologyRevisionDiff {
    pub ontology_id: OntologyId,
    pub from_revision_id: OntologyRevisionId,
    pub to_revision_id: OntologyRevisionId,
    pub diff: OntologyDiff,
}
