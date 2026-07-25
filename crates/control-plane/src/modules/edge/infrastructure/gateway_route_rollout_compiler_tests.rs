use super::{
    CompileGatewayRouteRollout, GatewayMemberSnapshotContext, GatewayRouteRolloutCompiler,
    GatewayRouteRolloutPlanner, GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig,
    PlanGatewayRouteRollout,
};
use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::domain::services::{
    IRouteTargetReader, ResolvedRouteTarget, ResolvedRouteTargetSet,
};
use crate::modules::edge::domain::{
    DomainNamePattern, GatewayRolloutPolicy, GatewayScope, GatewayScopeState, Route, RouteHostname,
    RoutePath, RoutePortName, RouteState, RouteTarget, UpstreamEndpoint,
};
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayRolloutId, GatewayScopeId,
    IdempotencyRequest, NodeId, OrganizationId, ProjectId, RepositoryError, RouteId, WorkloadId,
    WorkloadRevisionId,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn planner_derives_targets_and_loads_every_physical_snapshot_context() {
    let issued_at = Utc::now();
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let members = [NodeId::new(), NodeId::new()];
    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        members[0],
        members.to_vec(),
        GatewayRolloutPolicy::new(1, 1, members.len()).expect("rollout policy"),
        issued_at,
    )
    .expect("Gateway scope");
    let workload_id = WorkloadId::new();
    let revision_id = WorkloadRevisionId::new();
    let targets = target_set(workload_id, revision_id, &members, issued_at);
    let repository: Arc<dyn IEdgeRepository> =
        Arc::new(super::persistence::InMemoryEdgeRepository::new());
    let target_reader: Arc<dyn IRouteTargetReader> =
        Arc::new(FixedTargetSetReader(targets.clone()));
    let planned = GatewayRouteRolloutPlanner::new(repository, target_reader, compiler())
        .plan(PlanGatewayRouteRollout {
            scope,
            rollout_id: GatewayRolloutId::new(),
            generation: 1,
            correlation_id: Uuid::now_v7(),
            route_id: RouteId::new(),
            workload_revision_id: revision_id,
            hostname: RouteHostname::parse("api.example.com").expect("hostname"),
            path_prefix: RoutePath::parse("/").expect("path"),
            port_name: RoutePortName::parse("http").expect("port name"),
            domain_claim_id: DomainClaimId::new(),
            domain_pattern: DomainNamePattern::parse("api.example.com").expect("domain pattern"),
            issued_at,
        })
        .await
        .expect("planned rollout");

    assert_eq!(planned.publications.len(), members.len());
    assert_eq!(
        planned
            .route_replicas
            .iter()
            .map(|route| route.target.clone())
            .collect::<Vec<_>>(),
        targets
            .targets()
            .iter()
            .map(|target| target.target.clone())
            .collect::<Vec<_>>()
    );
    assert!(planned
        .expected_scope_versions
        .values()
        .all(|version| *version == 0));
}

#[tokio::test]
async fn planner_rejects_a_target_set_for_another_requested_revision() {
    let issued_at = Utc::now();
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let members = [NodeId::new(), NodeId::new()];
    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        members[0],
        members.to_vec(),
        GatewayRolloutPolicy::new(1, 1, members.len()).expect("rollout policy"),
        issued_at,
    )
    .expect("Gateway scope");
    let requested_revision_id = WorkloadRevisionId::new();
    let returned_revision_id = WorkloadRevisionId::new();
    let targets = target_set(WorkloadId::new(), returned_revision_id, &members, issued_at);
    let repository: Arc<dyn IEdgeRepository> =
        Arc::new(super::persistence::InMemoryEdgeRepository::new());
    let target_reader: Arc<dyn IRouteTargetReader> = Arc::new(FixedTargetSetReader(targets));
    let error = GatewayRouteRolloutPlanner::new(repository, target_reader, compiler())
        .plan(PlanGatewayRouteRollout {
            scope,
            rollout_id: GatewayRolloutId::new(),
            generation: 1,
            correlation_id: Uuid::now_v7(),
            route_id: RouteId::new(),
            workload_revision_id: requested_revision_id,
            hostname: RouteHostname::parse("api.example.com").expect("hostname"),
            path_prefix: RoutePath::parse("/").expect("path"),
            port_name: RoutePortName::parse("http").expect("port name"),
            domain_claim_id: DomainClaimId::new(),
            domain_pattern: DomainNamePattern::parse("api.example.com").expect("domain pattern"),
            issued_at,
        })
        .await
        .expect_err("mismatched target revision");

    assert!(matches!(error, RepositoryError::Conflict(_)));
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
        _now: chrono::DateTime<Utc>,
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
        _now: chrono::DateTime<Utc>,
    ) -> Result<ResolvedRouteTargetSet, RepositoryError> {
        ResolvedRouteTargetSet::new(member_node_ids, self.0.clone().into_targets())
            .map_err(RepositoryError::Conflict)
    }
}

#[test]
fn compiles_one_exact_complete_snapshot_for_every_desired_member() {
    let issued_at = Utc::now();
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let members = [NodeId::new(), NodeId::new(), NodeId::new()];
    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        members[0],
        members.to_vec(),
        GatewayRolloutPolicy::new(2, 1, members.len()).expect("rollout policy"),
        issued_at,
    )
    .expect("Gateway scope");
    let workload_id = WorkloadId::new();
    let revision_id = WorkloadRevisionId::new();
    let target_set = target_set(workload_id, revision_id, &members, issued_at);
    let retained_route = active_route(
        organization_id,
        project_id,
        environment_id,
        members[1],
        workload_id,
        revision_id,
        "retained.example.com",
        49200,
        issued_at,
    );
    let contexts = vec![
        GatewayMemberSnapshotContext {
            scope: GatewayScopeState::empty(members[2]),
            active_routes: Vec::new(),
        },
        GatewayMemberSnapshotContext {
            scope: GatewayScopeState {
                node_id: members[0],
                last_issued_revision: 4,
                installed_revision: Some(4),
                aggregate_version: 8,
            },
            active_routes: Vec::new(),
        },
        GatewayMemberSnapshotContext {
            scope: GatewayScopeState {
                node_id: members[1],
                last_issued_revision: 2,
                installed_revision: Some(2),
                aggregate_version: 5,
            },
            active_routes: vec![retained_route],
        },
    ];
    let rollout_id = GatewayRolloutId::new();
    let compilation = compiler()
        .compile(CompileGatewayRouteRollout {
            scope: scope.clone(),
            rollout_id,
            generation: 7,
            correlation_id: Uuid::now_v7(),
            route_id: RouteId::new(),
            hostname: RouteHostname::parse("api.example.com").expect("hostname"),
            path_prefix: RoutePath::parse("/v1").expect("path"),
            domain_claim_id: DomainClaimId::new(),
            domain_pattern: DomainNamePattern::parse("*.example.com").expect("domain pattern"),
            target_set,
            member_contexts: contexts,
            issued_at,
        })
        .expect("compiled rollout");

    assert_eq!(compilation.scope, scope);
    assert_eq!(compilation.rollout.id, rollout_id);
    assert_eq!(compilation.rollout.generation, 7);
    assert_eq!(compilation.rollout.replicas.len(), members.len());
    assert_eq!(compilation.route_replicas.len(), members.len());
    assert_eq!(compilation.publications.len(), members.len());
    assert_eq!(compilation.certificates.len(), members.len());
    assert_eq!(
        compilation
            .primary_route()
            .expect("primary route")
            .gateway_node_id,
        members[0]
    );
    for publication in &compilation.publications {
        let target = compilation
            .route_replicas
            .iter()
            .find(|route| route.gateway_node_id == publication.node_id)
            .expect("member route");
        assert!(publication.acl.contains(target.target.upstream.as_str()));
        assert!(publication.acl.contains("Host(`api.example.com`)"));
        assert_eq!(
            publication.acl.matches("http://127.0.0.1:49").count(),
            if publication.node_id == members[1] {
                2
            } else {
                1
            }
        );
        assert_eq!(
            publication
                .certificate_request
                .as_ref()
                .expect("certificate request")
                .dns_names,
            if publication.node_id == members[1] {
                vec![
                    "*.example.com".to_owned(),
                    "retained.example.com".to_owned(),
                ]
            } else {
                vec!["*.example.com".to_owned()]
            }
        );
    }
    assert_eq!(
        compilation
            .publications
            .iter()
            .find(|publication| publication.node_id == members[0])
            .expect("primary publication")
            .revision,
        5
    );
    assert_eq!(
        compilation.expected_scope_versions.get(&members[0]),
        Some(&8)
    );
    compilation.rollout.validate().expect("valid rollout");
    compilation
        .stage_bundle(
            IdempotencyRequest::new(
                format!("gateway-scopes/{}/rollouts", scope.id),
                "compiled-rollout",
                rollout_id.to_string().as_bytes(),
            )
            .expect("rollout idempotency"),
        )
        .expect("stage bundle");
}

#[test]
fn rejects_partial_targets_contexts_and_cross_organization_routes() {
    let issued_at = Utc::now();
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let members = [NodeId::new(), NodeId::new()];
    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        members[0],
        members.to_vec(),
        GatewayRolloutPolicy::new(1, 1, members.len()).expect("rollout policy"),
        issued_at,
    )
    .expect("Gateway scope");
    let workload_id = WorkloadId::new();
    let revision_id = WorkloadRevisionId::new();
    let request = |target_set, member_contexts| CompileGatewayRouteRollout {
        scope: scope.clone(),
        rollout_id: GatewayRolloutId::new(),
        generation: 1,
        correlation_id: Uuid::now_v7(),
        route_id: RouteId::new(),
        hostname: RouteHostname::parse("api.example.com").expect("hostname"),
        path_prefix: RoutePath::parse("/").expect("path"),
        domain_claim_id: DomainClaimId::new(),
        domain_pattern: DomainNamePattern::parse("api.example.com").expect("domain pattern"),
        target_set,
        member_contexts,
        issued_at,
    };
    let targets = target_set(workload_id, revision_id, &members, issued_at);
    assert!(compiler()
        .compile(request(
            targets.clone(),
            vec![GatewayMemberSnapshotContext {
                scope: GatewayScopeState::empty(members[0]),
                active_routes: Vec::new(),
            }],
        ))
        .is_err());

    let foreign = active_route(
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        members[1],
        workload_id,
        revision_id,
        "foreign.example.com",
        49300,
        issued_at,
    );
    assert!(compiler()
        .compile(request(
            targets,
            vec![
                GatewayMemberSnapshotContext {
                    scope: GatewayScopeState::empty(members[0]),
                    active_routes: Vec::new(),
                },
                GatewayMemberSnapshotContext {
                    scope: GatewayScopeState::empty(members[1]),
                    active_routes: vec![foreign],
                },
            ],
        ))
        .is_err());
}

fn compiler() -> GatewayRouteRolloutCompiler {
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

fn target_set(
    workload_id: WorkloadId,
    revision_id: WorkloadRevisionId,
    members: &[NodeId],
    observed_at: chrono::DateTime<Utc>,
) -> ResolvedRouteTargetSet {
    ResolvedRouteTargetSet::new(
        members,
        members
            .iter()
            .enumerate()
            .map(|(index, node_id)| ResolvedRouteTarget {
                workload_id,
                node_id: *node_id,
                target: route_target(
                    workload_id,
                    revision_id,
                    49152 + u16::try_from(index).expect("member index"),
                    observed_at,
                ),
            })
            .collect(),
    )
    .expect("target set")
}

#[allow(clippy::too_many_arguments)]
fn active_route(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    node_id: NodeId,
    workload_id: WorkloadId,
    revision_id: WorkloadRevisionId,
    hostname: &str,
    port: u16,
    created_at: chrono::DateTime<Utc>,
) -> Route {
    let mut route = Route::create(
        RouteId::new(),
        organization_id,
        project_id,
        environment_id,
        GatewayScopeId::new(),
        node_id,
        RouteHostname::parse(hostname).expect("hostname"),
        RoutePath::parse("/").expect("path"),
        DomainClaimId::new(),
        DomainNamePattern::parse(hostname).expect("domain pattern"),
        GatewayCertificateId::new(),
        workload_id,
        route_target(workload_id, revision_id, port, created_at),
        created_at,
    )
    .expect("route");
    route.state = RouteState::Active;
    route
}

fn route_target(
    workload_id: WorkloadId,
    revision_id: WorkloadRevisionId,
    port: u16,
    observed_at: chrono::DateTime<Utc>,
) -> RouteTarget {
    RouteTarget::new(
        workload_id,
        revision_id,
        format!("workload:{workload_id}:revision:{revision_id}"),
        3,
        RoutePortName::parse("http").expect("port name"),
        UpstreamEndpoint::parse(format!("http://127.0.0.1:{port}")).expect("upstream"),
        observed_at,
    )
    .expect("route target")
}
