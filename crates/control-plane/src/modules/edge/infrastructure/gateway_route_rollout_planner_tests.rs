use super::{
    GatewayNodeDesiredStatePlanner, GatewayRouteRolloutCompiler, GatewayRouteRolloutPlanner,
    GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig, GatewaySnapshotRouteInput,
    IMcpGatewayNodeProjectionPlanner, IMcpGatewaySnapshotRepository, McpGatewayProjectionAssembler,
    McpGatewayReconciliationScope, McpGatewaySnapshotDispatchTarget, McpGatewaySnapshotInputs,
    McpGatewaySnapshotReconciliationState, McpGatewaySnapshotStageResult,
    PlanManagedGatewayRouteRollout, PlanMcpGatewayNodeProjection, PlannedMcpGatewayNodeProjection,
    PlannedMcpGatewayProjectionSet, StageMcpGatewaySnapshot,
};
use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::domain::services::{
    IRouteTargetReader, ResolvedRouteTarget, ResolvedRouteTargetSet,
};
use crate::modules::edge::domain::{
    DomainClaim, DomainNamePattern, GatewayScope, GatewayScopeState, Route, RouteHostname,
    RoutePath, RoutePortName, RouteState, RouteTarget, UpstreamEndpoint,
};
use crate::modules::identity::application::{
    IInferenceCredentialAclProjectionPort, InferenceCredentialEnvironmentScope,
};
use crate::modules::inference::application::{
    EmptyInferenceWorkerAclProjectionPort, IInferenceRouteAclProjectionPort,
    InferenceRouteEnvironmentScope,
};
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayRolloutId, GatewayScopeId,
    NodeCommandId, NodeId, OrganizationId, ProjectId, RepositoryError, RouteId, WorkloadId,
    WorkloadRevisionId,
};
use a3s_cloud_contracts::{
    InferenceCredentialAclProjection, InferenceEndpointAcl, InferenceGrantAclProjection,
    InferenceLimitsAclProjection, InferenceModelAclProjection, InferenceRouteAclProjection,
    InferenceTargetAclProjection, INFERENCE_CREDENTIAL_AUDIENCE,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::sync::Arc;
use uuid::Uuid;

struct FakeDesiredStateRepository {
    scope: GatewayScope,
    inputs: McpGatewaySnapshotInputs,
}

impl FakeDesiredStateRepository {
    fn new(scope: GatewayScope, inputs: McpGatewaySnapshotInputs) -> Self {
        Self { scope, inputs }
    }
}

#[async_trait]
impl IMcpGatewaySnapshotRepository for FakeDesiredStateRepository {
    async fn mcp_gateway_reconciliation_scopes(
        &self,
        _observed_at: DateTime<Utc>,
        _after_gateway_scope_id: Option<GatewayScopeId>,
        _limit: usize,
    ) -> Result<Vec<McpGatewayReconciliationScope>, RepositoryError> {
        Ok(vec![McpGatewayReconciliationScope {
            node_ids: self.scope.member_node_ids.clone(),
            scope: self.scope.clone(),
        }])
    }

    async fn mcp_gateway_reconciliation_scope_set(
        &self,
        node_id: NodeId,
        _observed_at: DateTime<Utc>,
    ) -> Result<Vec<GatewayScope>, RepositoryError> {
        if self.scope.contains_member(node_id) {
            Ok(vec![self.scope.clone()])
        } else {
            Ok(Vec::new())
        }
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
            "MCP snapshot staging is unused by managed route-rollout planner tests".into(),
        ))
    }

    async fn pending_mcp_gateway_snapshots(
        &self,
        _limit: usize,
    ) -> Result<Vec<McpGatewaySnapshotDispatchTarget>, RepositoryError> {
        Ok(Vec::new())
    }

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
        Err(RepositoryError::NotFound)
    }
}

#[derive(Default)]
struct EmptyProjectionPlanner;

#[async_trait]
impl IMcpGatewayNodeProjectionPlanner for EmptyProjectionPlanner {
    async fn plan(
        &self,
        request: PlanMcpGatewayNodeProjection,
    ) -> Result<PlannedMcpGatewayNodeProjection, RepositoryError> {
        let planned = request
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
        PlannedMcpGatewayNodeProjection::aggregate(planned, McpGatewayProjectionAssembler)
            .map_err(RepositoryError::Conflict)
    }
}

struct FixedTargetSetReader(ResolvedRouteTargetSet);

#[async_trait]
impl IRouteTargetReader for FixedTargetSetReader {
    async fn resolve_healthy_target(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
        _revision_id: WorkloadRevisionId,
        _port_name: &RoutePortName,
        _now: DateTime<Utc>,
    ) -> Result<ResolvedRouteTarget, RepositoryError> {
        self.0
            .targets()
            .first()
            .cloned()
            .ok_or_else(|| RepositoryError::Conflict("empty target set".into()))
    }

    async fn resolve_healthy_target_set(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
        _revision_id: WorkloadRevisionId,
        _port_name: &RoutePortName,
        member_node_ids: &[NodeId],
        _now: DateTime<Utc>,
    ) -> Result<ResolvedRouteTargetSet, RepositoryError> {
        ResolvedRouteTargetSet::new(member_node_ids, self.0.clone().into_targets())
            .map_err(RepositoryError::Conflict)
    }
}

struct StubCredentialPort {
    projection: InferenceCredentialAclProjection,
}

#[async_trait]
impl IInferenceCredentialAclProjectionPort for StubCredentialPort {
    async fn list_inference_credential_acl_projections(
        &self,
        _scopes: &[InferenceCredentialEnvironmentScope],
    ) -> Result<Vec<InferenceCredentialAclProjection>, RepositoryError> {
        Ok(vec![self.projection.clone()])
    }
}

struct StubRoutePort {
    projection: InferenceRouteAclProjection,
}

#[async_trait]
impl IInferenceRouteAclProjectionPort for StubRoutePort {
    async fn list_inference_route_acl_projections(
        &self,
        _scopes: &[InferenceRouteEnvironmentScope],
    ) -> Result<Vec<InferenceRouteAclProjection>, RepositoryError> {
        Ok(vec![self.projection.clone()])
    }
}

#[tokio::test]
async fn managed_route_rollout_stages_inference_credential_and_route_acl_without_workers() {
    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const PREFIX: &str = "a3s_inf_rollstage01";
    let credential_id = Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap();
    let inference_route_id = Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap();

    let issued_at = Utc::now();
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let node_id = NodeId::new();
    let scope = GatewayScope::create(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        node_id,
        issued_at,
    )
    .expect("Gateway scope");
    let workload_id = WorkloadId::new();
    let revision_id = WorkloadRevisionId::new();
    let hostname = RouteHostname::parse("api.example.com").expect("hostname");
    let claim_id = DomainClaimId::new();
    let mut domain_claim = DomainClaim::create(
        claim_id,
        organization_id,
        project_id,
        environment_id,
        DomainNamePattern::parse(hostname.as_str()).expect("domain pattern"),
        format!("a3s-cloud-verification={claim_id}"),
        issued_at - Duration::minutes(1),
    )
    .expect("domain claim");
    domain_claim
        .verify(issued_at - Duration::seconds(1))
        .expect("verified claim");
    let ordinary_workload_id = WorkloadId::new();
    let ordinary_revision_id = WorkloadRevisionId::new();
    let mut ordinary = Route::create(
        RouteId::new(),
        organization_id,
        project_id,
        environment_id,
        scope.id,
        node_id,
        hostname.clone(),
        RoutePath::parse("/v1").expect("path"),
        domain_claim.id,
        domain_claim.pattern.clone(),
        GatewayCertificateId::new(),
        ordinary_workload_id,
        RouteTarget::new(
            ordinary_workload_id,
            ordinary_revision_id,
            format!("workload:{ordinary_workload_id}:revision:{ordinary_revision_id}"),
            1,
            RoutePortName::parse("http").expect("port"),
            UpstreamEndpoint::parse("http://127.0.0.1:8080").expect("upstream"),
            issued_at,
        )
        .expect("target"),
        issued_at,
    )
    .expect("ordinary route");
    ordinary.state = RouteState::Active;

    let managed = Arc::new(FakeDesiredStateRepository::new(
        scope.clone(),
        McpGatewaySnapshotInputs {
            physical_scope: GatewayScopeState {
                node_id,
                last_issued_revision: 1,
                installed_revision: Some(1),
                aggregate_version: 1,
            },
            active_routes: vec![GatewaySnapshotRouteInput {
                route: ordinary,
                domain_claim: domain_claim.clone(),
            }],
        },
    ));
    let desired_state =
        GatewayNodeDesiredStatePlanner::new(managed, Arc::new(EmptyProjectionPlanner));
    let targets = ResolvedRouteTargetSet::new(
        &[node_id],
        vec![ResolvedRouteTarget {
            workload_id,
            node_id,
            target: RouteTarget::new(
                workload_id,
                revision_id,
                format!("workload:{workload_id}:revision:{revision_id}"),
                3,
                RoutePortName::parse("http").expect("port name"),
                UpstreamEndpoint::parse("http://127.0.0.1:49152").expect("upstream"),
                issued_at,
            )
            .expect("route target"),
        }],
    )
    .expect("target set");
    let edge: Arc<dyn IEdgeRepository> =
        Arc::new(super::persistence::InMemoryEdgeRepository::new());
    let target_reader: Arc<dyn IRouteTargetReader> = Arc::new(FixedTargetSetReader(targets));

    let credential = InferenceCredentialAclProjection::new(
        credential_id,
        environment_id.as_uuid(),
        INFERENCE_CREDENTIAL_AUDIENCE,
        PREFIX,
        VERIFIER,
        3,
        issued_at + Duration::hours(2),
        false,
    )
    .expect("credential projection");
    let inference_route = InferenceRouteAclProjection {
        route_id: inference_route_id,
        router: "inference".into(),
        environment_id: environment_id.as_uuid(),
        policy_revision: 11,
        models: vec![InferenceModelAclProjection {
            alias: "chat-model".into(),
            model_id: Uuid::parse_str("55555555-5555-4555-8555-555555555555").unwrap(),
            targets: vec![InferenceTargetAclProjection {
                target_id: Uuid::parse_str("66666666-6666-4666-8666-666666666666").unwrap(),
                service: "model-service".into(),
                upstream_model: "internal/model-v1".into(),
                priority: 0,
                weight: 100,
            }],
        }],
        grants: vec![InferenceGrantAclProjection {
            credential_id,
            credential_generation: 3,
            models: vec!["chat-model".into()],
            endpoints: vec![InferenceEndpointAcl::Models],
            limits: InferenceLimitsAclProjection {
                max_concurrent_requests: 2,
                requests_per_minute: 60,
                request_burst: 2,
                tokens_per_minute: 10_000,
            },
        }],
    };

    let planned = GatewayRouteRolloutPlanner::new_managed(
        edge,
        target_reader,
        rollout_compiler(),
        desired_state,
        Arc::new(
            crate::modules::edge::IdentityInferenceEdgeManagedAclAccessAdapter::new(
                Arc::new(StubCredentialPort {
                    projection: credential,
                }),
                Arc::new(StubRoutePort {
                    projection: inference_route,
                }),
                Arc::new(EmptyInferenceWorkerAclProjectionPort),
            ),
        ),
    )
    .plan_managed(PlanManagedGatewayRouteRollout {
        scope,
        rollout_id: GatewayRolloutId::new(),
        generation: 1,
        correlation_id: Uuid::now_v7(),
        route_id: RouteId::new(),
        workload_revision_id: revision_id,
        hostname,
        path_prefix: RoutePath::parse("/publish").expect("path"),
        port_name: RoutePortName::parse("http").expect("port name"),
        domain_claim,
        issued_at,
    })
    .await
    .expect("managed route rollout with inference ACL");

    assert_eq!(planned.publications.len(), 1);
    let acl = &planned.publications[0].acl;
    assert!(
        acl.contains("inference {"),
        "publication ACL must embed inference policy shell"
    );
    assert!(
        acl.contains(&format!("prefix = \"{PREFIX}\"")),
        "publication ACL must embed Identity credential prefix"
    );
    assert!(
        acl.contains(&format!("routes \"{inference_route_id}\"")),
        "publication ACL must embed Inference route grants"
    );
    assert!(
        acl.contains("models \"chat-model\""),
        "publication ACL must embed model grant aliases"
    );
    assert!(
        !acl.contains("\n  workers ") && !acl.contains("\nworkers "),
        "empty Power worker port must not invent workers blocks"
    );
    assert!(
        !acl.contains("bearer") && !acl.contains("a3s_inf_rollstage01."),
        "publication ACL must not embed bearer secret material"
    );
}

fn rollout_compiler() -> GatewayRouteRolloutCompiler {
    GatewayRouteRolloutCompiler::new(
        GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
            entrypoint_address: "0.0.0.0:8081".into(),
            management_address: "127.0.0.1:9090".into(),
            management_path_prefix: "/api/gateway".into(),
            management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
            upstream_request_timeout_ms: 30_000,
            certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
            managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
        })
        .expect("snapshot compiler"),
        Duration::minutes(3),
        Duration::hours(24),
    )
    .expect("rollout compiler")
}
