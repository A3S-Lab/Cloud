use crate::modules::artifacts::domain::entities::PartnerArtifactAdmission;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AdmitPartnerArtifact {
    pub organization_id: OrganizationId,
    pub content_digest: String,
    pub kind: String,
    pub byte_size: u64,
    pub partner_ref: String,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for AdmitPartnerArtifact {
    type Output = ApplicationResult<AdmitPartnerArtifactResult>;
}

#[derive(Debug, Clone)]
pub struct AdmitPartnerArtifactResult {
    pub admission: PartnerArtifactAdmission,
    pub replayed: bool,
}
