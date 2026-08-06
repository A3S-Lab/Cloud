use super::*;
use crate::modules::assets::domain::McpServiceProfileBinding;
use crate::modules::edge::domain::repositories::IMcpCredentialRepository;
use crate::modules::edge::domain::services::{
    IRouteTargetReader, ResolvedMcpRouteProjectionInput, ResolvedRouteTarget,
};
use crate::modules::edge::domain::{
    DomainClaim, DomainNamePattern, GatewayPublication, GatewayScopeState, McpCredential,
    McpRoutePolicy, Route, RouteHostname, RoutePath, RoutePortName, RouteState,
};
use crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::{
    fixture, now, target,
};
use crate::modules::edge::infrastructure::{
    CompileManagedGatewayRouteSnapshot, CompileMcpGatewaySnapshot,
    GatewayManagedSnapshotComposition, GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig,
    GatewaySnapshotMetadata, GatewaySnapshotPublicationOwner, GatewaySnapshotRouteInput,
    McpRouteProjectionPlanner, McpRouteTargetProjectionCompiler, PlannedGatewayNodeDesiredState,
    PlannedMcpGatewayNodeProjection, StageMcpGatewaySnapshot,
};
use crate::modules::edge::InMemoryEdgeRepository;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayCertificateId, McpCredentialId, NodeCommandId,
    OrganizationId, ProjectId, RouteId, WorkloadRevisionId,
};
use async_trait::async_trait;
use chrono::Duration;
use std::sync::atomic::{AtomicUsize, Ordering};

const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

struct FixedInputReader(Vec<ResolvedMcpRouteProjectionInput>);

#[async_trait]
impl IMcpRouteProjectionInputReader for FixedInputReader {
    async fn list_active_projection_inputs(
        &self,
        _scope: &GatewayScope,
        _observed_at: DateTime<Utc>,
    ) -> Result<Vec<ResolvedMcpRouteProjectionInput>, RepositoryError> {
        Ok(self.0.clone())
    }
}

struct CountingTargetReader {
    target: Option<ResolvedRouteTarget>,
    calls: AtomicUsize,
}

#[async_trait]
impl IRouteTargetReader for CountingTargetReader {
    async fn resolve_healthy_target(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
        _revision_id: WorkloadRevisionId,
        _port_name: &RoutePortName,
        _now: DateTime<Utc>,
    ) -> Result<ResolvedRouteTarget, RepositoryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.target
            .clone()
            .ok_or_else(|| RepositoryError::Storage("target resolution must not be called".into()))
    }
}

fn scope(
    fixture: &crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::Fixture,
    node_id: NodeId,
) -> GatewayScope {
    let spec = fixture.policy.spec();
    GatewayScope::create(
        spec.gateway_scope_id,
        spec.organization_id,
        spec.project_id,
        spec.environment_id,
        node_id,
        now(),
    )
    .expect("scope")
}

fn input(
    fixture: &crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::Fixture,
) -> ResolvedMcpRouteProjectionInput {
    let spec = fixture.policy.spec();
    let mut domain_claim = DomainClaim::create(
        spec.domain_claim_id,
        spec.organization_id,
        spec.project_id,
        spec.environment_id,
        DomainNamePattern::parse(spec.hostname.as_str()).expect("domain pattern"),
        format!("a3s-cloud-verification={}", spec.domain_claim_id),
        now() - Duration::minutes(1),
    )
    .expect("domain claim");
    domain_claim
        .verify(now() - Duration::seconds(1))
        .expect("verify domain claim");
    ResolvedMcpRouteProjectionInput {
        policy: fixture.policy.clone(),
        domain_claim,
        profile_binding: McpServiceProfileBinding {
            organization_id: spec.organization_id,
            asset_id: spec.asset_id,
            asset_release_id: spec.asset_release_id,
            profile: fixture.profile.clone(),
            created_at: now(),
        },
        revision: fixture.revision.clone(),
        workload_aggregate_version: 2,
    }
}

fn credential(
    fixture: &crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::Fixture,
) -> McpCredential {
    let spec = fixture.policy.spec();
    McpCredential::issue(
        McpCredentialId::from_uuid(spec.grants[0].credential_id),
        spec.organization_id,
        spec.project_id,
        spec.environment_id,
        "a3s_mcp_abc12345def67890",
        VERIFIER,
        now() + Duration::minutes(30),
        now(),
    )
    .expect("credential")
}

fn planner(
    inputs: Vec<ResolvedMcpRouteProjectionInput>,
    targets: Arc<CountingTargetReader>,
    credentials: Arc<dyn IMcpCredentialRepository>,
) -> McpGatewayProjectionSetPlanner {
    let route_planner = McpRouteProjectionPlanner::new(targets, McpRouteTargetProjectionCompiler);
    McpGatewayProjectionSetPlanner::new(
        Arc::new(FixedInputReader(inputs)),
        McpGatewayProjectionPlanner::new(route_planner, credentials),
        McpGatewayProjectionAssembler,
    )
}

fn snapshot_compiler() -> GatewaySnapshotCompiler {
    GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: "0.0.0.0:8081".into(),
        management_address: "127.0.0.1:9090".into(),
        management_path_prefix: "/api/gateway".into(),
        management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
        upstream_request_timeout_ms: 30_000,
        certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
        managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
    })
    .expect("snapshot compiler")
}

#[tokio::test]
async fn plans_the_complete_active_set_for_one_receiving_gateway() {
    let fixture = fixture();
    let node_id = NodeId::new();
    let targets = Arc::new(CountingTargetReader {
        target: Some(target(&fixture, node_id, 49152)),
        calls: AtomicUsize::new(0),
    });
    let credentials = Arc::new(InMemoryEdgeRepository::new());
    credentials
        .create_mcp_credential(credential(&fixture))
        .await
        .expect("credential");
    let planned = planner(vec![input(&fixture)], targets.clone(), credentials)
        .plan(PlanMcpGatewayProjectionSet {
            scope: scope(&fixture, node_id),
            gateway_node_id: node_id,
            observed_at: now(),
        })
        .await
        .expect("projection set");

    assert_eq!(targets.calls.load(Ordering::SeqCst), 1);
    assert_eq!(planned.gateway_node_id(), node_id);
    assert_eq!(planned.route_versions().len(), 1);
    assert_eq!(planned.credential_authority_versions().len(), 1);
    assert!(planned.credential_authority_versions()[0].active_at_observed_at());
    assert_eq!(
        planned.route_versions()[0].route_id(),
        fixture.policy.spec().route_id
    );
    assert_eq!(planned.route_versions()[0].workload_aggregate_version(), 2);
    assert_eq!(
        planned.route_versions()[0].domain_claim_id(),
        fixture.policy.spec().domain_claim_id
    );
    assert_eq!(
        planned.route_versions()[0].domain_claim_aggregate_version(),
        2
    );
    assert_eq!(planned.ingress_routes().len(), 1);
    assert_eq!(
        planned.ingress_routes()[0].router(),
        format!(
            "mcp-route-{}",
            fixture.policy.spec().route_id.as_uuid().simple()
        )
    );
    assert_eq!(
        planned.ingress_routes()[0].hostname(),
        &fixture.policy.spec().hostname
    );
    let projection = planned.projection().expect("non-empty projection");
    assert_eq!(projection.projection().routes.len(), 1);
    assert_eq!(
        projection.projection().routes[0].targets[0].node_id,
        node_id.as_uuid()
    );
    let expires_at = projection.projection().expires_at;
    let compiled = snapshot_compiler()
        .compile_mcp_reconciliation(CompileMcpGatewaySnapshot {
            metadata: GatewaySnapshotMetadata::new(node_id, 1, None, now(), expires_at),
            physical_scope: GatewayScopeState::empty(node_id),
            certificate_id: Some(GatewayCertificateId::new()),
            active_routes: Vec::new(),
            mcp: PlannedMcpGatewayNodeProjection::single(planned)
                .expect("single-scope node projection"),
        })
        .expect("complete MCP snapshot");
    let snapshot = compiled.snapshot();
    a3s_acl::parse_acl(&snapshot.acl).expect("complete Gateway ACL");
    assert!(snapshot.acl.contains("mode { kind = \"cloud-managed\" }"));
    assert!(snapshot.acl.contains("routers \"mcp-route-"));
    assert!(snapshot
        .acl
        .contains("service = \"a3s-cloud-mcp-default-deny\""));
    assert!(snapshot.acl.contains("services \"mcp-target-"));
    assert!(snapshot.acl.contains("mcp {"));
    assert!(snapshot.acl.contains("management {"));
    assert_eq!(
        snapshot
            .certificate_request
            .as_ref()
            .expect("certificate")
            .dns_names,
        vec![fixture.policy.spec().hostname.as_str().to_owned()]
    );
    assert!(compiled.active_route_versions().is_empty());
    assert_eq!(compiled.domain_claim_versions().len(), 1);
    assert_eq!(
        compiled.domain_claim_versions()[0].domain_claim_id(),
        fixture.policy.spec().domain_claim_id
    );
    let stage = StageMcpGatewaySnapshot::new(
        compiled,
        NodeCommandId::new(),
        uuid::Uuid::now_v7(),
        now() + Duration::minutes(5),
    )
    .expect("durable stage intent");
    assert!(stage.certificate().is_some());
    assert_eq!(stage.event().event_key, "edge.mcp-gateway.snapshot-staged");
    stage.validate().expect("stage intent");
}

#[tokio::test]
async fn revoked_credential_keeps_cas_evidence_but_removes_the_gateway_route() {
    let fixture = fixture();
    let node_id = NodeId::new();
    let targets = Arc::new(CountingTargetReader {
        target: None,
        calls: AtomicUsize::new(0),
    });
    let credentials = Arc::new(InMemoryEdgeRepository::new());
    let mut revoked = credentials
        .create_mcp_credential(credential(&fixture))
        .await
        .expect("credential");
    revoked.revoke(now()).expect("revoke credential");
    credentials
        .update_mcp_credential(revoked.clone(), 1)
        .await
        .expect("persist revocation");

    let planned = planner(vec![input(&fixture)], targets.clone(), credentials)
        .plan(PlanMcpGatewayProjectionSet {
            scope: scope(&fixture, node_id),
            gateway_node_id: node_id,
            observed_at: now(),
        })
        .await
        .expect("revoked credential cleanup plan");

    assert_eq!(targets.calls.load(Ordering::SeqCst), 0);
    assert_eq!(planned.route_versions().len(), 1);
    assert!(planned.ingress_routes().is_empty());
    assert!(planned.projection().is_none());
    assert_eq!(
        planned.credential_authority_versions(),
        &[McpCredentialAuthorityVersion::new(
            revoked.id,
            revoked.generation(),
            revoked.aggregate_version(),
            false,
        )
        .expect("revoked authority version")]
    );

    let compiled = snapshot_compiler()
        .compile_mcp_reconciliation(CompileMcpGatewaySnapshot {
            metadata: GatewaySnapshotMetadata::new(
                node_id,
                1,
                None,
                now(),
                now() + Duration::minutes(10),
            ),
            physical_scope: GatewayScopeState::empty(node_id),
            certificate_id: None,
            active_routes: Vec::new(),
            mcp: PlannedMcpGatewayNodeProjection::single(planned)
                .expect("single-scope node projection"),
        })
        .expect("credential cleanup snapshot");
    assert_eq!(compiled.domain_claim_versions().len(), 1);
    assert!(compiled.certificate_domain_claim_ids().is_empty());
    assert!(!compiled.snapshot().acl.contains("mcp {"));
    let stage = StageMcpGatewaySnapshot::new(
        compiled,
        NodeCommandId::new(),
        uuid::Uuid::now_v7(),
        now() + Duration::minutes(5),
    )
    .expect("credential cleanup stage");
    assert!(stage.certificate().is_none());
    stage.validate().expect("credential cleanup stage");
}

#[tokio::test]
async fn revoked_credential_removes_only_its_route_from_the_complete_snapshot() {
    let fixture = fixture();
    let node_id = NodeId::new();
    let targets = Arc::new(CountingTargetReader {
        target: Some(target(&fixture, node_id, 49152)),
        calls: AtomicUsize::new(0),
    });
    let credentials = Arc::new(InMemoryEdgeRepository::new());
    let active = credentials
        .create_mcp_credential(credential(&fixture))
        .await
        .expect("active credential");

    let mut second_input = input(&fixture);
    let mut second_spec = second_input.policy.spec().clone();
    second_spec.route_id = RouteId::new();
    second_spec.domain_claim_id = DomainClaimId::new();
    second_spec.hostname = RouteHostname::parse("second-mcp.example.com").expect("second hostname");
    let second_credential_id = McpCredentialId::new();
    second_spec.grants[0].credential_id = second_credential_id.as_uuid();
    second_input.policy = McpRoutePolicy::create(
        second_spec.clone(),
        &second_input.profile_binding.profile,
        now(),
    )
    .expect("second policy");
    let mut second_claim = DomainClaim::create(
        second_spec.domain_claim_id,
        second_spec.organization_id,
        second_spec.project_id,
        second_spec.environment_id,
        DomainNamePattern::parse(second_spec.hostname.as_str()).expect("second pattern"),
        format!("a3s-cloud-verification={}", second_spec.domain_claim_id),
        now() - Duration::minutes(1),
    )
    .expect("second claim");
    second_claim
        .verify(now() - Duration::seconds(1))
        .expect("verify second claim");
    second_input.domain_claim = second_claim;

    let mut revoked = McpCredential::issue(
        second_credential_id,
        second_spec.organization_id,
        second_spec.project_id,
        second_spec.environment_id,
        "a3s_mcp_def67890abc12345",
        VERIFIER,
        now() + Duration::minutes(30),
        now(),
    )
    .expect("second credential");
    credentials
        .create_mcp_credential(revoked.clone())
        .await
        .expect("store second credential");
    revoked.revoke(now()).expect("revoke second credential");
    credentials
        .update_mcp_credential(revoked, 1)
        .await
        .expect("persist second revocation");

    let planned = planner(
        vec![input(&fixture), second_input],
        targets.clone(),
        credentials,
    )
    .plan(PlanMcpGatewayProjectionSet {
        scope: scope(&fixture, node_id),
        gateway_node_id: node_id,
        observed_at: now(),
    })
    .await
    .expect("partial credential cleanup plan");

    assert_eq!(targets.calls.load(Ordering::SeqCst), 1);
    assert_eq!(planned.route_versions().len(), 2);
    assert_eq!(planned.credential_authority_versions().len(), 2);
    assert_eq!(planned.ingress_routes().len(), 1);
    let projection = planned.projection().expect("remaining active route");
    assert_eq!(projection.projection().routes.len(), 1);
    assert_eq!(projection.projection().credentials.len(), 1);
    assert_eq!(
        projection.projection().credentials[0].credential_id,
        active.id.as_uuid()
    );
}

#[tokio::test]
async fn represents_an_empty_active_set_without_resolving_runtime() {
    let fixture = fixture();
    let node_id = NodeId::new();
    let targets = Arc::new(CountingTargetReader {
        target: None,
        calls: AtomicUsize::new(0),
    });
    let planned = planner(
        Vec::new(),
        targets.clone(),
        Arc::new(InMemoryEdgeRepository::new()),
    )
    .plan(PlanMcpGatewayProjectionSet {
        scope: scope(&fixture, node_id),
        gateway_node_id: node_id,
        observed_at: now(),
    })
    .await
    .expect("empty set");

    assert!(planned.projection().is_none());
    assert!(planned.route_versions().is_empty());
    assert!(planned.ingress_routes().is_empty());
    assert_eq!(targets.calls.load(Ordering::SeqCst), 0);
    let compiled = snapshot_compiler()
        .compile_mcp_reconciliation(CompileMcpGatewaySnapshot {
            metadata: GatewaySnapshotMetadata::new(
                node_id,
                1,
                None,
                now(),
                now() + Duration::minutes(10),
            ),
            physical_scope: GatewayScopeState::empty(node_id),
            certificate_id: None,
            active_routes: Vec::new(),
            mcp: PlannedMcpGatewayNodeProjection::single(planned)
                .expect("single-scope node projection"),
        })
        .expect("complete empty MCP snapshot");
    assert!(!compiled.snapshot().acl.contains("mcp {"));
    assert!(!compiled.snapshot().acl.contains("mcp-route-"));
    assert!(!compiled
        .snapshot()
        .acl
        .contains("entrypoints \"a3s-cloud-https\""));
    assert!(compiled.snapshot().acl.contains("management {"));
    let stage = StageMcpGatewaySnapshot::new(
        compiled,
        NodeCommandId::new(),
        uuid::Uuid::now_v7(),
        now() + Duration::minutes(5),
    )
    .expect("route-less durable stage intent");
    assert!(stage.certificate().is_none());
    stage.validate().expect("route-less stage intent");
}

#[tokio::test]
async fn composes_ordinary_and_mcp_routes_with_all_cas_evidence() {
    let fixture = fixture();
    let node_id = NodeId::new();
    let targets = Arc::new(CountingTargetReader {
        target: Some(target(&fixture, node_id, 49152)),
        calls: AtomicUsize::new(0),
    });
    let credentials = Arc::new(InMemoryEdgeRepository::new());
    credentials
        .create_mcp_credential(credential(&fixture))
        .await
        .expect("credential");
    let planned = planner(vec![input(&fixture)], targets, credentials)
        .plan(PlanMcpGatewayProjectionSet {
            scope: scope(&fixture, node_id),
            gateway_node_id: node_id,
            observed_at: now(),
        })
        .await
        .expect("MCP projection");
    let ordinary_hostname = RouteHostname::parse("app.example.com").expect("hostname");
    let ordinary_claim_id = DomainClaimId::new();
    let mut ordinary_claim = DomainClaim::create(
        ordinary_claim_id,
        fixture.policy.spec().organization_id,
        fixture.policy.spec().project_id,
        fixture.policy.spec().environment_id,
        DomainNamePattern::parse(ordinary_hostname.as_str()).expect("domain pattern"),
        format!("a3s-cloud-verification={ordinary_claim_id}"),
        now() - Duration::minutes(1),
    )
    .expect("domain claim");
    ordinary_claim
        .verify(now() - Duration::seconds(1))
        .expect("verified claim");
    let ordinary_id = RouteId::new();
    let mut ordinary = Route::create(
        ordinary_id,
        fixture.policy.spec().organization_id,
        fixture.policy.spec().project_id,
        fixture.policy.spec().environment_id,
        fixture.policy.spec().gateway_scope_id,
        node_id,
        ordinary_hostname,
        RoutePath::parse("/app").expect("prefix"),
        ordinary_claim.id,
        ordinary_claim.pattern.clone(),
        GatewayCertificateId::new(),
        fixture.policy.spec().workload_id,
        target(&fixture, node_id, 49153).target,
        now(),
    )
    .expect("ordinary route");
    ordinary.state = RouteState::Active;
    let expires_at = planned
        .projection()
        .expect("projection")
        .projection()
        .expires_at;

    let compiled = snapshot_compiler()
        .compile_mcp_reconciliation(CompileMcpGatewaySnapshot {
            metadata: GatewaySnapshotMetadata::new(node_id, 1, None, now(), expires_at),
            physical_scope: GatewayScopeState::empty(node_id),
            certificate_id: Some(GatewayCertificateId::new()),
            active_routes: vec![GatewaySnapshotRouteInput {
                route: ordinary,
                domain_claim: ordinary_claim,
            }],
            mcp: PlannedMcpGatewayNodeProjection::single(planned)
                .expect("single-scope node projection"),
        })
        .expect("mixed complete snapshot");

    assert!(compiled.snapshot().acl.contains(&format!(
        "routers \"route-{}\"",
        ordinary_id.as_uuid().simple()
    )));
    assert!(compiled.snapshot().acl.contains("routers \"mcp-route-"));
    assert!(compiled.snapshot().acl.contains("mcp {"));
    assert_eq!(compiled.active_route_versions().len(), 1);
    assert_eq!(compiled.active_route_versions()[0].route_id, ordinary_id);
    assert_eq!(compiled.domain_claim_versions().len(), 2);
    assert_eq!(
        compiled
            .snapshot()
            .certificate_request
            .as_ref()
            .expect("certificate")
            .dns_names,
        vec![
            "app.example.com".to_owned(),
            fixture.policy.spec().hostname.as_str().to_owned()
        ]
    );
}

#[tokio::test]
async fn ordinary_publication_composes_the_current_mcp_projection_in_the_same_snapshot() {
    let fixture = fixture();
    let node_id = NodeId::new();
    let targets = Arc::new(CountingTargetReader {
        target: Some(target(&fixture, node_id, 49152)),
        calls: AtomicUsize::new(0),
    });
    let credentials = Arc::new(InMemoryEdgeRepository::new());
    credentials
        .create_mcp_credential(credential(&fixture))
        .await
        .expect("credential");
    let planned = planner(vec![input(&fixture)], targets, credentials)
        .plan(PlanMcpGatewayProjectionSet {
            scope: scope(&fixture, node_id),
            gateway_node_id: node_id,
            observed_at: now(),
        })
        .await
        .expect("MCP projection");
    let expires_at = planned
        .projection()
        .expect("MCP projection content")
        .projection()
        .expires_at;
    let ordinary_hostname = RouteHostname::parse("ordinary.example.com").expect("hostname");
    let ordinary_claim_id = DomainClaimId::new();
    let mut ordinary_claim = DomainClaim::create(
        ordinary_claim_id,
        fixture.policy.spec().organization_id,
        fixture.policy.spec().project_id,
        fixture.policy.spec().environment_id,
        DomainNamePattern::parse(ordinary_hostname.as_str()).expect("domain pattern"),
        format!("a3s-cloud-verification={ordinary_claim_id}"),
        now() - Duration::minutes(1),
    )
    .expect("DomainClaim");
    ordinary_claim
        .verify(now() - Duration::seconds(1))
        .expect("verified DomainClaim");
    let ordinary_route_id = RouteId::new();
    let certificate_id = GatewayCertificateId::new();
    let ordinary_route = Route::create(
        ordinary_route_id,
        fixture.policy.spec().organization_id,
        fixture.policy.spec().project_id,
        fixture.policy.spec().environment_id,
        fixture.policy.spec().gateway_scope_id,
        node_id,
        ordinary_hostname,
        RoutePath::parse("/ordinary").expect("path"),
        ordinary_claim.id,
        ordinary_claim.pattern.clone(),
        certificate_id,
        fixture.policy.spec().workload_id,
        target(&fixture, node_id, 49153).target,
        now(),
    )
    .expect("pending ordinary Route");
    let desired_state = PlannedGatewayNodeDesiredState::new(
        GatewayScopeState::empty(node_id),
        Vec::new(),
        PlannedMcpGatewayNodeProjection::single(planned).expect("single-scope node projection"),
    )
    .expect("complete node desired state");
    let candidate = snapshot_compiler()
        .compile_managed_route_snapshot(CompileManagedGatewayRouteSnapshot {
            metadata: GatewaySnapshotMetadata::new(node_id, 1, None, now(), expires_at),
            desired_state,
            certificate_id,
            snapshot_routes: vec![ordinary_route],
            additional_domain_claims: vec![ordinary_claim],
        })
        .expect("ordinary-plus-MCP managed snapshot");

    assert_eq!(candidate.ordinary_route_ids(), &[ordinary_route_id]);
    assert!(candidate.snapshot().acl.contains(&format!(
        "routers \"route-{}\"",
        ordinary_route_id.as_uuid().simple()
    )));
    assert!(candidate.snapshot().acl.contains("routers \"mcp-route-"));
    assert!(candidate.snapshot().acl.contains("mcp {"));
    assert_eq!(candidate.certificate_domain_claim_ids().len(), 2);
    let publication = GatewayPublication::stage(
        node_id,
        NodeCommandId::new(),
        uuid::Uuid::now_v7(),
        candidate.snapshot().clone(),
        now(),
        now() + Duration::minutes(5),
    )
    .expect("ordinary publication");
    let composition = GatewayManagedSnapshotComposition::new(
        candidate,
        &publication,
        GatewaySnapshotPublicationOwner::Ordinary,
    )
    .expect("ordinary-owned composition");
    composition
        .validate_for(&publication)
        .expect("complete ordinary-owned composition");
}

#[tokio::test]
async fn rejects_duplicate_inputs_before_resolving_runtime() {
    let fixture = fixture();
    let node_id = NodeId::new();
    let targets = Arc::new(CountingTargetReader {
        target: None,
        calls: AtomicUsize::new(0),
    });
    let duplicate = input(&fixture);
    let result = planner(
        vec![duplicate.clone(), duplicate],
        targets.clone(),
        Arc::new(InMemoryEdgeRepository::new()),
    )
    .plan(PlanMcpGatewayProjectionSet {
        scope: scope(&fixture, node_id),
        gateway_node_id: node_id,
        observed_at: now(),
    })
    .await;

    assert!(matches!(result, Err(RepositoryError::Storage(_))));
    assert_eq!(targets.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn rejects_duplicate_ingress_ownership_before_resolving_runtime() {
    let fixture = fixture();
    let node_id = NodeId::new();
    let targets = Arc::new(CountingTargetReader {
        target: None,
        calls: AtomicUsize::new(0),
    });
    let first = input(&fixture);
    let mut second = first.clone();
    let mut second_spec = second.policy.spec().clone();
    second_spec.route_id = RouteId::new();
    second.policy = McpRoutePolicy::create(second_spec, &second.profile_binding.profile, now())
        .expect("second policy");
    let result = planner(
        vec![first, second],
        targets.clone(),
        Arc::new(InMemoryEdgeRepository::new()),
    )
    .plan(PlanMcpGatewayProjectionSet {
        scope: scope(&fixture, node_id),
        gateway_node_id: node_id,
        observed_at: now(),
    })
    .await;

    assert!(matches!(result, Err(RepositoryError::Storage(_))));
    assert_eq!(targets.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn complete_snapshot_rejects_an_ordinary_prefix_overlapping_mcp_ingress() {
    let fixture = fixture();
    let node_id = NodeId::new();
    let targets = Arc::new(CountingTargetReader {
        target: Some(target(&fixture, node_id, 49152)),
        calls: AtomicUsize::new(0),
    });
    let credentials = Arc::new(InMemoryEdgeRepository::new());
    credentials
        .create_mcp_credential(credential(&fixture))
        .await
        .expect("credential");
    let resolved = input(&fixture);
    let domain_claim = resolved.domain_claim.clone();
    let planned = planner(vec![resolved], targets, credentials)
        .plan(PlanMcpGatewayProjectionSet {
            scope: scope(&fixture, node_id),
            gateway_node_id: node_id,
            observed_at: now(),
        })
        .await
        .expect("MCP projection");
    let mut ordinary = Route::create(
        RouteId::new(),
        fixture.policy.spec().organization_id,
        fixture.policy.spec().project_id,
        fixture.policy.spec().environment_id,
        fixture.policy.spec().gateway_scope_id,
        node_id,
        fixture.policy.spec().hostname.clone(),
        RoutePath::parse("/").expect("prefix"),
        domain_claim.id,
        domain_claim.pattern.clone(),
        GatewayCertificateId::new(),
        fixture.policy.spec().workload_id,
        target(&fixture, node_id, 49153).target,
        now(),
    )
    .expect("ordinary route");
    ordinary.state = RouteState::Active;
    let expires_at = planned
        .projection()
        .expect("projection")
        .projection()
        .expires_at;

    assert!(snapshot_compiler()
        .compile_mcp_reconciliation(CompileMcpGatewaySnapshot {
            metadata: GatewaySnapshotMetadata::new(node_id, 1, None, now(), expires_at,),
            physical_scope: GatewayScopeState::empty(node_id),
            certificate_id: Some(GatewayCertificateId::new()),
            active_routes: vec![GatewaySnapshotRouteInput {
                route: ordinary,
                domain_claim,
            }],
            mcp: PlannedMcpGatewayNodeProjection::single(planned)
                .expect("single-scope node projection"),
        })
        .expect_err("overlapping ingress")
        .contains("PathPrefix"));
}
