use crate::modules::fleet::domain::entities::NodeCertificate;
use crate::modules::shared_kernel::domain::{NodeCertificateId, NodeId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeCertificateRequest {
    pub certificate_id: NodeCertificateId,
    pub node_id: NodeId,
    pub csr_pem: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[async_trait]
pub trait ICertificateAuthority: Send + Sync {
    async fn issue(
        &self,
        request: NodeCertificateRequest,
    ) -> Result<NodeCertificate, CertificateAuthorityError>;

    async fn revoke(&self, certificate: &NodeCertificate) -> Result<(), CertificateAuthorityError>;

    async fn health(&self) -> Result<bool, CertificateAuthorityError>;
}

#[derive(Debug, thiserror::Error)]
pub enum CertificateAuthorityError {
    #[error("certificate request is invalid: {0}")]
    InvalidRequest(String),
    #[error("certificate authority rejected the request: {0}")]
    Rejected(String),
    #[error("certificate authority is unavailable: {0}")]
    Unavailable(String),
}
