use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OntologyId, OntologyRevisionId, OrganizationId,
    PrincipalId, ProjectId, RepositoryError,
};
use crate::modules::workflow::domain::{Ontology, OntologyRevision};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OntologyRecord {
    pub ontology: Ontology,
    pub revision: OntologyRevision,
}

#[derive(Debug, Clone)]
pub struct CreateOntologyWrite {
    pub record: OntologyRecord,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct ReviseOntologyWrite {
    pub record: OntologyRecord,
    pub expected_version: u64,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OntologyWriteReference {
    pub organization_id: OrganizationId,
    pub ontology_id: OntologyId,
    pub revision_id: OntologyRevisionId,
}

#[async_trait]
pub trait IOntologyRepository: Send + Sync {
    async fn create(
        &self,
        write: CreateOntologyWrite,
    ) -> Result<IdempotentWrite<OntologyRecord>, RepositoryError>;

    async fn revise(
        &self,
        write: ReviseOntologyWrite,
    ) -> Result<IdempotentWrite<OntologyRecord>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        ontology_id: OntologyId,
    ) -> Result<Option<Ontology>, RepositoryError>;

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<Ontology>, RepositoryError>;

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        ontology_id: OntologyId,
        revision_id: OntologyRevisionId,
    ) -> Result<Option<OntologyRevision>, RepositoryError>;

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        ontology_id: OntologyId,
    ) -> Result<Vec<OntologyRevision>, RepositoryError>;
}
