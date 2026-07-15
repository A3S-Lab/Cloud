use crate::modules::edge::domain::{GatewayPublication, GatewayScopeState, Route};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, NodeId, OrganizationId, ProjectId, RepositoryError, RouteId,
};
use a3s_cloud_contracts::{DomainEventEnvelope, NodeGatewayAck};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct StageRoutePublication {
    pub route: Route,
    pub publication: GatewayPublication,
    pub expected_scope_version: u64,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

impl StageRoutePublication {
    pub fn validate(&self) -> Result<(), String> {
        let route = &self.route;
        let publication = &self.publication;
        if route.state != crate::modules::edge::domain::RouteState::Publishing
            || route.gateway_node_id != publication.node_id
            || route.gateway_revision != Some(publication.revision)
            || route.gateway_command_id != Some(publication.command_id)
            || route.snapshot_digest.as_deref() != Some(&publication.snapshot_digest)
            || publication.state != crate::modules::edge::domain::GatewayPublicationState::Pending
            || self.event.correlation_id != publication.command_correlation_id
        {
            return Err("route and complete Gateway publication are inconsistent".into());
        }
        publication.snapshot()?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeRoutePublicationResult {
    pub route: Route,
    pub publication: GatewayPublication,
    pub replayed: bool,
}

#[async_trait]
pub trait IEdgeRepository: Send + Sync {
    async fn gateway_scope(&self, node_id: NodeId) -> Result<GatewayScopeState, RepositoryError>;

    async fn active_routes(&self, node_id: NodeId) -> Result<Vec<Route>, RepositoryError>;

    async fn stage_route_publication(
        &self,
        bundle: StageRoutePublication,
    ) -> Result<EdgeRoutePublicationResult, RepositoryError>;

    async fn find_route(
        &self,
        organization_id: OrganizationId,
        route_id: RouteId,
    ) -> Result<Route, RepositoryError>;

    async fn list_routes(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<Route>, RepositoryError>;

    async fn project_gateway_acknowledgement(
        &self,
        acknowledgement: &NodeGatewayAck,
        received_at: DateTime<Utc>,
    ) -> Result<bool, RepositoryError>;
}
