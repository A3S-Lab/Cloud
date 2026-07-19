use crate::modules::edge::domain::GatewayCertificate;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayCertificateResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub node_id: Uuid,
    pub domain_claim_ids: Vec<Uuid>,
    pub dns_names: Vec<String>,
    pub gateway_revision: u64,
    pub gateway_command_id: Uuid,
    pub snapshot_digest: String,
    pub state: String,
    pub serial_number: Option<String>,
    pub fingerprint: Option<String>,
    pub issued_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub failure: Option<String>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub ready_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<GatewayCertificate> for GatewayCertificateResponse {
    fn from(certificate: GatewayCertificate) -> Self {
        let (serial_number, fingerprint, issued_at, expires_at) =
            certificate
                .material
                .map_or((None, None, None, None), |material| {
                    (
                        Some(material.serial_number),
                        Some(material.fingerprint),
                        Some(material.issued_at),
                        Some(material.expires_at),
                    )
                });
        Self {
            id: certificate.id.as_uuid(),
            organization_id: certificate.organization_id.as_uuid(),
            node_id: certificate.node_id.as_uuid(),
            domain_claim_ids: certificate
                .domain_claim_ids
                .into_iter()
                .map(|id| id.as_uuid())
                .collect(),
            dns_names: certificate.request.dns_names,
            gateway_revision: certificate.gateway_revision,
            gateway_command_id: certificate.gateway_command_id.as_uuid(),
            snapshot_digest: certificate.snapshot_digest,
            state: certificate.state.as_str().into(),
            serial_number,
            fingerprint,
            issued_at,
            expires_at,
            failure: certificate.failure,
            aggregate_version: certificate.aggregate_version,
            created_at: certificate.created_at,
            updated_at: certificate.updated_at,
            ready_at: certificate.ready_at,
            revoked_at: certificate.revoked_at,
        }
    }
}
