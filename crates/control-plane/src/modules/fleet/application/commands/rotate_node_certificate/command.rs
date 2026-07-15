use crate::modules::fleet::domain::entities::NodeCertificate;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{NodeCertificateId, NodeId, OrganizationId};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone)]
pub struct RotateNodeCertificate {
    pub organization_id: OrganizationId,
    pub node_id: NodeId,
    pub current_certificate_id: NodeCertificateId,
    pub csr_pem: String,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for RotateNodeCertificate {
    type Output = ApplicationResult<RotateNodeCertificateResult>;
}

#[derive(Debug, Clone)]
pub struct RotateNodeCertificateResult {
    pub certificate: NodeCertificate,
    pub replayed: bool,
}
