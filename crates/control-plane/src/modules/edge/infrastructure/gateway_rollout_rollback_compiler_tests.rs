use super::{
    CompileGatewayRolloutRollback, CompileManagedGatewayRolloutRollback,
    GatewayRollbackMemberSnapshotContext, GatewayRolloutRollbackCompiler, GatewaySnapshotCompiler,
    GatewaySnapshotCompilerConfig, GatewaySnapshotPublicationOwner,
    ManagedGatewayRollbackMemberSnapshotContext, McpGatewayNodeProjectionAssembler,
    McpGatewaySnapshotAnchor, PlannedGatewayNodeDesiredState,
};
use crate::modules::edge::domain::{
    DomainClaim, DomainNamePattern, GatewayCertificate, GatewayCertificateMaterial, GatewayRollout,
    GatewayRolloutPolicy, GatewayRolloutRollback, GatewayScope, GatewayScopeState, Route,
    RouteHostname, RoutePath, RoutePortName, RouteTarget, UpstreamEndpoint,
};
use crate::modules::edge::infrastructure::GatewaySnapshotRouteInput;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayRolloutId, GatewayScopeId,
    NodeCommandId, NodeId, OrganizationId, ProjectId, RouteId, WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{
    GatewayAckState, GatewayCertificateRequest, GatewayManagementProtocol, GatewaySnapshot,
    NodeGatewayAck,
};
use chrono::{Duration, Utc};
use uuid::Uuid;

#[test]
fn compiles_exact_member_rollback_from_observed_physical_revisions_and_reuses_valid_certificates() {
    let now = Utc::now();
    let members = [NodeId::new(), NodeId::new()];
    let scope = replicated_scope(members, now - Duration::minutes(30));
    let failed = failed_rollout(&scope, now - Duration::minutes(2));
    let rollback = GatewayRolloutRollback::required(&failed).expect("required rollback");
    let mut contexts = Vec::new();
    for (index, node_id) in members.into_iter().enumerate() {
        let (route, certificate) = active_route_with_ready_certificate(
            &scope,
            node_id,
            &format!("retained-{index}.example.com"),
            now - Duration::minutes(20),
        );
        contexts.push(GatewayRollbackMemberSnapshotContext {
            scope: GatewayScopeState {
                node_id,
                last_issued_revision: 2,
                installed_revision: Some(if index == 0 { 2 } else { 1 }),
                aggregate_version: 5,
            },
            active_routes: vec![route],
            reusable_certificate: Some(certificate),
        });
    }

    let compiled = rollback_compiler()
        .compile(CompileGatewayRolloutRollback {
            scope: scope.clone(),
            failed_rollout: failed,
            rollback: rollback.clone(),
            member_contexts: contexts,
            issued_at: now,
        })
        .expect("compile exact rollback");

    assert_eq!(compiled.rollout.id, rollback.rollback_rollout_id);
    assert_eq!(compiled.rollout.generation, rollback.rollback_generation);
    assert_eq!(compiled.rollout.policy.min_ready, 2);
    assert_eq!(compiled.rollout.policy.max_unavailable, 0);
    assert_eq!(compiled.publications.len(), 2);
    assert!(compiled.certificates.is_empty());
    assert_eq!(compiled.reused_certificates.len(), 2);
    assert_eq!(compiled.rollback.state.as_str(), "staged");
    for (index, publication) in compiled.publications.iter().enumerate() {
        assert_eq!(publication.revision, 3);
        assert_eq!(
            publication.expected_revision,
            Some(if index == 0 { 2 } else { 1 })
        );
        assert_eq!(
            publication.command_correlation_id,
            rollback.rollback_rollout_id.as_uuid()
        );
        assert!(publication
            .acl
            .contains(&format!("retained-{index}.example.com")));
        assert!(!publication.acl.contains("failed.example.com"));
        assert_eq!(
            publication
                .certificate_request
                .as_ref()
                .expect("reused certificate request")
                .certificate_id,
            compiled.reused_certificates[index].id.as_uuid()
        );
    }
    assert!(compiled
        .expected_scope_versions
        .values()
        .all(|version| *version == 5));
}

#[test]
fn managed_rollback_carries_one_complete_composition_for_every_member() {
    let now = Utc::now();
    let members = [NodeId::new(), NodeId::new()];
    let scope = replicated_scope(members, now - Duration::minutes(30));
    let failed = failed_rollout(&scope, now - Duration::minutes(2));
    let rollback = GatewayRolloutRollback::required(&failed).expect("required rollback");
    let mut contexts = Vec::new();
    for (index, node_id) in members.into_iter().enumerate() {
        let (route, certificate) = active_route_with_ready_certificate(
            &scope,
            node_id,
            &format!("managed-retained-{index}.example.com"),
            now - Duration::minutes(20),
        );
        let claim_id = route.domain_claim_id.expect("route claim");
        let mut claim = DomainClaim::create(
            claim_id,
            route.organization_id,
            route.project_id,
            route.environment_id,
            route.domain_pattern.clone().expect("route domain pattern"),
            format!("a3s-cloud-verification={claim_id}"),
            now - Duration::minutes(30),
        )
        .expect("claim");
        claim
            .verify(now - Duration::minutes(25))
            .expect("verified claim");
        let mcp = McpGatewayNodeProjectionAssembler::default()
            .assemble(
                McpGatewaySnapshotAnchor::from_scope(&scope),
                node_id,
                now,
                Vec::new(),
            )
            .expect("empty MCP desired state");
        let desired_state = PlannedGatewayNodeDesiredState::new(
            GatewayScopeState {
                node_id,
                last_issued_revision: 2,
                installed_revision: Some(if index == 0 { 2 } else { 1 }),
                aggregate_version: 5,
            },
            vec![GatewaySnapshotRouteInput {
                route,
                domain_claim: claim,
            }],
            mcp,
        )
        .expect("managed member state");
        contexts.push(ManagedGatewayRollbackMemberSnapshotContext {
            desired_state,
            reusable_certificate: Some(certificate),
        });
    }

    let compiled = rollback_compiler()
        .compile_managed(CompileManagedGatewayRolloutRollback {
            scope,
            failed_rollout: failed,
            rollback,
            member_contexts: contexts,
            issued_at: now,
        })
        .expect("managed rollback");
    assert_eq!(compiled.publications.len(), 2);
    assert_eq!(compiled.managed_compositions.len(), 2);
    assert!(compiled.certificates.is_empty());
    assert_eq!(compiled.reused_certificates.len(), 2);
    for publication in &compiled.publications {
        let composition = compiled
            .managed_compositions
            .get(&publication.node_id)
            .expect("member composition");
        assert_eq!(
            composition.owner(),
            GatewaySnapshotPublicationOwner::Ordinary
        );
        assert_eq!(composition.candidate().ordinary_route_ids().len(), 1);
        composition
            .validate_for(publication)
            .expect("exact composition");
    }
    compiled
        .managed_stage_bundle()
        .expect("managed rollback stage bundle");
}

fn rollback_compiler() -> GatewayRolloutRollbackCompiler {
    GatewayRolloutRollbackCompiler::new(
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
    .expect("rollback compiler")
}

fn replicated_scope(members: [NodeId; 2], created_at: chrono::DateTime<Utc>) -> GatewayScope {
    GatewayScope::create_replicated(
        GatewayScopeId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        members[0],
        members.to_vec(),
        GatewayRolloutPolicy::new(2, 0, members.len()).expect("rollout policy"),
        created_at,
    )
    .expect("Gateway scope")
}

fn failed_rollout(scope: &GatewayScope, started_at: chrono::DateTime<Utc>) -> GatewayRollout {
    let correlation_id = Uuid::now_v7();
    let publications = scope
        .member_node_ids
        .iter()
        .map(|node_id| {
            let snapshot = GatewaySnapshot::new(
                node_id.as_uuid(),
                2,
                Some(1),
                started_at,
                started_at + Duration::hours(1),
                format!("# failed snapshot for {node_id}"),
            )
            .expect("failed snapshot");
            crate::modules::edge::domain::GatewayPublication::stage(
                *node_id,
                NodeCommandId::new(),
                correlation_id,
                snapshot,
                started_at,
                started_at + Duration::minutes(1),
            )
            .expect("failed publication")
        })
        .collect::<Vec<_>>();
    let mut rollout =
        GatewayRollout::stage(GatewayRolloutId::new(), scope, 1, &publications, started_at)
            .expect("failed rollout");
    rollout
        .acknowledge(&acknowledgement(
            &publications[0],
            GatewayAckState::Applied,
            started_at + Duration::seconds(1),
        ))
        .expect("applied failed member");
    rollout
        .acknowledge(&acknowledgement(
            &publications[1],
            GatewayAckState::Rejected,
            started_at + Duration::seconds(2),
        ))
        .expect("rejected failed member");
    rollout
}

fn active_route_with_ready_certificate(
    scope: &GatewayScope,
    node_id: NodeId,
    hostname: &str,
    created_at: chrono::DateTime<Utc>,
) -> (Route, GatewayCertificate) {
    let certificate_id = GatewayCertificateId::new();
    let workload_id = WorkloadId::new();
    let revision_id = WorkloadRevisionId::new();
    let mut route = Route::create(
        RouteId::new(),
        scope.organization_id,
        scope.project_id,
        scope.environment_id,
        scope.id,
        node_id,
        RouteHostname::parse(hostname).expect("hostname"),
        RoutePath::parse("/").expect("path"),
        DomainClaimId::new(),
        DomainNamePattern::parse(hostname).expect("domain pattern"),
        certificate_id,
        workload_id,
        RouteTarget::new(
            workload_id,
            revision_id,
            format!("workload:{workload_id}:revision:{revision_id}"),
            1,
            RoutePortName::parse("http").expect("port"),
            UpstreamEndpoint::parse("http://127.0.0.1:49152").expect("upstream"),
            created_at,
        )
        .expect("target"),
        created_at,
    )
    .expect("route");
    let command_id = NodeCommandId::new();
    let digest = format!("sha256:{}", "a".repeat(64));
    let staged_at = created_at + Duration::seconds(1);
    route
        .stage(1, command_id, digest.clone(), staged_at)
        .expect("stage retained route");
    let acknowledged_at = staged_at + Duration::seconds(2);
    let route_ack = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: command_id.as_uuid(),
        node_id: node_id.as_uuid(),
        gateway_id: node_id.as_uuid(),
        revision: 1,
        snapshot_digest: digest.clone(),
        expires_at: acknowledged_at + Duration::days(1),
        state: GatewayAckState::Applied,
        ready: true,
        message: None,
        acknowledged_at,
        management_protocol: Some(GatewayManagementProtocol::advertised_v1()),
    };
    route
        .apply_gateway_acknowledgement(&route_ack)
        .expect("activate retained route");

    let request = GatewayCertificateRequest::new(
        certificate_id.as_uuid(),
        vec![hostname.into()],
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/certificate.pem"),
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/private-key.pem"),
    )
    .expect("certificate request");
    let mut certificate = GatewayCertificate::provision(
        certificate_id,
        scope.organization_id,
        node_id,
        vec![route.domain_claim_id.expect("domain claim")],
        1,
        command_id,
        digest,
        request,
        staged_at,
    )
    .expect("certificate");
    certificate
        .record_issued(
            format!("sha256:{}", "b".repeat(64)),
            GatewayCertificateMaterial {
                serial_number: certificate_id.to_string(),
                fingerprint: format!("sha256:{}", "c".repeat(64)),
                certificate_pem:
                    "-----BEGIN CERTIFICATE-----\ndGVzdA==\n-----END CERTIFICATE-----\n".into(),
                ca_bundle_pem:
                    "-----BEGIN CERTIFICATE-----\ndGVzdC1jYQ==\n-----END CERTIFICATE-----\n".into(),
                issued_at: staged_at,
                expires_at: staged_at + Duration::days(30),
            },
            staged_at,
        )
        .expect("issue certificate");
    certificate
        .apply_gateway_acknowledgement(&route_ack)
        .expect("ready certificate");
    (route, certificate)
}

fn acknowledgement(
    publication: &crate::modules::edge::domain::GatewayPublication,
    state: GatewayAckState,
    acknowledged_at: chrono::DateTime<Utc>,
) -> NodeGatewayAck {
    NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: publication.command_id.as_uuid(),
        node_id: publication.node_id.as_uuid(),
        gateway_id: publication.node_id.as_uuid(),
        revision: publication.revision,
        snapshot_digest: publication.snapshot_digest.clone(),
        expires_at: publication.snapshot_expires_at,
        state,
        ready: state == GatewayAckState::Applied,
        message: (state == GatewayAckState::Rejected).then(|| "snapshot rejected".into()),
        acknowledged_at,
        management_protocol: Some(GatewayManagementProtocol::advertised_v1()),
    }
}
