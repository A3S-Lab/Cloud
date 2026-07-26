use super::*;
use crate::modules::edge::domain::GatewayCertificateConvergenceState;

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
