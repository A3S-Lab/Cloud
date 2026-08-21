use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    OntologyId, OntologyRevisionId, OrganizationId, ProjectId, Sha256Digest,
};
use async_trait::async_trait;

/// Exact Ontology revision evidence admitted before an Application invocation
/// becomes durable. Workflow validates the same evidence again when it adopts
/// the resulting Goal/Plan/Run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationOntologyRevisionEvidence {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub ontology_id: OntologyId,
    pub ontology_revision_id: OntologyRevisionId,
    pub ontology_digest: Sha256Digest,
}

#[async_trait]
pub trait IApplicationOntologyRevisionPort: Send + Sync {
    async fn resolve_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        ontology_id: OntologyId,
        ontology_revision_id: OntologyRevisionId,
    ) -> ApplicationResult<ApplicationOntologyRevisionEvidence>;
}
