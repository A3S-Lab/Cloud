use super::*;
use crate::modules::edge::domain::events::{
    expiry_risk_subject_id, renewal_subject_id, GatewayCertificateExpiryRiskChanged,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GatewayCertificateId, NodeCommandId, NodeId,
};
use a3s_cloud_contracts::GatewayCertificateRequest;
use chrono::{DateTime, Duration, TimeZone, Utc};
use uuid::Uuid;

#[test]
fn certificate_expiry_risk_is_exact_at_the_window_and_replays_silently() {
    let observed_at = fixture_time();
    let route = active_route(observed_at - Duration::seconds(1));
    let certificate = ready_certificate(
        &route,
        route.gateway_certificate_id.expect("certificate"),
        observed_at + Duration::hours(24),
        observed_at - Duration::seconds(1),
    );

    let risk = GatewayCertificateExpiryRisk::observe(None, &route, &certificate, observed_at)
        .expect("observe threshold equality")
        .expect("at-risk transition");
    assert_eq!(risk.state, GatewayCertificateExpiryRiskState::AtRisk);
    assert_eq!(risk.generation, 1);
    assert!(GatewayCertificateExpiryRisk::observe(
        Some(&risk),
        &route,
        &certificate,
        observed_at + Duration::seconds(1),
    )
    .expect("replay observation")
    .is_none());

    let event = GatewayCertificateExpiryRiskChanged::envelope(None, &risk, &route, Uuid::now_v7())
        .expect("at-risk event");
    assert_eq!(event.event_key, "edge.gateway-certificate.expiry-at-risk");
    assert_eq!(event.aggregate_version, 1);
    assert_eq!(
        event.aggregate_id,
        expiry_risk_subject_id(route.id, route.gateway_node_id)
    );
    assert_ne!(
        event.aggregate_id,
        renewal_subject_id(route.id, route.gateway_node_id)
    );
    let payload: GatewayCertificateExpiryRiskChanged =
        serde_json::from_value(event.payload.clone()).expect("typed payload");
    assert_eq!(payload.active_certificate_id, certificate.id);
    assert_eq!(payload.risk_window_seconds, 24 * 60 * 60);
    assert_eq!(payload.state, GatewayCertificateExpiryRiskState::AtRisk);
    let encoded = serde_json::to_string(&event.payload).expect("encoded payload");
    assert!(!encoded.contains("certificate_pem"));
    assert!(!encoded.contains("PRIVATE"));
}

#[test]
fn applied_short_lived_replacement_stays_at_risk_until_a_safe_certificate_applies() {
    let observed_at = fixture_time();
    let mut route = active_route(observed_at - Duration::seconds(1));
    let first_certificate = ready_certificate(
        &route,
        route.gateway_certificate_id.expect("certificate"),
        observed_at + Duration::hours(1),
        observed_at - Duration::seconds(1),
    );
    let first =
        GatewayCertificateExpiryRisk::observe(None, &route, &first_certificate, observed_at)
            .expect("first observation")
            .expect("first risk");

    let short_lived_id = GatewayCertificateId::new();
    apply_certificate_binding(
        &mut route,
        short_lived_id,
        8,
        observed_at + Duration::seconds(1),
    );
    let short_lived = ready_certificate(
        &route,
        short_lived_id,
        observed_at + Duration::hours(12),
        observed_at + Duration::seconds(1),
    );
    let second = GatewayCertificateExpiryRisk::observe(
        Some(&first),
        &route,
        &short_lived,
        observed_at + Duration::seconds(1),
    )
    .expect("short-lived replacement")
    .expect("new risk generation");
    assert_eq!(second.state, GatewayCertificateExpiryRiskState::AtRisk);
    assert_eq!(second.generation, 2);
    assert_eq!(second.active_certificate_id, short_lived_id);

    let safe_id = GatewayCertificateId::new();
    apply_certificate_binding(&mut route, safe_id, 9, observed_at + Duration::seconds(2));
    let safe = ready_certificate(
        &route,
        safe_id,
        observed_at + Duration::days(30),
        observed_at + Duration::seconds(2),
    );
    let clear = GatewayCertificateExpiryRisk::observe(
        Some(&second),
        &route,
        &safe,
        observed_at + Duration::seconds(2),
    )
    .expect("safe replacement")
    .expect("clear transition");
    assert_eq!(clear.state, GatewayCertificateExpiryRiskState::Clear);
    assert_eq!(clear.generation, 3);
    assert_eq!(clear.previous_at_risk_certificate_id, Some(short_lived_id));

    let event = GatewayCertificateExpiryRiskChanged::envelope(
        Some(&second),
        &clear,
        &route,
        Uuid::now_v7(),
    )
    .expect("clear event");
    assert_eq!(
        event.event_key,
        "edge.gateway-certificate.expiry-risk-cleared"
    );
    let payload: GatewayCertificateExpiryRiskChanged =
        serde_json::from_value(event.payload).expect("typed clear payload");
    assert_eq!(
        payload.previous_at_risk_certificate_id,
        Some(short_lived_id)
    );
}

#[test]
fn safe_initial_certificate_and_backward_observation_are_fail_closed() {
    let observed_at = fixture_time();
    let route = active_route(observed_at - Duration::seconds(1));
    let certificate = ready_certificate(
        &route,
        route.gateway_certificate_id.expect("certificate"),
        observed_at + Duration::days(30),
        observed_at - Duration::seconds(1),
    );
    assert!(
        GatewayCertificateExpiryRisk::observe(None, &route, &certificate, observed_at)
            .expect("safe initial observation")
            .is_none()
    );
    let mut mismatched_certificate = certificate.clone();
    mismatched_certificate.node_id = NodeId::new();
    assert!(GatewayCertificateExpiryRisk::observe(
        None,
        &route,
        &mismatched_certificate,
        observed_at,
    )
    .is_err());

    let at_risk_certificate = ready_certificate(
        &route,
        certificate.id,
        observed_at + Duration::hours(1),
        observed_at - Duration::seconds(1),
    );
    let risk =
        GatewayCertificateExpiryRisk::observe(None, &route, &at_risk_certificate, observed_at)
            .expect("at-risk observation")
            .expect("at-risk state");
    assert!(GatewayCertificateExpiryRisk::observe(
        Some(&risk),
        &route,
        &at_risk_certificate,
        observed_at - Duration::seconds(1),
    )
    .is_err());
}

#[test]
fn cleared_certificate_can_age_back_into_risk_without_changing_identity() {
    let observed_at = canonical_timestamp(Utc::now());
    let mut route = active_route(observed_at);
    let expiring = ready_certificate(
        &route,
        route.gateway_certificate_id.expect("certificate"),
        observed_at + Duration::hours(24),
        observed_at,
    );
    let first = GatewayCertificateExpiryRisk::observe(None, &route, &expiring, observed_at)
        .expect("first observation")
        .expect("first risk");

    let safe_at = observed_at + Duration::seconds(1);
    let safe_id = GatewayCertificateId::new();
    apply_certificate_binding(&mut route, safe_id, 8, safe_at);
    let safe = ready_certificate(&route, safe_id, safe_at + Duration::days(30), safe_at);
    let clear = GatewayCertificateExpiryRisk::observe(Some(&first), &route, &safe, safe_at)
        .expect("safe observation")
        .expect("clear transition");
    let clear_event =
        GatewayCertificateExpiryRiskChanged::envelope(Some(&first), &clear, &route, Uuid::now_v7())
            .expect("clear event");
    assert_eq!(clear_event.aggregate_version, 2);

    let aged_at = safe.material.as_ref().expect("safe material").expires_at - Duration::hours(24);
    let aged = GatewayCertificateExpiryRisk::observe(Some(&clear), &route, &safe, aged_at)
        .expect("aged observation")
        .expect("aged risk transition");
    assert_eq!(aged.state, GatewayCertificateExpiryRiskState::AtRisk);
    assert_eq!(aged.active_certificate_id, safe.id);
    assert_eq!(aged.generation, 3);
    let event =
        GatewayCertificateExpiryRiskChanged::envelope(Some(&clear), &aged, &route, Uuid::now_v7())
            .expect("aged risk event");
    assert_eq!(event.event_key, "edge.gateway-certificate.expiry-at-risk");
    assert_eq!(event.aggregate_version, 3);
}

fn fixture_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 22, 9, 0, 0)
        .single()
        .expect("fixture time")
}

fn active_route(created_at: DateTime<Utc>) -> Route {
    let mut route = super::route(created_at);
    route.state = RouteState::Active;
    route.gateway_revision = Some(7);
    route.gateway_command_id = Some(NodeCommandId::new());
    route.snapshot_digest = Some(format!("sha256:{}", "a".repeat(64)));
    route.aggregate_version = 3;
    route.updated_at = canonical_timestamp(created_at);
    route.activated_at = Some(canonical_timestamp(created_at));
    route
}

fn apply_certificate_binding(
    route: &mut Route,
    certificate_id: GatewayCertificateId,
    revision: u64,
    applied_at: DateTime<Utc>,
) {
    route.gateway_certificate_id = Some(certificate_id);
    route.gateway_revision = Some(revision);
    route.gateway_command_id = Some(NodeCommandId::new());
    route.snapshot_digest = Some(format!("sha256:{}", "b".repeat(64)));
    route.aggregate_version += 1;
    route.updated_at = canonical_timestamp(applied_at);
    route.activated_at = Some(canonical_timestamp(applied_at));
}

fn ready_certificate(
    route: &Route,
    certificate_id: GatewayCertificateId,
    expires_at: DateTime<Utc>,
    ready_at: DateTime<Utc>,
) -> GatewayCertificate {
    let request = GatewayCertificateRequest::new(
        certificate_id.as_uuid(),
        vec![route.hostname.as_str().into()],
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/certificate.pem"),
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/private-key.pem"),
    )
    .expect("certificate request");
    GatewayCertificate {
        id: certificate_id,
        organization_id: route.organization_id,
        node_id: route.gateway_node_id,
        domain_claim_ids: vec![route.domain_claim_id.expect("domain claim")],
        gateway_revision: route.gateway_revision.expect("revision"),
        gateway_command_id: route.gateway_command_id.expect("command"),
        snapshot_digest: route.snapshot_digest.clone().expect("digest"),
        request,
        state: GatewayCertificateState::Ready,
        csr_digest: Some(format!("sha256:{}", "c".repeat(64))),
        material: Some(GatewayCertificateMaterial {
            serial_number: certificate_id.to_string(),
            fingerprint: format!("sha256:{}", "d".repeat(64)),
            certificate_pem: "-----BEGIN CERTIFICATE-----\ndGVzdA==\n-----END CERTIFICATE-----\n"
                .into(),
            ca_bundle_pem: "-----BEGIN CERTIFICATE-----\ndGVzdC1jYQ==\n-----END CERTIFICATE-----\n"
                .into(),
            issued_at: canonical_timestamp(ready_at - Duration::seconds(1)),
            expires_at: canonical_timestamp(expires_at),
        }),
        failure: None,
        aggregate_version: 3,
        created_at: canonical_timestamp(ready_at - Duration::seconds(1)),
        updated_at: canonical_timestamp(ready_at),
        ready_at: Some(canonical_timestamp(ready_at)),
        revoked_at: None,
    }
}
