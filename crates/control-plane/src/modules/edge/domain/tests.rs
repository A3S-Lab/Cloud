use super::*;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayRolloutId,
    GatewayScopeId, NodeCommandId, NodeId, OrganizationId, ProjectId, RouteId, WorkloadId,
    WorkloadRevisionId,
};
use a3s_cloud_contracts::{
    GatewayAckState, GatewayCertificateRequest, GatewaySnapshot, NodeGatewayAck,
};
use chrono::{Duration, TimeZone, Utc};
use uuid::Uuid;

fn route(now: chrono::DateTime<Utc>) -> Route {
    let workload_id = WorkloadId::new();
    let workload_revision_id = WorkloadRevisionId::new();
    Route::create(
        RouteId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        GatewayScopeId::new(),
        NodeId::new(),
        RouteHostname::parse("API.Example.COM").expect("hostname"),
        RoutePath::parse("/v1").expect("path"),
        DomainClaimId::new(),
        DomainNamePattern::parse("api.example.com").expect("domain pattern"),
        GatewayCertificateId::new(),
        workload_id,
        RouteTarget::new(
            workload_id,
            workload_revision_id,
            format!("workload:{workload_id}:revision:{workload_revision_id}"),
            1,
            RoutePortName::parse("http").expect("port"),
            UpstreamEndpoint::parse("http://127.0.0.1:49152").expect("upstream"),
            now,
        )
        .expect("target"),
        now,
    )
    .expect("route")
}

#[test]
fn logical_gateway_scope_owns_one_environment_node_binding() {
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
        Utc::now(),
    )
    .expect("Gateway scope");

    assert!(scope.owns(organization_id, project_id, environment_id, node_id));
    assert!(!scope.owns(organization_id, project_id, EnvironmentId::new(), node_id,));
    assert!(!scope.owns(organization_id, project_id, environment_id, NodeId::new(),));
}

#[test]
fn logical_gateway_scope_owns_bounded_replica_membership_and_rollout_policy() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let primary = NodeId::new();
    let secondary = NodeId::new();
    let policy = GatewayRolloutPolicy::new(1, 1, 2).expect("replicated policy");
    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        primary,
        vec![secondary, primary],
        policy,
        Utc::now(),
    )
    .expect("replicated Gateway scope");

    assert_eq!(scope.node_id, primary);
    assert_eq!(scope.member_node_ids, vec![primary, secondary]);
    assert_eq!(scope.membership_generation, 1);
    assert_eq!(
        scope
            .rollout_policy
            .required_ready(2)
            .expect("valid scope rollout policy"),
        1
    );
    assert!(scope.owns(organization_id, project_id, environment_id, primary));
    assert!(!scope.owns(organization_id, project_id, environment_id, secondary));
    assert!(scope.contains_member(primary));
    assert!(scope.contains_member(secondary));
    assert!(GatewayScope::create_replicated(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        primary,
        vec![primary, primary],
        policy,
        Utc::now(),
    )
    .is_err());
    assert!(GatewayRolloutPolicy::new(0, 0, 2).is_err());
    assert!(GatewayRolloutPolicy::new(1, 2, 2).is_err());
}

#[test]
fn replicated_gateway_rollout_requires_exact_per_member_terminal_evidence() {
    let now = Utc::now();
    let correlation_id = Uuid::now_v7();
    let primary = NodeId::new();
    let secondary = NodeId::new();
    let tertiary = NodeId::new();
    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        primary,
        vec![primary, secondary, tertiary],
        GatewayRolloutPolicy::new(2, 1, 3).expect("rollout policy"),
        now,
    )
    .expect("replicated scope");
    let publications = scope
        .member_node_ids
        .iter()
        .map(|node_id| rollout_publication(*node_id, correlation_id, now))
        .collect::<Vec<_>>();
    let mut rollout = GatewayRollout::stage(GatewayRolloutId::new(), &scope, 1, &publications, now)
        .expect("stage rollout");

    assert_eq!(rollout.required_ready().expect("valid rollout policy"), 2);
    assert!(!rollout
        .serves_traffic()
        .expect("valid pending rollout policy"));
    rollout
        .acknowledge(&rollout_acknowledgement(
            &publications[0],
            GatewayAckState::Applied,
            now + Duration::seconds(1),
        ))
        .expect("first replica");
    assert_eq!(rollout.state, GatewayRolloutState::Pending);
    rollout
        .acknowledge(&rollout_acknowledgement(
            &publications[1],
            GatewayAckState::Applied,
            now + Duration::seconds(2),
        ))
        .expect("second replica");
    assert_eq!(rollout.state, GatewayRolloutState::Ready);
    assert!(rollout
        .serves_traffic()
        .expect("valid ready rollout policy"));
    rollout
        .mark_unavailable(
            publications[2].node_id,
            "Gateway did not become ready before the rollout deadline",
            now + Duration::seconds(3),
        )
        .expect("terminal unavailable result");
    assert_eq!(rollout.state, GatewayRolloutState::Degraded);
    assert_eq!(rollout.ready_replicas, 2);
    assert_eq!(rollout.unavailable_replicas, 1);
    assert!(rollout.completed_at.is_some());
    assert!(rollout
        .serves_traffic()
        .expect("valid degraded rollout policy"));

    let replay = rollout
        .acknowledge(&rollout_acknowledgement(
            &publications[0],
            GatewayAckState::Applied,
            now + Duration::seconds(1),
        ))
        .expect("exact acknowledgement replay");
    assert!(!replay);
    let mut mismatched = rollout_acknowledgement(
        &publications[1],
        GatewayAckState::Applied,
        now + Duration::seconds(2),
    );
    mismatched.snapshot_digest = format!("sha256:{}", "0".repeat(64));
    let terminal = rollout.clone();
    assert!(rollout.acknowledge(&mismatched).is_err());
    assert_eq!(rollout, terminal);

    let mut exhausted =
        GatewayRollout::stage(GatewayRolloutId::new(), &scope, 2, &publications, now)
            .expect("stage exhausted rollout");
    exhausted.aggregate_version = u64::MAX;
    let unchanged = exhausted.clone();
    assert!(exhausted
        .acknowledge(&rollout_acknowledgement(
            &publications[0],
            GatewayAckState::Applied,
            now + Duration::seconds(1),
        ))
        .is_err());
    assert_eq!(exhausted, unchanged);
}

#[test]
fn gateway_publication_canonicalizes_snapshot_validity_at_database_precision() {
    let node_id = NodeId::new();
    let issued_at = Utc
        .timestamp_opt(1_700_000_000, 123_456_789)
        .single()
        .expect("issue time");
    let expires_at = issued_at + Duration::hours(1);
    let snapshot = GatewaySnapshot::new(
        node_id.as_uuid(),
        1,
        None,
        issued_at,
        expires_at,
        "# exact timestamp snapshot",
    )
    .expect("Gateway snapshot");

    let publication = GatewayPublication::stage(
        node_id,
        NodeCommandId::new(),
        Uuid::now_v7(),
        snapshot,
        issued_at,
        issued_at + Duration::minutes(3),
    )
    .expect("Gateway publication");

    assert_eq!(
        publication.command_issued_at,
        canonical_timestamp(issued_at)
    );
    assert_eq!(
        publication.snapshot_expires_at,
        canonical_timestamp(expires_at)
    );
    let recovered = publication.snapshot().expect("recovered snapshot");
    assert_eq!(recovered.issued_at, canonical_timestamp(issued_at));
    assert_eq!(recovered.expires_at, canonical_timestamp(expires_at));
}

fn rollout_publication(
    node_id: NodeId,
    correlation_id: Uuid,
    now: chrono::DateTime<Utc>,
) -> GatewayPublication {
    let snapshot = GatewaySnapshot::new(
        node_id.as_uuid(),
        1,
        None,
        now,
        now + Duration::hours(1),
        format!("# exact snapshot for {node_id}"),
    )
    .expect("Gateway snapshot");
    GatewayPublication::stage(
        node_id,
        NodeCommandId::new(),
        correlation_id,
        snapshot,
        now,
        now + Duration::minutes(3),
    )
    .expect("Gateway publication")
}

fn rollout_acknowledgement(
    publication: &GatewayPublication,
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
        management_protocol: Some(a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1()),
    }
}

#[test]
fn normalizes_route_ownership_and_rejects_ambiguous_values() {
    assert_eq!(
        RouteHostname::parse(" API.Example.COM ")
            .expect("hostname")
            .as_str(),
        "api.example.com"
    );
    assert!(RouteHostname::parse("127.0.0.1").is_err());
    assert!(RouteHostname::parse("api..example.com").is_err());
    assert!(RoutePath::parse("v1").is_err());
    assert!(RoutePath::parse("/v1//chat").is_err());
    assert!(RoutePath::parse("/v1/../admin").is_err());
    assert!(RoutePath::parse("/v1%2").is_err());
    assert!(UpstreamEndpoint::parse("http://10.0.0.8:8080").is_err());
    assert!(UpstreamEndpoint::parse("https://127.0.0.1:8080").is_err());
}

#[test]
fn exact_and_single_label_wildcard_domain_policy_is_closed() {
    let exact = DomainNamePattern::parse("API.Example.COM").expect("exact pattern");
    let wildcard = DomainNamePattern::parse("*.example.com").expect("wildcard pattern");
    let nested = DomainNamePattern::parse("*.api.example.com").expect("nested wildcard pattern");
    assert_eq!(exact.as_str(), "api.example.com");
    assert!(exact.covers(&RouteHostname::parse("api.example.com").expect("hostname")));
    assert!(!exact.covers(&RouteHostname::parse("www.example.com").expect("hostname")));
    assert!(wildcard.covers(&RouteHostname::parse("api.example.com").expect("hostname")));
    assert!(!wildcard.covers(&RouteHostname::parse("example.com").expect("hostname")));
    assert!(!wildcard.covers(&RouteHostname::parse("deep.api.example.com").expect("hostname")));
    assert!(wildcard.conflicts_with(&exact));
    assert!(!wildcard.conflicts_with(&nested));
    assert_eq!(
        wildcard.challenge_dns_name(),
        "_a3s-cloud-challenge.example.com"
    );
    assert!(DomainNamePattern::parse("*.*.example.com").is_err());
    assert!(DomainNamePattern::parse("*.localhost").is_err());
}

#[test]
fn domain_claim_must_be_verified_and_revocation_is_terminal() {
    let now = Utc::now();
    let mut claim = DomainClaim::create(
        DomainClaimId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        DomainNamePattern::parse("*.example.com").expect("pattern"),
        format!("a3s-cloud-verification={}", Uuid::now_v7()),
        now,
    )
    .expect("domain claim");
    let hostname = RouteHostname::parse("api.example.com").expect("hostname");
    assert!(!claim.covers(&hostname));
    claim
        .verify(now + Duration::seconds(1))
        .expect("verify claim");
    assert!(claim.covers(&hostname));
    claim
        .revoke("ownership removed", now + Duration::seconds(2))
        .expect("revoke claim");
    assert!(!claim.covers(&hostname));
    assert!(claim.verify(now + Duration::seconds(3)).is_err());
}

#[test]
fn gateway_certificate_becomes_ready_only_after_issuance_and_exact_reload_ack() {
    let now = Utc::now();
    let certificate_id = GatewayCertificateId::new();
    let node_id = NodeId::new();
    let command_id = NodeCommandId::new();
    let request = GatewayCertificateRequest::new(
        certificate_id.as_uuid(),
        vec!["*.example.com".into()],
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/certificate.pem"),
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/private-key.pem"),
    )
    .expect("certificate request");
    let snapshot = GatewaySnapshot::new_with_certificate(
        node_id.as_uuid(),
        3,
        Some(2),
        now,
        now + Duration::minutes(10),
        format!(
            "entrypoints \"https\" {{ tls {{ cert_file = \"{}\"; key_file = \"{}\" }} }}\n",
            request.certificate_file, request.private_key_file
        ),
        Some(request.clone()),
    )
    .expect("snapshot");
    let mut certificate = GatewayCertificate::provision(
        certificate_id,
        OrganizationId::new(),
        node_id,
        vec![DomainClaimId::new()],
        snapshot.revision,
        command_id,
        snapshot.snapshot_digest.clone(),
        request,
        now,
    )
    .expect("provision certificate");
    let applied = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: command_id.as_uuid(),
        node_id: node_id.as_uuid(),
        gateway_id: node_id.as_uuid(),
        revision: snapshot.revision,
        snapshot_digest: snapshot.snapshot_digest,
        expires_at: snapshot.expires_at,
        state: GatewayAckState::Applied,
        ready: true,
        message: None,
        acknowledged_at: now + Duration::seconds(2),
        management_protocol: Some(a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1()),
    };
    assert!(certificate.apply_gateway_acknowledgement(&applied).is_err());
    certificate
        .record_issued(
            format!("sha256:{}", "b".repeat(64)),
            GatewayCertificateMaterial {
                serial_number: certificate_id.to_string(),
                fingerprint: format!("sha256:{}", "a".repeat(64)),
                certificate_pem:
                    "-----BEGIN CERTIFICATE-----\ndGVzdA==\n-----END CERTIFICATE-----\n".into(),
                ca_bundle_pem:
                    "-----BEGIN CERTIFICATE-----\ndGVzdC1jYQ==\n-----END CERTIFICATE-----\n".into(),
                issued_at: now,
                expires_at: now + Duration::days(30),
            },
            now + Duration::seconds(1),
        )
        .expect("record issuance");
    certificate
        .apply_gateway_acknowledgement(&applied)
        .expect("ready certificate");
    assert_eq!(certificate.state, GatewayCertificateState::Ready);
    assert_eq!(
        certificate.ready_at,
        Some(canonical_timestamp(applied.acknowledged_at))
    );
    certificate
        .revoke("domain ownership removed", now + Duration::seconds(3))
        .expect("revoke ready certificate");
    assert_eq!(certificate.state, GatewayCertificateState::Revoked);
    assert_eq!(
        certificate.revoked_at,
        Some(canonical_timestamp(now + Duration::seconds(3)))
    );
}

#[test]
fn gateway_certificate_records_a_bounded_provisioning_failure() {
    let now = Utc::now();
    let certificate_id = GatewayCertificateId::new();
    let request = GatewayCertificateRequest::new(
        certificate_id.as_uuid(),
        vec!["api.example.com".into()],
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/certificate.pem"),
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/private-key.pem"),
    )
    .expect("certificate request");
    let mut certificate = GatewayCertificate::provision(
        certificate_id,
        OrganizationId::new(),
        NodeId::new(),
        vec![DomainClaimId::new()],
        1,
        NodeCommandId::new(),
        format!("sha256:{}", "a".repeat(64)),
        request,
        now,
    )
    .expect("provision certificate");
    certificate
        .fail_provisioning(
            format!("sha256:{}", "b".repeat(64)),
            " CA rejected the CSR\n",
            now + Duration::seconds(1),
        )
        .expect("record provisioning failure");
    assert_eq!(certificate.state, GatewayCertificateState::Failed);
    assert_eq!(certificate.failure.as_deref(), Some("CA rejected the CSR"));
    assert!(certificate.material.is_none());
    assert!(certificate
        .revoke("not ready", now + Duration::seconds(2))
        .is_err());
}

#[test]
fn route_activates_only_for_the_exact_gateway_publication() {
    let now = Utc::now();
    let mut route = route(now);
    let command_id = NodeCommandId::new();
    let snapshot = GatewaySnapshot::new(
        route.gateway_node_id.as_uuid(),
        3,
        Some(2),
        now,
        now + Duration::minutes(10),
        "management { enabled = true }\n",
    )
    .expect("snapshot");
    route
        .stage(
            snapshot.revision,
            command_id,
            snapshot.snapshot_digest.clone(),
            now + Duration::seconds(1),
        )
        .expect("stage");
    let wrong = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: command_id.as_uuid(),
        node_id: route.gateway_node_id.as_uuid(),
        gateway_id: route.gateway_node_id.as_uuid(),
        revision: 4,
        snapshot_digest: snapshot.snapshot_digest.clone(),
        expires_at: snapshot.expires_at,
        state: GatewayAckState::Applied,
        ready: true,
        message: None,
        acknowledged_at: now + Duration::seconds(2),
        management_protocol: Some(a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1()),
    };
    assert!(route.apply_gateway_acknowledgement(&wrong).is_err());
    assert_eq!(route.state, RouteState::Publishing);

    let applied = NodeGatewayAck {
        revision: 3,
        ..wrong
    };
    route
        .apply_gateway_acknowledgement(&applied)
        .expect("apply exact acknowledgement");
    assert_eq!(route.state, RouteState::Active);
    assert_eq!(
        route.activated_at,
        Some(canonical_timestamp(applied.acknowledged_at))
    );
}

#[test]
fn rejected_publication_preserves_failure_without_false_activation() {
    let now = Utc::now();
    let mut route = route(now);
    let command_id = NodeCommandId::new();
    let snapshot = GatewaySnapshot::new(
        route.gateway_node_id.as_uuid(),
        1,
        None,
        now,
        now + Duration::minutes(10),
        "management { enabled = true }\n",
    )
    .expect("snapshot");
    route
        .stage(1, command_id, snapshot.snapshot_digest.clone(), now)
        .expect("stage");
    route
        .apply_gateway_acknowledgement(&NodeGatewayAck {
            schema: NodeGatewayAck::SCHEMA.into(),
            acknowledgement_id: Uuid::now_v7(),
            command_id: command_id.as_uuid(),
            node_id: route.gateway_node_id.as_uuid(),
            gateway_id: route.gateway_node_id.as_uuid(),
            revision: 1,
            snapshot_digest: snapshot.snapshot_digest,
            expires_at: snapshot.expires_at,
            state: GatewayAckState::Rejected,
            ready: false,
            message: Some("validation failed".into()),
            acknowledged_at: now + Duration::seconds(1),
            management_protocol: Some(
                a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1(),
            ),
        })
        .expect("reject");
    assert_eq!(route.state, RouteState::Rejected);
    assert_eq!(route.failure.as_deref(), Some("validation failed"));
    assert_eq!(route.activated_at, None);
}

#[test]
fn route_cutover_rejects_equal_and_stale_runtime_generations() {
    let now = canonical_timestamp(Utc::now());
    let mut active = route(now);
    active.target.runtime_generation = 2;
    let command_id = NodeCommandId::new();
    let digest = format!("sha256:{}", "a".repeat(64));
    active
        .stage(1, command_id, digest.clone(), now)
        .expect("stage active route");
    active
        .apply_gateway_acknowledgement(&NodeGatewayAck {
            schema: NodeGatewayAck::SCHEMA.into(),
            acknowledgement_id: Uuid::now_v7(),
            command_id: command_id.as_uuid(),
            node_id: active.gateway_node_id.as_uuid(),
            gateway_id: active.gateway_node_id.as_uuid(),
            revision: 1,
            snapshot_digest: digest,
            expires_at: now + Duration::minutes(10),
            state: GatewayAckState::Applied,
            ready: true,
            message: None,
            acknowledged_at: now + Duration::seconds(1),
            management_protocol: Some(
                a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1(),
            ),
        })
        .expect("activate route");

    for candidate_generation in [2, 1] {
        let candidate_revision_id = WorkloadRevisionId::new();
        let target = RouteTarget::new(
            active.workload_id,
            candidate_revision_id,
            format!(
                "workload:{}:revision:{candidate_revision_id}",
                active.workload_id
            ),
            candidate_generation,
            active.target.port_name.clone(),
            active.target.upstream.clone(),
            now + Duration::seconds(1),
        )
        .expect("candidate target");
        assert!(active
            .prepare_cutover(
                target,
                GatewayCertificateId::new(),
                now + Duration::seconds(2),
            )
            .is_err());
    }
}

#[test]
fn active_route_certificate_convergence_preserves_service_until_exact_apply() {
    let now = Utc::now();
    let mut active = route(now);
    let first_command = NodeCommandId::new();
    let first_snapshot = GatewaySnapshot::new(
        active.gateway_node_id.as_uuid(),
        1,
        None,
        now,
        now + Duration::minutes(10),
        "management { enabled = true }\n",
    )
    .expect("snapshot");
    active
        .stage(
            1,
            first_command,
            first_snapshot.snapshot_digest.clone(),
            now,
        )
        .expect("stage initial route");
    active
        .apply_gateway_acknowledgement(&NodeGatewayAck {
            schema: NodeGatewayAck::SCHEMA.into(),
            acknowledgement_id: Uuid::now_v7(),
            command_id: first_command.as_uuid(),
            node_id: active.gateway_node_id.as_uuid(),
            gateway_id: active.gateway_node_id.as_uuid(),
            revision: 1,
            snapshot_digest: first_snapshot.snapshot_digest,
            expires_at: first_snapshot.expires_at,
            state: GatewayAckState::Applied,
            ready: true,
            message: None,
            acknowledged_at: now + Duration::seconds(1),
            management_protocol: Some(
                a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1(),
            ),
        })
        .expect("activate route");
    let activated_at = active.activated_at;
    let previous_version = active.aggregate_version;
    let replacement_certificate = GatewayCertificateId::new();
    let replacement_command = NodeCommandId::new();
    let replacement_digest = format!("sha256:{}", "d".repeat(64));

    assert!(active
        .bind_gateway_certificate(
            2,
            replacement_command,
            replacement_digest.clone(),
            replacement_certificate,
            now + Duration::seconds(2),
        )
        .expect("bind replacement"));
    assert_eq!(active.state, RouteState::Active);
    assert_eq!(active.activated_at, activated_at);
    assert_eq!(active.gateway_certificate_id, Some(replacement_certificate));
    assert_eq!(active.aggregate_version, previous_version + 1);
    assert!(!active
        .bind_gateway_certificate(
            2,
            replacement_command,
            replacement_digest,
            replacement_certificate,
            now + Duration::seconds(2),
        )
        .expect("exact replay"));
}

#[test]
fn revoked_domain_policy_removes_only_an_active_route() {
    let now = Utc::now();
    let mut active = route(now);
    let first_command = NodeCommandId::new();
    let first_snapshot = GatewaySnapshot::new(
        active.gateway_node_id.as_uuid(),
        1,
        None,
        now,
        now + Duration::minutes(10),
        "management { enabled = true }\n",
    )
    .expect("snapshot");
    active
        .stage(
            1,
            first_command,
            first_snapshot.snapshot_digest.clone(),
            now,
        )
        .expect("stage initial route");
    active
        .apply_gateway_acknowledgement(&NodeGatewayAck {
            schema: NodeGatewayAck::SCHEMA.into(),
            acknowledgement_id: Uuid::now_v7(),
            command_id: first_command.as_uuid(),
            node_id: active.gateway_node_id.as_uuid(),
            gateway_id: active.gateway_node_id.as_uuid(),
            revision: 1,
            snapshot_digest: first_snapshot.snapshot_digest,
            expires_at: first_snapshot.expires_at,
            state: GatewayAckState::Applied,
            ready: true,
            message: None,
            acknowledged_at: now + Duration::seconds(1),
            management_protocol: Some(
                a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1(),
            ),
        })
        .expect("activate route");

    active
        .reject_for_domain_revocation(
            2,
            NodeCommandId::new(),
            format!("sha256:{}", "e".repeat(64)),
            now + Duration::seconds(2),
        )
        .expect("remove revoked-domain route");
    assert_eq!(active.state, RouteState::Rejected);
    assert_eq!(
        active.failure.as_deref(),
        Some("domain ownership is no longer verified")
    );
    assert_eq!(active.activated_at, None);
}

#[test]
fn certificate_convergence_is_exact_and_preserves_route_versions() {
    let now = Utc::now();
    let node_id = NodeId::new();
    let command_id = NodeCommandId::new();
    let previous_certificate_id = GatewayCertificateId::new();
    let replacement_certificate_id = GatewayCertificateId::new();
    let route = route(now);
    let digest = format!("sha256:{}", "f".repeat(64));
    let retained =
        vec![GatewayRouteVersion::new(route.id, route.aggregate_version).expect("route version")];
    let mut convergence = GatewayCertificateConvergence::stage(
        route.organization_id,
        node_id,
        2,
        command_id,
        previous_certificate_id,
        Some(replacement_certificate_id),
        digest.clone(),
        retained.clone(),
        Vec::new(),
        GatewayCertificateConvergenceReason::Renewal,
        now,
    )
    .expect("certificate convergence");
    assert_eq!(convergence.retained_routes, retained);
    assert_eq!(
        convergence.state,
        GatewayCertificateConvergenceState::Pending
    );

    let mut wrong = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: command_id.as_uuid(),
        node_id: node_id.as_uuid(),
        gateway_id: node_id.as_uuid(),
        revision: 3,
        snapshot_digest: digest.clone(),
        expires_at: now + Duration::minutes(10),
        state: GatewayAckState::Applied,
        ready: true,
        message: None,
        acknowledged_at: now + Duration::seconds(1),
        management_protocol: Some(a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1()),
    };
    assert!(convergence.acknowledge(&wrong).is_err());
    wrong.revision = 2;
    convergence
        .acknowledge(&wrong)
        .expect("exact convergence acknowledgement");
    assert_eq!(
        convergence.state,
        GatewayCertificateConvergenceState::Applied
    );
    assert_eq!(
        convergence.acknowledged_at,
        Some(canonical_timestamp(wrong.acknowledged_at))
    );
}

#[test]
fn complete_domain_revocation_convergence_requires_no_replacement_certificate() {
    let now = Utc::now();
    let route = route(now);
    let convergence = GatewayCertificateConvergence::stage(
        route.organization_id,
        route.gateway_node_id,
        2,
        NodeCommandId::new(),
        GatewayCertificateId::new(),
        None,
        format!("sha256:{}", "a".repeat(64)),
        Vec::new(),
        vec![GatewayRouteVersion::new(route.id, route.aggregate_version).expect("route version")],
        GatewayCertificateConvergenceReason::DomainRevocation,
        now,
    )
    .expect("route-less convergence");
    assert!(convergence.replacement_certificate_id.is_none());
    assert!(GatewayCertificateConvergence::stage(
        route.organization_id,
        route.gateway_node_id,
        2,
        NodeCommandId::new(),
        GatewayCertificateId::new(),
        None,
        format!("sha256:{}", "b".repeat(64)),
        vec![GatewayRouteVersion::new(route.id, route.aggregate_version).expect("route version")],
        Vec::new(),
        GatewayCertificateConvergenceReason::Renewal,
        now,
    )
    .is_err());
}

#[test]
fn snapshot_renewal_retains_the_active_certificate_without_reissuing_it() {
    let now = Utc::now();
    let route = route(now);
    let previous_certificate_id = GatewayCertificateId::new();
    let convergence = GatewayCertificateConvergence::stage(
        route.organization_id,
        route.gateway_node_id,
        2,
        NodeCommandId::new(),
        previous_certificate_id,
        None,
        format!("sha256:{}", "c".repeat(64)),
        vec![GatewayRouteVersion::new(route.id, route.aggregate_version).expect("route version")],
        Vec::new(),
        GatewayCertificateConvergenceReason::SnapshotRenewal,
        now,
    )
    .expect("snapshot renewal");
    assert_eq!(
        convergence.active_certificate_id(),
        Some(previous_certificate_id)
    );
    assert!(convergence.replacement_certificate_id.is_none());
    assert!(GatewayCertificateConvergence::stage(
        route.organization_id,
        route.gateway_node_id,
        3,
        NodeCommandId::new(),
        previous_certificate_id,
        Some(GatewayCertificateId::new()),
        format!("sha256:{}", "d".repeat(64)),
        vec![GatewayRouteVersion::new(route.id, route.aggregate_version).expect("route version")],
        Vec::new(),
        GatewayCertificateConvergenceReason::SnapshotRenewal,
        now,
    )
    .is_err());
}
