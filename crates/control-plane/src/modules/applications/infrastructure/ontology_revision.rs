use crate::modules::applications::application::{
    ApplicationOntologyRevisionEvidence, IApplicationOntologyRevisionPort,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    OntologyId, OntologyRevisionId, OrganizationId, ProjectId,
};
use crate::modules::workflow::domain::IOntologyRepository;
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Clone)]
pub struct WorkflowApplicationOntologyRevisionReader {
    ontologies: Arc<dyn IOntologyRepository>,
}

impl WorkflowApplicationOntologyRevisionReader {
    pub fn new(ontologies: Arc<dyn IOntologyRepository>) -> Self {
        Self { ontologies }
    }
}

#[async_trait]
impl IApplicationOntologyRevisionPort for WorkflowApplicationOntologyRevisionReader {
    async fn resolve_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        ontology_id: OntologyId,
        ontology_revision_id: OntologyRevisionId,
    ) -> ApplicationResult<ApplicationOntologyRevisionEvidence> {
        let revision = self
            .ontologies
            .find_revision(organization_id, ontology_id, ontology_revision_id)
            .await?
            .filter(|revision| revision.project_id == project_id)
            .ok_or_else(|| {
                ApplicationError::NotFound("Application OntologyRevision not found".into())
            })?;
        revision.validate().map_err(|error| {
            ApplicationError::Internal(format!("stored Ontology revision is invalid: {error}"))
        })?;
        Ok(ApplicationOntologyRevisionEvidence {
            organization_id: revision.organization_id,
            project_id: revision.project_id,
            ontology_id: revision.ontology_id,
            ontology_revision_id: revision.id,
            ontology_digest: revision.contract.digest().clone(),
        })
    }
}
