use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::domain::services::IRouteTargetReader;
use crate::modules::edge::domain::{
    DomainNamePattern, GatewayScope, RouteHostname, RoutePath, RoutePortName,
};
use crate::modules::edge::infrastructure::{
    CompileGatewayRouteRollout, CompiledGatewayRouteRollout, GatewayMemberSnapshotContext,
    GatewayRouteRolloutCompiler,
};
use crate::modules::shared_kernel::domain::{
    DomainClaimId, GatewayRolloutId, RepositoryError, RouteId, WorkloadRevisionId,
};
use chrono::{DateTime, Utc};
use futures_util::future::try_join_all;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PlanGatewayRouteRollout {
    pub scope: GatewayScope,
    pub rollout_id: GatewayRolloutId,
    pub generation: u64,
    pub correlation_id: Uuid,
    pub route_id: RouteId,
    pub workload_revision_id: WorkloadRevisionId,
    pub hostname: RouteHostname,
    pub path_prefix: RoutePath,
    pub port_name: RoutePortName,
    pub domain_claim_id: DomainClaimId,
    pub domain_pattern: DomainNamePattern,
    pub issued_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct GatewayRouteRolloutPlanner {
    routes: Arc<dyn IEdgeRepository>,
    targets: Arc<dyn IRouteTargetReader>,
    compiler: GatewayRouteRolloutCompiler,
}

impl GatewayRouteRolloutPlanner {
    pub fn new(
        routes: Arc<dyn IEdgeRepository>,
        targets: Arc<dyn IRouteTargetReader>,
        compiler: GatewayRouteRolloutCompiler,
    ) -> Self {
        Self {
            routes,
            targets,
            compiler,
        }
    }

    pub async fn plan(
        &self,
        request: PlanGatewayRouteRollout,
    ) -> Result<CompiledGatewayRouteRollout, RepositoryError> {
        request
            .scope
            .validate()
            .map_err(RepositoryError::Conflict)?;
        let target_set = self
            .targets
            .resolve_healthy_target_set(
                request.scope.organization_id,
                request.scope.project_id,
                request.scope.environment_id,
                request.workload_revision_id,
                &request.port_name,
                &request.scope.member_node_ids,
                request.issued_at,
            )
            .await?;
        let target_binding = target_set.targets().first().ok_or_else(|| {
            RepositoryError::Storage("route target reader returned an empty target set".into())
        })?;
        if target_binding.target.workload_revision_id != request.workload_revision_id
            || target_binding.target.port_name != request.port_name
        {
            return Err(RepositoryError::Conflict(
                "route target set does not match the requested workload revision and port".into(),
            ));
        }
        let contexts = try_join_all(request.scope.member_node_ids.iter().map(|node_id| {
            let routes = Arc::clone(&self.routes);
            let node_id = *node_id;
            async move {
                let (scope, active_routes) =
                    tokio::try_join!(routes.gateway_scope(node_id), routes.active_routes(node_id))?;
                Ok::<_, RepositoryError>(GatewayMemberSnapshotContext {
                    scope,
                    active_routes,
                })
            }
        }))
        .await?;
        self.compiler
            .compile(CompileGatewayRouteRollout {
                scope: request.scope,
                rollout_id: request.rollout_id,
                generation: request.generation,
                correlation_id: request.correlation_id,
                route_id: request.route_id,
                hostname: request.hostname,
                path_prefix: request.path_prefix,
                domain_claim_id: request.domain_claim_id,
                domain_pattern: request.domain_pattern,
                target_set,
                member_contexts: contexts,
                issued_at: request.issued_at,
            })
            .map_err(RepositoryError::Conflict)
    }
}
