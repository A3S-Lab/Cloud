use crate::modules::edge::infrastructure::{
    GatewaySnapshotRouteInput, IMcpGatewayProjectionSetPlanner, IMcpGatewaySnapshotRepository,
    McpGatewayNodeProjectionAssembler, McpGatewaySnapshotAnchor, PlanMcpGatewayProjectionSet,
    PlannedMcpGatewayNodeProjection,
};
use crate::modules::shared_kernel::domain::{canonical_timestamp, NodeId, RepositoryError};
use chrono::{DateTime, Utc};
use futures_util::{stream, StreamExt, TryStreamExt};
use std::sync::Arc;

const SCOPE_PLANNING_CONCURRENCY: usize = 16;

#[derive(Debug, Clone, Copy)]
pub struct PlanGatewayNodeDesiredState {
    pub gateway_node_id: NodeId,
    pub fallback_anchor: McpGatewaySnapshotAnchor,
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
    projections: Arc<dyn IMcpGatewayProjectionSetPlanner>,
    assembler: McpGatewayNodeProjectionAssembler,
}

impl GatewayNodeDesiredStatePlanner {
    pub fn new(
        repository: Arc<dyn IMcpGatewaySnapshotRepository>,
        projections: Arc<dyn IMcpGatewayProjectionSetPlanner>,
    ) -> Self {
        Self {
            repository,
            projections,
            assembler: McpGatewayNodeProjectionAssembler::default(),
        }
    }

    pub async fn plan(
        &self,
        request: PlanGatewayNodeDesiredState,
    ) -> Result<PlannedGatewayNodeDesiredState, RepositoryError> {
        request
            .fallback_anchor
            .validate()
            .map_err(RepositoryError::Conflict)?;
        if request.gateway_node_id.as_uuid().is_nil() {
            return Err(RepositoryError::Conflict(
                "Gateway desired-state node identity is invalid".into(),
            ));
        }
        let observed_at = canonical_timestamp(request.observed_at);
        let (inputs, scopes) = tokio::try_join!(
            self.repository
                .mcp_gateway_snapshot_inputs(request.gateway_node_id),
            self.repository
                .mcp_gateway_active_scopes(request.gateway_node_id, observed_at),
        )?;
        inputs
            .validate(request.gateway_node_id)
            .map_err(RepositoryError::Storage)?;
        if scopes.iter().any(|scope| {
            scope.organization_id != request.fallback_anchor.organization_id
                || !scope.contains_member(request.gateway_node_id)
        }) {
            return Err(RepositoryError::Conflict(
                "Gateway desired state crosses its physical node organization or membership".into(),
            ));
        }
        let anchor = scopes
            .first()
            .map(McpGatewaySnapshotAnchor::from_scope)
            .unwrap_or(request.fallback_anchor);
        let sets = stream::iter(scopes.into_iter().map(|scope| {
            let projections = Arc::clone(&self.projections);
            async move {
                projections
                    .plan(PlanMcpGatewayProjectionSet {
                        scope,
                        gateway_node_id: request.gateway_node_id,
                        observed_at,
                    })
                    .await
            }
        }))
        .buffered(SCOPE_PLANNING_CONCURRENCY)
        .try_collect()
        .await?;
        let mcp = self
            .assembler
            .assemble(anchor, request.gateway_node_id, observed_at, sets)
            .map_err(RepositoryError::Conflict)?;
        PlannedGatewayNodeDesiredState::new(inputs.physical_scope, inputs.active_routes, mcp)
            .map_err(RepositoryError::Conflict)
    }
}
