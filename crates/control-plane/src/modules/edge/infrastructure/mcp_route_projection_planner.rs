use crate::modules::assets::domain::McpServiceProfileBinding;
use crate::modules::edge::domain::services::IRouteTargetReader;
use crate::modules::edge::domain::{GatewayScope, McpRoutePolicy, RoutePortName};
use crate::modules::edge::infrastructure::{
    McpRouteTargetCandidate, McpRouteTargetProjectionCompiler,
};
use crate::modules::shared_kernel::domain::{canonical_timestamp, RepositoryError};
use crate::modules::workloads::domain::entities::WorkloadRevision;
use a3s_cloud_contracts::McpRoutePolicyProjection;
use chrono::{DateTime, Utc};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct PlanMcpRouteProjection {
    pub policy: McpRoutePolicy,
    pub profile_binding: McpServiceProfileBinding,
    pub revision: WorkloadRevision,
    pub scope: GatewayScope,
    pub observed_at: DateTime<Utc>,
}

/// Resolves every desired Gateway member through the existing Runtime health
/// evidence path before compiling one hosted MCP route projection.
#[derive(Clone)]
pub struct McpRouteProjectionPlanner {
    targets: Arc<dyn IRouteTargetReader>,
    compiler: McpRouteTargetProjectionCompiler,
}

impl McpRouteProjectionPlanner {
    pub fn new(
        targets: Arc<dyn IRouteTargetReader>,
        compiler: McpRouteTargetProjectionCompiler,
    ) -> Self {
        Self { targets, compiler }
    }

    pub async fn plan(
        &self,
        request: PlanMcpRouteProjection,
    ) -> Result<McpRoutePolicyProjection, RepositoryError> {
        request
            .scope
            .validate()
            .map_err(RepositoryError::Conflict)?;
        request
            .profile_binding
            .validate()
            .map_err(RepositoryError::Conflict)?;
        let observed_at = canonical_timestamp(request.observed_at);
        let policy_spec = request.policy.spec();
        let profile_binding = &request.profile_binding;

        if request.scope.id != policy_spec.gateway_scope_id
            || request.scope.organization_id != policy_spec.organization_id
            || request.scope.project_id != policy_spec.project_id
            || request.scope.environment_id != policy_spec.environment_id
            || profile_binding.organization_id != policy_spec.organization_id
            || profile_binding.asset_id != policy_spec.asset_id
            || profile_binding.asset_release_id != policy_spec.asset_release_id
            || request.revision.workload_id != policy_spec.workload_id
        {
            return Err(RepositoryError::Conflict(
                "MCP route policy, Gateway scope, Service profile, and Workload differ".into(),
            ));
        }
        if observed_at < request.policy.updated_at()
            || observed_at < request.revision.created_at
            || observed_at < profile_binding.created_at
            || observed_at >= policy_spec.expires_at
        {
            return Err(RepositoryError::Conflict(
                "MCP route projection time is outside its desired-state validity".into(),
            ));
        }

        let port_name = RoutePortName::parse(&profile_binding.profile.spec().runtime_port)
            .map_err(RepositoryError::Conflict)?;
        let target_set = self
            .targets
            .resolve_healthy_target_set(
                policy_spec.organization_id,
                policy_spec.project_id,
                policy_spec.environment_id,
                request.revision.id,
                &port_name,
                &request.scope.member_node_ids,
                observed_at,
            )
            .await?;
        let candidates = target_set
            .into_targets()
            .into_iter()
            .map(|target| McpRouteTargetCandidate::new(target, 0, 1))
            .collect::<Result<Vec<_>, _>>()
            .map_err(RepositoryError::Conflict)?;
        let router = format!("mcp-route-{}", policy_spec.route_id.as_uuid().simple());

        self.compiler
            .compile(
                &request.policy,
                &profile_binding.profile,
                &request.revision,
                router,
                candidates,
            )
            .map_err(RepositoryError::Conflict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::services::ResolvedRouteTarget;
    use crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::{
        fixture, now, target,
    };
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, NodeId, OrganizationId, ProjectId, WorkloadRevisionId,
    };
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RecordedTargetRequest {
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        revision_id: WorkloadRevisionId,
        port_name: RoutePortName,
        observed_at: DateTime<Utc>,
    }

    struct RecordingTargetReader {
        target: ResolvedRouteTarget,
        request: Mutex<Option<RecordedTargetRequest>>,
    }

    #[async_trait]
    impl IRouteTargetReader for RecordingTargetReader {
        async fn resolve_healthy_target(
            &self,
            organization_id: OrganizationId,
            project_id: ProjectId,
            environment_id: EnvironmentId,
            revision_id: WorkloadRevisionId,
            port_name: &RoutePortName,
            observed_at: DateTime<Utc>,
        ) -> Result<ResolvedRouteTarget, RepositoryError> {
            *self.request.lock().expect("target request lock") = Some(RecordedTargetRequest {
                organization_id,
                project_id,
                environment_id,
                revision_id,
                port_name: port_name.clone(),
                observed_at,
            });
            Ok(self.target.clone())
        }
    }

    fn scope(
        fixture: &crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::Fixture,
        node_id: NodeId,
    ) -> GatewayScope {
        let policy = fixture.policy.spec();
        GatewayScope::create(
            policy.gateway_scope_id,
            policy.organization_id,
            policy.project_id,
            policy.environment_id,
            node_id,
            now(),
        )
        .expect("scope")
    }

    fn profile_binding(
        fixture: &crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::Fixture,
    ) -> McpServiceProfileBinding {
        let policy = fixture.policy.spec();
        McpServiceProfileBinding {
            organization_id: policy.organization_id,
            asset_id: policy.asset_id,
            asset_release_id: policy.asset_release_id,
            profile: fixture.profile.clone(),
            created_at: now(),
        }
    }

    #[tokio::test]
    async fn resolves_exact_scope_members_before_compiling_projection() {
        let fixture = fixture();
        let node_id = NodeId::new();
        let reader = Arc::new(RecordingTargetReader {
            target: target(&fixture, node_id, 49152),
            request: Mutex::new(None),
        });
        let planner =
            McpRouteProjectionPlanner::new(reader.clone(), McpRouteTargetProjectionCompiler);
        let policy_spec = fixture.policy.spec().clone();

        let projection = planner
            .plan(PlanMcpRouteProjection {
                policy: fixture.policy.clone(),
                profile_binding: profile_binding(&fixture),
                revision: fixture.revision.clone(),
                scope: scope(&fixture, node_id),
                observed_at: now(),
            })
            .await
            .expect("projection");

        assert_eq!(
            reader.request.lock().expect("target request").clone(),
            Some(RecordedTargetRequest {
                organization_id: policy_spec.organization_id,
                project_id: policy_spec.project_id,
                environment_id: policy_spec.environment_id,
                revision_id: fixture.revision.id,
                port_name: RoutePortName::parse("mcp").expect("port name"),
                observed_at: now(),
            })
        );
        assert_eq!(
            projection.router,
            format!("mcp-route-{}", policy_spec.route_id.as_uuid().simple())
        );
        assert_eq!(projection.targets.len(), 1);
        assert_eq!(projection.targets[0].node_id, node_id.as_uuid());
        assert_eq!(projection.targets[0].priority, 0);
        assert_eq!(projection.targets[0].weight, 1);
    }

    #[tokio::test]
    async fn rejects_cross_environment_scope_before_reading_runtime() {
        let fixture = fixture();
        let node_id = NodeId::new();
        let reader = Arc::new(RecordingTargetReader {
            target: target(&fixture, node_id, 49152),
            request: Mutex::new(None),
        });
        let mut wrong_scope = scope(&fixture, node_id);
        wrong_scope.environment_id = EnvironmentId::new();
        let planner =
            McpRouteProjectionPlanner::new(reader.clone(), McpRouteTargetProjectionCompiler);

        let error = planner
            .plan(PlanMcpRouteProjection {
                policy: fixture.policy.clone(),
                profile_binding: profile_binding(&fixture),
                revision: fixture.revision.clone(),
                scope: wrong_scope,
                observed_at: now(),
            })
            .await
            .expect_err("cross-environment scope");

        assert!(matches!(error, RepositoryError::Conflict(_)));
        assert!(reader.request.lock().expect("target request").is_none());
    }

    #[tokio::test]
    async fn rejects_expired_policy_and_wrong_target_member() {
        let fixture = fixture();
        let scope_node_id = NodeId::new();
        let reader = Arc::new(RecordingTargetReader {
            target: target(&fixture, NodeId::new(), 49152),
            request: Mutex::new(None),
        });
        let planner =
            McpRouteProjectionPlanner::new(reader.clone(), McpRouteTargetProjectionCompiler);

        let expired = planner
            .plan(PlanMcpRouteProjection {
                policy: fixture.policy.clone(),
                profile_binding: profile_binding(&fixture),
                revision: fixture.revision.clone(),
                scope: scope(&fixture, scope_node_id),
                observed_at: fixture.policy.spec().expires_at,
            })
            .await
            .expect_err("expired policy");
        assert!(matches!(expired, RepositoryError::Conflict(_)));
        assert!(reader.request.lock().expect("target request").is_none());

        let wrong_member = planner
            .plan(PlanMcpRouteProjection {
                policy: fixture.policy.clone(),
                profile_binding: profile_binding(&fixture),
                revision: fixture.revision.clone(),
                scope: scope(&fixture, scope_node_id),
                observed_at: now(),
            })
            .await
            .expect_err("wrong target member");
        assert!(matches!(wrong_member, RepositoryError::Conflict(_)));
    }
}
