use crate::modules::forms::domain::{FormDraft, FormRelease};
use crate::modules::shared_kernel::domain::{
    FormId, FormReleaseId, IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId,
    ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormPublicationRecord {
    pub draft: FormDraft,
    pub release: FormRelease,
}

#[derive(Debug, Clone)]
pub struct CreateFormDraftWrite {
    pub draft: FormDraft,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct ReviseFormDraftWrite {
    pub draft: FormDraft,
    pub expected_version: u64,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct PublishFormReleaseWrite {
    pub publication: FormPublicationRecord,
    pub expected_version: u64,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[async_trait]
pub trait IFormRepository: Send + Sync {
    async fn replay_draft_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<FormDraft>>, RepositoryError>;

    async fn replay_publication(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<FormPublicationRecord>>, RepositoryError>;

    async fn create_draft(
        &self,
        write: CreateFormDraftWrite,
    ) -> Result<IdempotentWrite<FormDraft>, RepositoryError>;

    async fn revise_draft(
        &self,
        write: ReviseFormDraftWrite,
    ) -> Result<IdempotentWrite<FormDraft>, RepositoryError>;

    async fn publish_release(
        &self,
        write: PublishFormReleaseWrite,
    ) -> Result<IdempotentWrite<FormPublicationRecord>, RepositoryError>;

    async fn find_draft(
        &self,
        organization_id: OrganizationId,
        form_id: FormId,
    ) -> Result<Option<FormDraft>, RepositoryError>;

    async fn list_drafts(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<FormDraft>, RepositoryError>;

    async fn find_release(
        &self,
        organization_id: OrganizationId,
        form_id: FormId,
        release_id: FormReleaseId,
    ) -> Result<Option<FormRelease>, RepositoryError>;

    async fn list_releases(
        &self,
        organization_id: OrganizationId,
        form_id: FormId,
    ) -> Result<Vec<FormRelease>, RepositoryError>;
}
