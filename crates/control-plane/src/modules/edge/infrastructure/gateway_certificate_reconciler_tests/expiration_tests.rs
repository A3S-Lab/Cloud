use super::*;
use crate::modules::edge::domain::{
    GatewayCertificateConvergenceReason, GatewayCertificateConvergenceState,
};

#[tokio::test]
async fn expired_convergence_preserves_installed_state_and_retries_a_new_revision() {
    let fixture = Fixture::new();
    let base = Utc::now();
    let claim = fixture.verified_claim("expiry.example.com", base).await;
    let (route, certificate) = fixture
        .activate_route(
            &claim,
            "expiry.example.com",
            base + Duration::seconds(1),
            base + Duration::days(30),
        )
        .await;
    let installed_revision = route.gateway_revision.expect("installed revision");
    let queue = Arc::new(RecordingGatewayQueue::default());
    let authority = Arc::new(RecordingGatewayCertificateAuthority::default());
    let reconciler = reconciler(&fixture, queue.clone(), authority);
    let staged_at = base + Duration::hours(18) + Duration::seconds(2);

    let staged = reconciler
        .run_once(staged_at)
        .await
        .expect("stage expiring snapshot convergence");
    assert_eq!(staged.staged_convergences, 1);
    let first = fixture
        .repository
        .pending_gateway_certificate_convergences(10)
        .await
        .expect("first pending convergence")
        .pop()
        .expect("first convergence");
    assert!(first.certificate.is_none());
    assert_eq!(first.convergence.previous_certificate_id, certificate.id);

    let expired_at = first.publication.command_not_after;
    let retried = reconciler
        .run_once(expired_at)
        .await
        .expect("expire and retry convergence");
    assert_eq!(retried.pending_convergences, 1);
    assert_eq!(retried.unavailable_convergences, 1);
    assert_eq!(retried.staged_convergences, 1);
    assert_eq!(retried.dispatched_commands, 1);
    assert!(retried.failures.is_empty());

    let unavailable = fixture
        .repository
        .find_gateway_certificate_convergence(fixture.node_id, first.publication.revision)
        .await
        .expect("find unavailable convergence")
        .expect("unavailable convergence");
    assert_eq!(
        unavailable.state,
        GatewayCertificateConvergenceState::Unavailable
    );
    assert_eq!(
        unavailable.failure.as_deref(),
        Some("Gateway certificate convergence command expired before acknowledgement")
    );
    let replayed_unavailable = fixture
        .repository
        .mark_gateway_certificate_convergence_unavailable(
            fixture.organization_id,
            fixture.node_id,
            first.publication.revision,
            first.publication.command_id,
            "Gateway certificate convergence command expired before acknowledgement",
            expired_at,
        )
        .await
        .expect("replay unavailable convergence");
    assert_eq!(
        replayed_unavailable.publication.state,
        GatewayPublicationState::Unavailable
    );

    let retry = fixture
        .repository
        .pending_gateway_certificate_convergences(10)
        .await
        .expect("retry convergence")
        .pop()
        .expect("pending retry");
    assert!(retry.publication.revision > first.publication.revision);
    assert_eq!(
        retry.publication.expected_revision,
        Some(installed_revision)
    );
    assert_eq!(retry.convergence.previous_certificate_id, certificate.id);
    assert!(retry.certificate.is_none());
    assert_eq!(
        fixture
            .repository
            .gateway_scope(fixture.node_id)
            .await
            .expect("scope after retry")
            .installed_revision,
        Some(installed_revision)
    );
    let retained_route = fixture
        .repository
        .find_route(fixture.organization_id, route.id)
        .await
        .expect("retained active route");
    assert_eq!(retained_route.state, RouteState::Active);
    assert_eq!(retained_route.gateway_revision, Some(installed_revision));
    assert_eq!(retained_route.gateway_certificate_id, Some(certificate.id));
    assert_eq!(
        fixture
            .repository
            .find_gateway_certificate(fixture.node_id, certificate.id)
            .await
            .expect("retained certificate")
            .state,
        GatewayCertificateState::Ready
    );
    assert!(fixture
        .repository
        .obsolete_gateway_certificates(10)
        .await
        .expect("obsolete certificates")
        .is_empty());
    assert_eq!(queue.publications.lock().await.len(), 2);
}

#[tokio::test]
async fn expired_certificate_renewal_emits_one_unavailable_fact() {
    let fixture = Fixture::new();
    let base = Utc::now();
    let claim = fixture
        .verified_claim("renewal-expiry.example.com", base)
        .await;
    let previous_expires_at = base + Duration::days(30);
    let (route, previous) = fixture
        .activate_route(
            &claim,
            "renewal-expiry.example.com",
            base + Duration::seconds(1),
            previous_expires_at,
        )
        .await;
    let queue = Arc::new(RecordingGatewayQueue::default());
    let authority = Arc::new(RecordingGatewayCertificateAuthority::default());
    let reconciler = reconciler(&fixture, queue, authority);
    let staged_at = base + Duration::days(24);
    reconciler
        .run_once(staged_at)
        .await
        .expect("stage expiring certificate renewal");
    let first = fixture
        .repository
        .pending_gateway_certificate_convergences(10)
        .await
        .expect("pending renewal")
        .pop()
        .expect("renewal");
    assert_eq!(
        first.convergence.reason,
        GatewayCertificateConvergenceReason::Renewal
    );
    assert!(first.convergence.replacement_certificate_id.is_some());
    assert!(renewal_facts(fixture.repository.as_ref()).await.is_empty());

    let expired_at = first.publication.command_not_after;
    let report = reconciler
        .run_once(expired_at)
        .await
        .expect("expire certificate renewal");
    assert_eq!(report.unavailable_convergences, 1);
    let facts = renewal_facts(fixture.repository.as_ref()).await;
    assert_eq!(facts.len(), 1);
    assert_eq!(
        facts[0].event_key,
        "edge.gateway-certificate.renewal-failed"
    );
    assert_eq!(
        facts[0].aggregate_id,
        renewal_subject_id(route.id, fixture.node_id)
    );
    let payload: GatewayCertificateRenewalChanged =
        serde_json::from_value(facts[0].payload.clone()).expect("unavailable renewal payload");
    assert_eq!(payload.status, GatewayCertificateRenewalStatus::Failed);
    assert_eq!(
        payload.failure_kind,
        Some(GatewayCertificateRenewalFailureKind::Unavailable)
    );
    assert_eq!(payload.active_certificate_id, previous.id);
    assert_eq!(
        payload.active_certificate_expires_at,
        canonical_timestamp(previous_expires_at)
    );
    assert!(!facts[0]
        .payload
        .to_string()
        .contains("expired before acknowledgement"));

    fixture
        .repository
        .mark_gateway_certificate_convergence_unavailable(
            fixture.organization_id,
            fixture.node_id,
            first.publication.revision,
            first.publication.command_id,
            "Gateway certificate convergence command expired before acknowledgement",
            expired_at,
        )
        .await
        .expect("replay unavailable renewal");
    assert_eq!(renewal_facts(fixture.repository.as_ref()).await.len(), 1);
}
