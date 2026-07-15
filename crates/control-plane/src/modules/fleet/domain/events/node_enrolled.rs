use crate::modules::fleet::domain::entities::{Node, NodeCertificate};
use crate::modules::shared_kernel::domain::{NodeCertificateId, NodeId, OrganizationId};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeEnrolled {
    pub organization_id: OrganizationId,
    pub node_id: NodeId,
    pub certificate_id: NodeCertificateId,
    pub provider_id: String,
    pub capabilities_digest: String,
}

impl NodeEnrolled {
    pub fn envelope(
        node: &Node,
        certificate: &NodeCertificate,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "fleet.node.enrolled".into(),
            schema_version: 1,
            organization_id: node.organization_id.as_uuid(),
            aggregate_id: node.id.as_uuid(),
            aggregate_version: node.aggregate_version,
            occurred_at: node.enrolled_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: node.organization_id,
                node_id: node.id,
                certificate_id: certificate.id,
                provider_id: node.capabilities.provider_id().into(),
                capabilities_digest: node.capabilities.digest().into(),
            })?,
        })
    }
}
