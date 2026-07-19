use crate::modules::edge::domain::{GatewayCertificate, GatewayCertificateMaterial};
use crate::modules::shared_kernel::domain::{GatewayCertificateId, NodeId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayCertificateIssueRequest {
    pub certificate_id: GatewayCertificateId,
    pub node_id: NodeId,
    pub dns_names: Vec<String>,
    pub csr_pem: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[async_trait]
pub trait IGatewayCertificateAuthority: Send + Sync {
    async fn issue(
        &self,
        request: GatewayCertificateIssueRequest,
    ) -> Result<GatewayCertificateMaterial, GatewayCertificateAuthorityError>;

    async fn revoke(
        &self,
        certificate: &GatewayCertificate,
    ) -> Result<(), GatewayCertificateAuthorityError>;

    async fn health(&self) -> Result<bool, GatewayCertificateAuthorityError>;
}

#[derive(Debug, thiserror::Error)]
pub enum GatewayCertificateAuthorityError {
    #[error("Gateway certificate request is invalid: {0}")]
    InvalidRequest(String),
    #[error("Gateway certificate authority rejected the request: {0}")]
    Rejected(String),
    #[error("Gateway certificate authority is unavailable: {0}")]
    Unavailable(String),
}
