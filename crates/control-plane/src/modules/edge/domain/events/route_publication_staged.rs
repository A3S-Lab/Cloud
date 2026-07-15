use crate::modules::edge::domain::{GatewayPublication, Route};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeCommandId, NodeId, OrganizationId, ProjectId, RouteId, WorkloadId,
    WorkloadRevisionId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutePublicationStaged {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub route_id: RouteId,
    pub node_id: NodeId,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub gateway_revision: u64,
    pub gateway_command_id: NodeCommandId,
    pub snapshot_digest: String,
    pub hostname: String,
    pub path_prefix: String,
}

impl RoutePublicationStaged {
    pub fn envelope(
        route: &Route,
        publication: &GatewayPublication,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "edge.route.publication-staged".into(),
            schema_version: 1,
            organization_id: route.organization_id.as_uuid(),
            aggregate_id: route.id.as_uuid(),
            aggregate_version: route.aggregate_version,
            occurred_at: route.updated_at,
            correlation_id: publication.command_correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: route.organization_id,
                project_id: route.project_id,
                environment_id: route.environment_id,
                route_id: route.id,
                node_id: route.gateway_node_id,
                workload_id: route.workload_id,
                workload_revision_id: route.workload_revision_id,
                gateway_revision: publication.revision,
                gateway_command_id: publication.command_id,
                snapshot_digest: publication.snapshot_digest.clone(),
                hostname: route.hostname.as_str().to_owned(),
                path_prefix: route.path_prefix.as_str().to_owned(),
            })?,
        })
    }
}
