use crate::modules::edge::domain::GatewayPublication;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayScopeId, NodeCommandId, NodeId,
    OrganizationId, ProjectId, RouteId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpGatewaySnapshotStaged {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub gateway_scope_id: GatewayScopeId,
    pub desired_gateway_scope_ids: Vec<GatewayScopeId>,
    pub node_id: NodeId,
    pub gateway_revision: u64,
    pub gateway_command_id: NodeCommandId,
    pub snapshot_digest: String,
    pub ordinary_route_ids: Vec<RouteId>,
    pub mcp_route_ids: Vec<RouteId>,
    pub domain_claim_ids: Vec<DomainClaimId>,
    pub gateway_certificate_id: Option<GatewayCertificateId>,
}

impl McpGatewaySnapshotStaged {
    #[allow(clippy::too_many_arguments)]
    pub fn envelope(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        gateway_scope_id: GatewayScopeId,
        desired_gateway_scope_ids: Vec<GatewayScopeId>,
        next_physical_scope_version: u64,
        publication: &GatewayPublication,
        ordinary_route_ids: Vec<RouteId>,
        mcp_route_ids: Vec<RouteId>,
        domain_claim_ids: Vec<DomainClaimId>,
    ) -> Result<DomainEventEnvelope, String> {
        publication.snapshot()?;
        if organization_id.as_uuid().is_nil()
            || project_id.as_uuid().is_nil()
            || environment_id.as_uuid().is_nil()
            || gateway_scope_id.as_uuid().is_nil()
            || next_physical_scope_version == 0
            || desired_gateway_scope_ids
                .first()
                .is_some_and(|scope_id| *scope_id != gateway_scope_id)
            || !strictly_sorted(&desired_gateway_scope_ids)
            || !strictly_sorted(&ordinary_route_ids)
            || !strictly_sorted(&mcp_route_ids)
            || !strictly_sorted(&domain_claim_ids)
        {
            return Err("staged MCP Gateway snapshot event evidence is invalid".into());
        }
        let gateway_certificate_id = publication
            .certificate_request
            .as_ref()
            .map(|request| GatewayCertificateId::from_uuid(request.certificate_id));
        if gateway_certificate_id.is_some() != !domain_claim_ids.is_empty() {
            return Err(
                "staged MCP Gateway snapshot certificate and domain evidence differ".into(),
            );
        }
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "edge.mcp-gateway.snapshot-staged".into(),
            schema_version: 2,
            organization_id: organization_id.as_uuid(),
            aggregate_id: publication.node_id.as_uuid(),
            aggregate_version: next_physical_scope_version,
            occurred_at: publication.command_issued_at,
            correlation_id: publication.command_correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id,
                project_id,
                environment_id,
                gateway_scope_id,
                desired_gateway_scope_ids,
                node_id: publication.node_id,
                gateway_revision: publication.revision,
                gateway_command_id: publication.command_id,
                snapshot_digest: publication.snapshot_digest.clone(),
                ordinary_route_ids,
                mcp_route_ids,
                domain_claim_ids,
                gateway_certificate_id,
            })
            .map_err(|error| error.to_string())?,
        })
    }
}

fn strictly_sorted<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|values| values[0] < values[1])
}
