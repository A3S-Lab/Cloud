use crate::modules::artifacts::domain::entities::{
    PartnerArtifactAdmission, PartnerArtifactAdmissionId,
};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, RepositoryError, Sha256Digest,
};
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct AdmitPartnerArtifactWrite {
    pub admission: PartnerArtifactAdmission,
    pub idempotency: IdempotencyRequest,
}

#[async_trait]
pub trait IPartnerArtifactAdmissionRepository: Send + Sync {
    async fn admit(
        &self,
        write: AdmitPartnerArtifactWrite,
    ) -> Result<IdempotentWrite<PartnerArtifactAdmission>, RepositoryError>;

    async fn find_by_id(
        &self,
        organization_id: OrganizationId,
        id: PartnerArtifactAdmissionId,
    ) -> Result<Option<PartnerArtifactAdmission>, RepositoryError>;

    async fn find_by_digest(
        &self,
        organization_id: OrganizationId,
        content_digest: &Sha256Digest,
    ) -> Result<Option<PartnerArtifactAdmission>, RepositoryError>;

    async fn list_by_organization(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<PartnerArtifactAdmission>, RepositoryError>;
}
