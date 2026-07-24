use crate::modules::edge::domain::{GatewayPublication, GatewayRouteCutover};
use crate::modules::shared_kernel::domain::{
    DeploymentId, GatewayCertificateId, NodeCommandId, NodeId, OrganizationId, RouteId, WorkloadId,
    WorkloadRevisionId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayRouteCutoverStaged {
    pub deployment_id: DeploymentId,
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub previous_revision_id: WorkloadRevisionId,
    pub candidate_revision_id: WorkloadRevisionId,
    pub node_id: NodeId,
    pub route_ids: Vec<RouteId>,
    pub gateway_certificate_id: GatewayCertificateId,
    pub gateway_revision: u64,
    pub gateway_command_id: NodeCommandId,
    pub snapshot_digest: String,
}

impl GatewayRouteCutoverStaged {
    pub fn envelope(
        cutover: &GatewayRouteCutover,
        publication: &GatewayPublication,
    ) -> Result<DomainEventEnvelope, String> {
        cutover.validate()?;
        if cutover.node_id != publication.node_id
            || cutover.gateway_revision != publication.revision
            || cutover.gateway_command_id != publication.command_id
            || cutover.snapshot_digest != publication.snapshot_digest
            || cutover.snapshot_expires_at != publication.snapshot_expires_at
        {
            return Err("route cutover event publication identity is inconsistent".into());
        }
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "edge.route.cutover-staged".into(),
            schema_version: 1,
            organization_id: cutover.organization_id.as_uuid(),
            aggregate_id: cutover.deployment_id.as_uuid(),
            aggregate_version: 1,
            occurred_at: cutover.staged_at,
            correlation_id: publication.command_correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                deployment_id: cutover.deployment_id,
                organization_id: cutover.organization_id,
                workload_id: cutover.workload_id,
                previous_revision_id: cutover.previous_revision_id,
                candidate_revision_id: cutover.candidate_revision_id,
                node_id: cutover.node_id,
                route_ids: cutover.routes.iter().map(|route| route.id).collect(),
                gateway_certificate_id: cutover.gateway_certificate_id,
                gateway_revision: cutover.gateway_revision,
                gateway_command_id: cutover.gateway_command_id,
                snapshot_digest: cutover.snapshot_digest.clone(),
            })
            .map_err(|error| error.to_string())?,
        })
    }
}
