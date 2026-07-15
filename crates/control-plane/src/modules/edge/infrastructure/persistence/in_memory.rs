use crate::modules::edge::domain::repositories::{
    EdgeRoutePublicationResult, IEdgeRepository, StageRoutePublication,
};
use crate::modules::edge::domain::{
    GatewayPublication, GatewayPublicationState, GatewayScopeState, Route, RouteState,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeCommandId, NodeId, OrganizationId, ProjectId, RepositoryError, RouteId,
};
use a3s_cloud_contracts::{DomainEventEnvelope, GatewayAckState, NodeGatewayAck};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryEdgeRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    scopes: BTreeMap<NodeId, GatewayScopeState>,
    routes: BTreeMap<RouteId, Route>,
    ownership: BTreeMap<(NodeId, String, String), RouteId>,
    publications: BTreeMap<(NodeId, u64), GatewayPublication>,
    commands: BTreeMap<(NodeId, NodeCommandId), u64>,
    idempotency: BTreeMap<(String, String), (String, EdgeRoutePublicationResult)>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryEdgeRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IEdgeRepository for InMemoryEdgeRepository {
    async fn replay_route_publication(
        &self,
        idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
    ) -> Result<Option<EdgeRoutePublicationResult>, RepositoryError> {
        let state = self.state.read().await;
        let Some((digest, existing)) = state
            .idempotency
            .get(&(idempotency.scope.clone(), idempotency.key.clone()))
        else {
            return Ok(None);
        };
        if digest != &idempotency.request_digest {
            return Err(RepositoryError::IdempotencyConflict);
        }
        let mut replay = existing.clone();
        replay.replayed = true;
        Ok(Some(replay))
    }

    async fn gateway_scope(&self, node_id: NodeId) -> Result<GatewayScopeState, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .scopes
            .get(&node_id)
            .cloned()
            .unwrap_or_else(|| GatewayScopeState::empty(node_id)))
    }

    async fn active_routes(&self, node_id: NodeId) -> Result<Vec<Route>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .routes
            .values()
            .filter(|route| route.gateway_node_id == node_id && route.state == RouteState::Active)
            .cloned()
            .collect())
    }

    async fn stage_route_publication(
        &self,
        bundle: StageRoutePublication,
    ) -> Result<EdgeRoutePublicationResult, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        let idempotency_key = (
            bundle.idempotency.scope.clone(),
            bundle.idempotency.key.clone(),
        );
        if let Some((digest, existing)) = state.idempotency.get(&idempotency_key) {
            if digest != &bundle.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            let mut replay = existing.clone();
            replay.replayed = true;
            return Ok(replay);
        }
        let current = state
            .scopes
            .get(&bundle.publication.node_id)
            .cloned()
            .unwrap_or_else(|| GatewayScopeState::empty(bundle.publication.node_id));
        if current.aggregate_version != bundle.expected_scope_version {
            return Err(RepositoryError::Conflict(
                "Gateway scope changed while compiling the complete snapshot".into(),
            ));
        }
        if state.publications.values().any(|publication| {
            publication.node_id == bundle.publication.node_id
                && publication.state == GatewayPublicationState::Pending
        }) {
            return Err(RepositoryError::Conflict(
                "Gateway scope already has a pending complete snapshot".into(),
            ));
        }
        if bundle.publication.revision
            != current.next_revision().map_err(RepositoryError::Conflict)?
            || bundle.publication.expected_revision != current.installed_revision
        {
            return Err(RepositoryError::Conflict(
                "Gateway publication does not advance the authoritative scope revision".into(),
            ));
        }
        let ownership = (
            bundle.route.gateway_node_id,
            bundle.route.hostname.as_str().to_owned(),
            bundle.route.path_prefix.as_str().to_owned(),
        );
        if state.ownership.contains_key(&ownership) || state.routes.contains_key(&bundle.route.id) {
            return Err(RepositoryError::Conflict(
                "hostname and path are already owned in this Gateway scope".into(),
            ));
        }
        let result = EdgeRoutePublicationResult {
            route: bundle.route.clone(),
            publication: bundle.publication.clone(),
            replayed: false,
        };
        state.ownership.insert(ownership, bundle.route.id);
        state.routes.insert(bundle.route.id, bundle.route);
        state.publications.insert(
            (bundle.publication.node_id, bundle.publication.revision),
            bundle.publication.clone(),
        );
        state.commands.insert(
            (bundle.publication.node_id, bundle.publication.command_id),
            bundle.publication.revision,
        );
        state.scopes.insert(
            bundle.publication.node_id,
            GatewayScopeState {
                node_id: bundle.publication.node_id,
                last_issued_revision: bundle.publication.revision,
                installed_revision: current.installed_revision,
                aggregate_version: current.aggregate_version + 1,
            },
        );
        state.idempotency.insert(
            idempotency_key,
            (bundle.idempotency.request_digest, result.clone()),
        );
        state.outbox.push(bundle.event);
        Ok(result)
    }

    async fn find_route(
        &self,
        organization_id: OrganizationId,
        route_id: RouteId,
    ) -> Result<Route, RepositoryError> {
        self.state
            .read()
            .await
            .routes
            .get(&route_id)
            .filter(|route| route.organization_id == organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn list_routes(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<Route>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .routes
            .values()
            .filter(|route| {
                route.organization_id == organization_id
                    && route.project_id == project_id
                    && route.environment_id == environment_id
            })
            .cloned()
            .collect())
    }

    async fn project_gateway_acknowledgement(
        &self,
        acknowledgement: &NodeGatewayAck,
        received_at: DateTime<Utc>,
    ) -> Result<bool, RepositoryError> {
        acknowledgement
            .validate()
            .map_err(RepositoryError::Conflict)?;
        if received_at < acknowledgement.acknowledged_at {
            return Err(RepositoryError::Conflict(
                "Gateway acknowledgement receipt predates its node timestamp".into(),
            ));
        }
        let node_id = NodeId::from_uuid(acknowledgement.node_id);
        let command_id = NodeCommandId::from_uuid(acknowledgement.command_id);
        let mut state = self.state.write().await;
        let Some(revision) = state.commands.get(&(node_id, command_id)).copied() else {
            return Ok(false);
        };
        let mut publication = state
            .publications
            .get(&(node_id, revision))
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage(
                    "Gateway publication command references missing desired state".into(),
                )
            })?;
        publication
            .acknowledge(acknowledgement)
            .map_err(RepositoryError::Conflict)?;
        let route_ids = state
            .routes
            .values()
            .filter(|route| {
                route.gateway_node_id == node_id
                    && route.gateway_revision == Some(revision)
                    && route.gateway_command_id == Some(command_id)
            })
            .map(|route| route.id)
            .collect::<Vec<_>>();
        if route_ids.is_empty() {
            return Err(RepositoryError::Storage(
                "Gateway publication has no staged routes".into(),
            ));
        }
        for route_id in route_ids {
            state
                .routes
                .get_mut(&route_id)
                .ok_or_else(|| RepositoryError::Storage("staged route disappeared".into()))?
                .apply_gateway_acknowledgement(acknowledgement)
                .map_err(RepositoryError::Conflict)?;
        }
        state.publications.insert((node_id, revision), publication);
        if acknowledgement.state == GatewayAckState::Applied {
            let scope = state.scopes.get_mut(&node_id).ok_or_else(|| {
                RepositoryError::Storage("Gateway scope disappeared during acknowledgement".into())
            })?;
            scope.installed_revision = Some(revision);
            scope.aggregate_version += 1;
        }
        Ok(true)
    }
}
