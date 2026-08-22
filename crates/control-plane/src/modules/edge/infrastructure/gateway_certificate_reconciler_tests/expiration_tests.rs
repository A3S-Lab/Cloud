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
    assert!(expiry_facts(fixture.repository.as_ref()).await.is_empty());
}

#[tokio::test]
async fn certificate_expiry_firing_is_scoped_and_retry_safe() {
    let fixture = Fixture::new();
    let base = Utc::now();
    let claim = fixture.verified_claim("firing.example.com", base).await;
    let previous_expires_at = base + Duration::days(30);
    let (route, previous) = fixture
        .activate_route(
            &claim,
            "firing.example.com",
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
        .expect("stage certificate renewal");
    let first = fixture
        .repository
        .pending_gateway_certificate_convergences(10)
        .await
        .expect("pending certificate renewal")
        .pop()
        .expect("certificate renewal");
    let replacement_certificate_id = first
        .convergence
        .replacement_certificate_id
        .expect("replacement certificate");
    let facts = expiry_facts(fixture.repository.as_ref()).await;
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].event_key, "edge.gateway-certificate.expiring");
    assert_eq!(
        facts[0].aggregate_id,
        renewal_subject_id(route.id, fixture.node_id)
    );
    assert_eq!(
        facts[0].aggregate_version,
        certificate_expiry_aggregate_version(
            previous.gateway_revision,
            GatewayCertificateExpiryStatus::Expiring
        )
        .expect("expiry firing aggregate version")
    );
    assert_eq!(facts[0].occurred_at, canonical_timestamp(staged_at));
    assert_eq!(
        facts[0].correlation_id,
        first.publication.command_correlation_id
    );
    let payload: GatewayCertificateExpiryChanged =
        serde_json::from_value(facts[0].payload.clone()).expect("expiry firing payload");
    assert_eq!(payload.organization_id, fixture.organization_id);
    assert_eq!(payload.project_id, fixture.project_id);
    assert_eq!(payload.environment_id, fixture.environment_id);
    assert_eq!(payload.route_id, route.id);
    assert_eq!(payload.workload_id, fixture.workload_id);
    assert_eq!(payload.node_id, fixture.node_id);
    assert_eq!(payload.hostname, "firing.example.com");
    assert_eq!(payload.path_prefix, "/");
    assert_eq!(
        payload.certificate_gateway_revision,
        previous.gateway_revision
    );
    assert_eq!(
        payload.renewal_gateway_revision,
        first.convergence.gateway_revision
    );
    assert_eq!(payload.previous_certificate_id, previous.id);
    assert_eq!(
        payload.replacement_certificate_id,
        replacement_certificate_id
    );
    assert_eq!(payload.active_certificate_id, previous.id);
    assert_eq!(
        payload.active_certificate_expires_at,
        canonical_timestamp(previous_expires_at)
    );
    assert_eq!(payload.status, GatewayCertificateExpiryStatus::Expiring);
    assert!(!facts[0].payload.to_string().contains("certificate_pem"));

    reconciler
        .run_once(first.publication.command_not_after)
        .await
        .expect("expire and retry certificate renewal");
    let retries = fixture
        .repository
        .pending_gateway_certificate_convergences(10)
        .await
        .expect("retry certificate renewal");
    assert_eq!(retries.len(), 1);
    assert!(retries[0].convergence.gateway_revision > first.convergence.gateway_revision);
    assert_eq!(retries[0].convergence.previous_certificate_id, previous.id);
    let retried_facts = expiry_facts(fixture.repository.as_ref()).await;
    assert_eq!(retried_facts.len(), 1);
    assert_eq!(retried_facts[0].event_id, facts[0].event_id);
    let retry_candidate = GatewayCertificateExpiryChanged::envelopes(
        &retries[0].convergence,
        &retries[0].publication,
        &previous,
        std::slice::from_ref(&route),
    )
    .expect("retry expiry firing candidate")
    .pop()
    .expect("retry expiry firing fact");
    assert_eq!(retry_candidate.event_id, facts[0].event_id);
    assert_ne!(retry_candidate.payload, facts[0].payload);
    assert!(
        GatewayCertificateExpiryChanged::same_firing_identity(&facts[0], &retry_candidate)
            .expect("same certificate retry identity")
    );
    let mut forged = facts[0].clone();
    forged.payload["active_certificate_id"] = serde_json::json!(GatewayCertificateId::new());
    assert!(
        GatewayCertificateExpiryChanged::same_firing_identity(&forged, &retry_candidate).is_err()
    );
}

#[tokio::test]
async fn applied_certificate_renewal_resolves_the_expiry_fact() {
    let fixture = Fixture::new();
    let base = Utc::now();
    let claim = fixture.verified_claim("resolved.example.com", base).await;
    let previous_expires_at = base + Duration::days(30);
    let (route, previous) = fixture
        .activate_route(
            &claim,
            "resolved.example.com",
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
        .expect("stage certificate renewal");
    let renewal = fixture
        .repository
        .pending_gateway_certificate_convergences(10)
        .await
        .expect("pending certificate renewal")
        .pop()
        .expect("certificate renewal");
    let replacement = renewal
        .certificate
        .as_ref()
        .expect("replacement certificate");
    let replacement_expires_at = base + Duration::days(120);
    issue_certificate(
        fixture.repository.as_ref(),
        replacement,
        staged_at + Duration::seconds(1),
        replacement_expires_at,
    )
    .await;
    let acknowledged_at = staged_at + Duration::seconds(2);
    apply_convergence(
        fixture.repository.as_ref(),
        &renewal.publication,
        acknowledged_at,
    )
    .await;

    let facts = expiry_facts(fixture.repository.as_ref()).await;
    assert_eq!(facts.len(), 2);
    assert_eq!(facts[0].event_key, "edge.gateway-certificate.expiring");
    assert_eq!(
        facts[1].event_key,
        "edge.gateway-certificate.expiry-resolved"
    );
    assert_eq!(facts[1].aggregate_id, facts[0].aggregate_id);
    assert_eq!(
        facts[1].aggregate_id,
        renewal_subject_id(route.id, fixture.node_id)
    );
    assert_eq!(
        facts[0].aggregate_version,
        certificate_expiry_aggregate_version(
            previous.gateway_revision,
            GatewayCertificateExpiryStatus::Expiring
        )
        .expect("expiry firing aggregate version")
    );
    assert_eq!(
        facts[1].aggregate_version,
        certificate_expiry_aggregate_version(
            replacement.gateway_revision,
            GatewayCertificateExpiryStatus::Resolved
        )
        .expect("expiry resolution aggregate version")
    );
    assert!(facts[1].aggregate_version > facts[0].aggregate_version);
    assert_eq!(facts[1].occurred_at, canonical_timestamp(acknowledged_at));
    let payload: GatewayCertificateExpiryChanged =
        serde_json::from_value(facts[1].payload.clone()).expect("expiry resolution payload");
    assert_eq!(payload.status, GatewayCertificateExpiryStatus::Resolved);
    assert_eq!(payload.previous_certificate_id, previous.id);
    assert_eq!(payload.replacement_certificate_id, replacement.id);
    assert_eq!(payload.active_certificate_id, replacement.id);
    assert_eq!(
        payload.active_certificate_expires_at,
        canonical_timestamp(replacement_expires_at)
    );
    assert_eq!(
        payload.certificate_gateway_revision,
        replacement.gateway_revision
    );
    assert_eq!(
        payload.renewal_gateway_revision,
        renewal.convergence.gateway_revision
    );
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
