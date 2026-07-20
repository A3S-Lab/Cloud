use crate::modules::edge::domain::{
    GatewayCertificateConvergence, GatewayPublication, GatewayRouteVersion,
};
use crate::modules::shared_kernel::domain::{
    GatewayCertificateId, NodeCommandId, NodeId, OrganizationId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayCertificateConvergenceStaged {
    pub organization_id: OrganizationId,
    pub node_id: NodeId,
    pub gateway_revision: u64,
    pub gateway_command_id: NodeCommandId,
    pub previous_certificate_id: GatewayCertificateId,
    pub replacement_certificate_id: Option<GatewayCertificateId>,
    pub retained_routes: Vec<GatewayRouteVersion>,
    pub rejected_routes: Vec<GatewayRouteVersion>,
    pub reason: String,
}

impl GatewayCertificateConvergenceStaged {
    pub fn envelope(
        convergence: &GatewayCertificateConvergence,
        publication: &GatewayPublication,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "edge.gateway-certificate.convergence-staged".into(),
            schema_version: 1,
            organization_id: convergence.organization_id.as_uuid(),
            aggregate_id: convergence
                .replacement_certificate_id
                .unwrap_or(convergence.previous_certificate_id)
                .as_uuid(),
            aggregate_version: 1,
            occurred_at: convergence.staged_at,
            correlation_id: publication.command_correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: convergence.organization_id,
                node_id: convergence.node_id,
                gateway_revision: convergence.gateway_revision,
                gateway_command_id: convergence.gateway_command_id,
                previous_certificate_id: convergence.previous_certificate_id,
                replacement_certificate_id: convergence.replacement_certificate_id,
                retained_routes: convergence.retained_routes.clone(),
                rejected_routes: convergence.rejected_routes.clone(),
                reason: convergence.reason.as_str().into(),
            })?,
        })
    }
}
