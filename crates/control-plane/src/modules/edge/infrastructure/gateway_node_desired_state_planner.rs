use crate::modules::edge::domain::GatewayScope;
use crate::modules::edge::infrastructure::{
    GatewaySnapshotRouteInput, IMcpGatewayNodeProjectionPlanner, IMcpGatewaySnapshotRepository,
    PlanMcpGatewayNodeProjection, PlannedMcpGatewayNodeProjection,
};
use crate::modules::shared_kernel::domain::{canonical_timestamp, NodeId, RepositoryError};
use chrono::{DateTime, Utc};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct PlanGatewayNodeDesiredState {
    pub gateway_node_id: NodeId,
    pub fallback_scope: GatewayScope,
    pub observed_at: DateTime<Utc>,
}

/// One exact read-side observation used to compile a complete physical
/// Gateway snapshot. Ordinary Route and MCP policy evidence deliberately
/// travel together so persistence can validate both under one transaction.
#[derive(Debug, Clone)]
pub struct PlannedGatewayNodeDesiredState {
    physical_scope: crate::modules::edge::domain::GatewayScopeState,
    active_routes: Vec<GatewaySnapshotRouteInput>,
    mcp: PlannedMcpGatewayNodeProjection,
}

impl PlannedGatewayNodeDesiredState {
    pub fn new(
        physical_scope: crate::modules::edge::domain::GatewayScopeState,
        active_routes: Vec<GatewaySnapshotRouteInput>,
        mcp: PlannedMcpGatewayNodeProjection,
    ) -> Result<Self, String> {
        if physical_scope.node_id != mcp.gateway_node_id()
            || active_routes
                .iter()
                .any(|input| input.route.gateway_node_id != physical_scope.node_id)
        {
            return Err("Gateway node desired-state evidence crosses a physical node".into());
        }
        Ok(Self {
            physical_scope,
            active_routes,
            mcp,
        })
    }

    pub const fn physical_scope(&self) -> &crate::modules::edge::domain::GatewayScopeState {
        &self.physical_scope
    }

    pub fn active_routes(&self) -> &[GatewaySnapshotRouteInput] {
        &self.active_routes
    }

    pub const fn mcp(&self) -> &PlannedMcpGatewayNodeProjection {
        &self.mcp
    }

    pub fn into_parts(
        self,
    ) -> (
        crate::modules::edge::domain::GatewayScopeState,
        Vec<GatewaySnapshotRouteInput>,
        PlannedMcpGatewayNodeProjection,
    ) {
        (self.physical_scope, self.active_routes, self.mcp)
    }
}

#[derive(Clone)]
pub struct GatewayNodeDesiredStatePlanner {
    repository: Arc<dyn IMcpGatewaySnapshotRepository>,
    projections: Arc<dyn IMcpGatewayNodeProjectionPlanner>,
}

impl GatewayNodeDesiredStatePlanner {
    pub fn new(
        repository: Arc<dyn IMcpGatewaySnapshotRepository>,
        projections: Arc<dyn IMcpGatewayNodeProjectionPlanner>,
    ) -> Self {
        Self {
            repository,
            projections,
        }
    }

    pub async fn plan(
        &self,
        request: PlanGatewayNodeDesiredState,
    ) -> Result<PlannedGatewayNodeDesiredState, RepositoryError> {
        request
            .fallback_scope
            .validate()
            .map_err(RepositoryError::Conflict)?;
        if request.gateway_node_id.as_uuid().is_nil()
            || !request
                .fallback_scope
                .contains_member(request.gateway_node_id)
        {
            return Err(RepositoryError::Conflict(
                "Gateway desired-state fallback scope does not contain its physical node".into(),
            ));
        }
        let observed_at = canonical_timestamp(request.observed_at);
        let (inputs, mut scopes) = tokio::try_join!(
            self.repository
                .mcp_gateway_snapshot_inputs(request.gateway_node_id),
            self.repository
                .mcp_gateway_reconciliation_scope_set(request.gateway_node_id, observed_at),
        )?;
        inputs
            .validate(request.gateway_node_id)
            .map_err(RepositoryError::Storage)?;
        if scopes.iter().any(|scope| {
            scope.organization_id != request.fallback_scope.organization_id
                || !scope.contains_member(request.gateway_node_id)
        }) {
            return Err(RepositoryError::Conflict(
                "Gateway desired state crosses its physical node organization or membership".into(),
            ));
        }
        if scopes.is_empty() {
            scopes.push(request.fallback_scope);
        } else if let Ok(index) =
            scopes.binary_search_by_key(&request.fallback_scope.id, |scope| scope.id)
        {
            if scopes[index] != request.fallback_scope {
                return Err(RepositoryError::Conflict(
                    "Gateway fallback scope changed while planning complete desired state".into(),
                ));
            }
        }
        let mcp = self
            .projections
            .plan(PlanMcpGatewayNodeProjection {
                scopes,
                gateway_node_id: request.gateway_node_id,
                observed_at,
            })
            .await?;
        PlannedGatewayNodeDesiredState::new(inputs.physical_scope, inputs.active_routes, mcp)
            .map_err(RepositoryError::Conflict)
    }
}
