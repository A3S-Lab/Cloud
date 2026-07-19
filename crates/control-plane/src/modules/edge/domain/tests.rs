use super::*;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DomainClaimId, EnvironmentId, GatewayCertificateId, NodeCommandId, NodeId,
    OrganizationId, ProjectId, RouteId, WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{
    GatewayAckState, GatewayCertificateRequest, GatewaySnapshot, NodeGatewayAck,
};
use chrono::{Duration, Utc};
use uuid::Uuid;

fn route(now: chrono::DateTime<Utc>) -> Route {
    Route::create(
        RouteId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        NodeId::new(),
        RouteHostname::parse("API.Example.COM").expect("hostname"),
        RoutePath::parse("/v1").expect("path"),
        DomainClaimId::new(),
        DomainNamePattern::parse("api.example.com").expect("domain pattern"),
        GatewayCertificateId::new(),
        WorkloadId::new(),
        WorkloadRevisionId::new(),
        RoutePortName::parse("http").expect("port"),
        UpstreamEndpoint::parse("http://127.0.0.1:49152").expect("upstream"),
        now,
    )
    .expect("route")
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
        3,
        Some(2),
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
        revision: snapshot.revision,
        snapshot_digest: snapshot.snapshot_digest,
        state: GatewayAckState::Applied,
        message: None,
        acknowledged_at: now + Duration::seconds(2),
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
    let snapshot =
        GatewaySnapshot::new(3, Some(2), "management { enabled = true }\n").expect("snapshot");
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
        revision: 4,
        snapshot_digest: snapshot.snapshot_digest.clone(),
        state: GatewayAckState::Applied,
        message: None,
        acknowledged_at: now + Duration::seconds(2),
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
    let snapshot =
        GatewaySnapshot::new(1, None, "management { enabled = true }\n").expect("snapshot");
    route
        .stage(1, command_id, snapshot.snapshot_digest.clone(), now)
        .expect("stage");
    route
        .apply_gateway_acknowledgement(&NodeGatewayAck {
            schema: NodeGatewayAck::SCHEMA.into(),
            acknowledgement_id: Uuid::now_v7(),
            command_id: command_id.as_uuid(),
            node_id: route.gateway_node_id.as_uuid(),
            revision: 1,
            snapshot_digest: snapshot.snapshot_digest,
            state: GatewayAckState::Rejected,
            message: Some("validation failed".into()),
            acknowledged_at: now + Duration::seconds(1),
        })
        .expect("reject");
    assert_eq!(route.state, RouteState::Rejected);
    assert_eq!(route.failure.as_deref(), Some("validation failed"));
    assert_eq!(route.activated_at, None);
}
