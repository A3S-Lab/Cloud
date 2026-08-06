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
        if scopes
            .iter()
            .any(|scope| scope.organization_id != request.fallback_scope.organization_id)
        {
            return Err(RepositoryError::Conflict(
                "Gateway desired state crosses its physical node organization".into(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::GatewayScopeState;
    use crate::modules::edge::infrastructure::{
        McpGatewayProjectionAssembler, McpGatewayReconciliationScope,
        McpGatewaySnapshotDispatchTarget, McpGatewaySnapshotInputs,
        McpGatewaySnapshotReconciliationState, McpGatewaySnapshotStageResult,
        PlannedMcpGatewayProjectionSet, StageMcpGatewaySnapshot,
    };
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, GatewayScopeId, NodeCommandId, OrganizationId, ProjectId,
    };
    use async_trait::async_trait;
    use std::sync::Mutex;

    struct FakeRepository {
        inputs: McpGatewaySnapshotInputs,
        scopes: Vec<GatewayScope>,
    }

    #[async_trait]
    impl IMcpGatewaySnapshotRepository for FakeRepository {
        async fn mcp_gateway_reconciliation_scopes(
            &self,
            _observed_at: DateTime<Utc>,
            _after_gateway_scope_id: Option<GatewayScopeId>,
            _limit: usize,
        ) -> Result<Vec<McpGatewayReconciliationScope>, RepositoryError> {
            Ok(Vec::new())
        }

        async fn mcp_gateway_reconciliation_scope_set(
            &self,
            _node_id: NodeId,
            _observed_at: DateTime<Utc>,
        ) -> Result<Vec<GatewayScope>, RepositoryError> {
            Ok(self.scopes.clone())
        }

        async fn mcp_gateway_snapshot_reconciliation_state(
            &self,
            _node_id: NodeId,
        ) -> Result<McpGatewaySnapshotReconciliationState, RepositoryError> {
            Ok(McpGatewaySnapshotReconciliationState {
                pending_publication: false,
                latest_mcp_snapshot: None,
            })
        }

        async fn mcp_gateway_snapshot_inputs(
            &self,
            _node_id: NodeId,
        ) -> Result<McpGatewaySnapshotInputs, RepositoryError> {
            Ok(self.inputs.clone())
        }

        async fn stage_mcp_gateway_snapshot(
            &self,
            _stage: StageMcpGatewaySnapshot,
        ) -> Result<McpGatewaySnapshotStageResult, RepositoryError> {
            Err(RepositoryError::Storage(
                "staging is outside this planner test".into(),
            ))
        }

        async fn pending_mcp_gateway_snapshots(
            &self,
            _limit: usize,
        ) -> Result<Vec<McpGatewaySnapshotDispatchTarget>, RepositoryError> {
            Ok(Vec::new())
        }

        #[allow(clippy::too_many_arguments)]
        async fn mark_mcp_gateway_snapshot_unavailable(
            &self,
            _organization_id: OrganizationId,
            _gateway_scope_id: GatewayScopeId,
            _node_id: NodeId,
            _gateway_revision: u64,
            _gateway_command_id: NodeCommandId,
            _failure: &str,
            _observed_at: DateTime<Utc>,
        ) -> Result<McpGatewaySnapshotStageResult, RepositoryError> {
            Err(RepositoryError::Storage(
                "dispatch is outside this planner test".into(),
            ))
        }
    }

    #[derive(Default)]
    struct CapturingProjectionPlanner {
        requests: Mutex<Vec<PlanMcpGatewayNodeProjection>>,
    }

    impl CapturingProjectionPlanner {
        fn requested_scope_ids(&self) -> Vec<GatewayScopeId> {
            self.requests
                .lock()
                .expect("projection requests")
                .last()
                .map(|request| request.scopes.iter().map(|scope| scope.id).collect())
                .unwrap_or_default()
        }

        fn call_count(&self) -> usize {
            self.requests.lock().expect("projection requests").len()
        }
    }

    #[async_trait]
    impl IMcpGatewayNodeProjectionPlanner for CapturingProjectionPlanner {
        async fn plan(
            &self,
            request: PlanMcpGatewayNodeProjection,
        ) -> Result<PlannedMcpGatewayNodeProjection, RepositoryError> {
            self.requests
                .lock()
                .expect("projection requests")
                .push(request.clone());
            let scope_sets = request
                .scopes
                .into_iter()
                .map(|scope| {
                    if scope.contains_member(request.gateway_node_id) {
                        PlannedMcpGatewayProjectionSet::empty(
                            scope,
                            request.gateway_node_id,
                            request.observed_at,
                        )
                    } else {
                        PlannedMcpGatewayProjectionSet::empty_for_departed_member(
                            scope,
                            request.gateway_node_id,
                            request.observed_at,
                        )
                    }
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(RepositoryError::Conflict)?;
            PlannedMcpGatewayNodeProjection::aggregate(scope_sets, McpGatewayProjectionAssembler)
                .map_err(RepositoryError::Conflict)
        }
    }

    #[tokio::test]
    async fn empty_mcp_scope_set_uses_the_ordinary_scope_as_empty_cas_evidence() {
        let now = canonical_timestamp(Utc::now());
        let node_id = NodeId::new();
        let fallback_scope = scope(now, node_id, OrganizationId::new());
        let repository = Arc::new(repository(node_id, Vec::new()));
        let projections = Arc::new(CapturingProjectionPlanner::default());

        let planned = GatewayNodeDesiredStatePlanner::new(repository, projections.clone())
            .plan(PlanGatewayNodeDesiredState {
                gateway_node_id: node_id,
                fallback_scope: fallback_scope.clone(),
                observed_at: now,
            })
            .await
            .expect("ordinary-only node desired state");

        assert_eq!(projections.requested_scope_ids(), vec![fallback_scope.id]);
        assert_eq!(planned.mcp().scope_sets().len(), 1);
        assert_eq!(planned.mcp().primary_scope(), &fallback_scope);
        assert!(planned.mcp().projection().is_none());
    }

    #[tokio::test]
    async fn node_wide_mcp_scope_set_is_not_polluted_by_an_unrelated_ordinary_scope() {
        let now = canonical_timestamp(Utc::now());
        let node_id = NodeId::new();
        let organization_id = OrganizationId::new();
        let fallback_scope = scope(now, node_id, organization_id);
        let active_scope = scope(now, node_id, organization_id);
        let mut departed_scope = scope(now, NodeId::new(), organization_id);
        while departed_scope.contains_member(node_id) {
            departed_scope = scope(now, NodeId::new(), organization_id);
        }
        let mut mcp_scopes = vec![active_scope.clone(), departed_scope.clone()];
        mcp_scopes.sort_by_key(|scope| scope.id);
        let expected_scope_ids = mcp_scopes.iter().map(|scope| scope.id).collect::<Vec<_>>();
        let repository = Arc::new(repository(node_id, mcp_scopes));
        let projections = Arc::new(CapturingProjectionPlanner::default());

        let planned = GatewayNodeDesiredStatePlanner::new(repository, projections.clone())
            .plan(PlanGatewayNodeDesiredState {
                gateway_node_id: node_id,
                fallback_scope: fallback_scope.clone(),
                observed_at: now,
            })
            .await
            .expect("node-wide MCP desired state");

        assert_eq!(projections.requested_scope_ids(), expected_scope_ids);
        assert!(planned
            .mcp()
            .scope_sets()
            .iter()
            .all(|scope_set| scope_set.scope().id != fallback_scope.id));
        assert!(planned
            .mcp()
            .scope_sets()
            .iter()
            .any(|scope_set| scope_set.scope().id == departed_scope.id));
    }

    #[tokio::test]
    async fn same_identity_fallback_change_fails_closed() {
        let now = canonical_timestamp(Utc::now());
        let node_id = NodeId::new();
        let fallback_scope = scope(now, node_id, OrganizationId::new());
        let mut changed_scope = fallback_scope.clone();
        changed_scope.aggregate_version += 1;
        let repository = Arc::new(repository(node_id, vec![changed_scope]));
        let projections = Arc::new(CapturingProjectionPlanner::default());

        let result = GatewayNodeDesiredStatePlanner::new(repository, projections.clone())
            .plan(PlanGatewayNodeDesiredState {
                gateway_node_id: node_id,
                fallback_scope,
                observed_at: now,
            })
            .await;

        assert!(matches!(result, Err(RepositoryError::Conflict(_))));
        assert_eq!(projections.call_count(), 0);
    }

    #[tokio::test]
    async fn cross_organization_mcp_scope_fails_closed() {
        let now = canonical_timestamp(Utc::now());
        let node_id = NodeId::new();
        let fallback_scope = scope(now, node_id, OrganizationId::new());
        let foreign_scope = scope(now, node_id, OrganizationId::new());
        let repository = Arc::new(repository(node_id, vec![foreign_scope]));
        let projections = Arc::new(CapturingProjectionPlanner::default());

        let result = GatewayNodeDesiredStatePlanner::new(repository, projections.clone())
            .plan(PlanGatewayNodeDesiredState {
                gateway_node_id: node_id,
                fallback_scope,
                observed_at: now,
            })
            .await;

        assert!(matches!(result, Err(RepositoryError::Conflict(_))));
        assert_eq!(projections.call_count(), 0);
    }

    fn repository(node_id: NodeId, scopes: Vec<GatewayScope>) -> FakeRepository {
        FakeRepository {
            inputs: McpGatewaySnapshotInputs {
                physical_scope: GatewayScopeState::empty(node_id),
                active_routes: Vec::new(),
            },
            scopes,
        }
    }

    fn scope(now: DateTime<Utc>, node_id: NodeId, organization_id: OrganizationId) -> GatewayScope {
        GatewayScope::create(
            GatewayScopeId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            node_id,
            now,
        )
        .expect("Gateway scope")
    }
}
