use crate::modules::edge::domain::{RoutePortName, UpstreamEndpoint};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeId, OrganizationId, ProjectId, RepositoryError, WorkloadId,
    WorkloadRevisionId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteTarget {
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub node_id: NodeId,
    pub upstream: UpstreamEndpoint,
}

#[async_trait]
pub trait IRouteTargetReader: Send + Sync {
    #[allow(clippy::too_many_arguments)]
    async fn resolve_healthy_target(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        revision_id: WorkloadRevisionId,
        port_name: &RoutePortName,
        now: DateTime<Utc>,
    ) -> Result<RouteTarget, RepositoryError>;
}
