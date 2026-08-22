use super::*;
use crate::modules::edge::domain::events::{
    expiry_risk_subject_id, GatewayCertificateExpiryRiskChanged,
};
use crate::modules::edge::domain::{
    GatewayCertificateExpiryRiskState, GATEWAY_CERTIFICATE_EXPIRY_RISK_WINDOW_SECONDS,
};

async fn expiry_risk_facts(
    repository: &InMemoryEdgeRepository,
) -> Vec<a3s_cloud_contracts::DomainEventEnvelope> {
    repository
        .outbox_events()
        .await
        .into_iter()
        .filter(|event| {
            matches!(
                event.event_key.as_str(),
                "edge.gateway-certificate.expiry-at-risk"
                    | "edge.gateway-certificate.expiry-risk-cleared"
            )
        })
        .collect()
}

fn acknowledgement(
    publication: &GatewayPublication,
    state: GatewayAckState,
    acknowledged_at: chrono::DateTime<Utc>,
    message: Option<&str>,
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
        message: message.map(str::to_owned),
        acknowledged_at,
        management_protocol: Some(a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1()),
    }
}

#[tokio::test]
async fn fixed_expiry_window_is_exact_pending_safe_and_replay_silent() {
    let fixture = Fixture::new();
    let base = canonical_timestamp(Utc::now());
    let claim = fixture
        .verified_claim("risk-window.example.com", base)
        .await;
    let scan_at = base + Duration::hours(2);
    let expires_at = scan_at + Duration::hours(24);
    let (route, certificate) = fixture
        .activate_route(
            &claim,
            "risk-window.example.com",
            base + Duration::seconds(1),
            expires_at,
        )
        .await;
    assert!(fixture
        .repository
        .find_gateway_certificate_expiry_risk(route.id, fixture.node_id)
        .await
        .expect("initial risk lookup")
        .is_none());

    let queue = Arc::new(RecordingGatewayQueue::default());
    let authority = Arc::new(RecordingGatewayCertificateAuthority::default());
    let reconciler = reconciler(&fixture, queue, authority);
    let before_window = scan_at - Duration::minutes(1);
    let staged = reconciler
        .run_once(before_window)
        .await
        .expect("stage ordinary renewal before emergency window");
    assert_eq!(staged.expiry_risk_targets, 0);
    assert_eq!(staged.projected_expiry_risks, 0);
    assert_eq!(staged.staged_convergences, 1);
    assert!(expiry_risk_facts(fixture.repository.as_ref())
        .await
        .is_empty());

    let report = reconciler
        .run_once(scan_at)
        .await
        .expect("observe exact emergency threshold");
    assert_eq!(report.pending_convergences, 1);
    assert_eq!(report.expiry_risk_targets, 1);
    assert_eq!(report.projected_expiry_risks, 1);
    assert!(report.failures.is_empty());
    let risk = fixture
        .repository
        .find_gateway_certificate_expiry_risk(route.id, fixture.node_id)
        .await
        .expect("risk lookup")
        .expect("at-risk projection");
    assert_eq!(risk.state, GatewayCertificateExpiryRiskState::AtRisk);
    assert_eq!(risk.active_certificate_id, certificate.id);
    assert_eq!(risk.active_certificate_expires_at, expires_at);
    assert_eq!(risk.generation, 1);
    assert_eq!(risk.updated_at, scan_at);

    let facts = expiry_risk_facts(fixture.repository.as_ref()).await;
    assert_eq!(facts.len(), 1);
    assert_eq!(
        facts[0].aggregate_id,
        expiry_risk_subject_id(route.id, fixture.node_id)
    );
    assert_ne!(
        facts[0].aggregate_id,
        renewal_subject_id(route.id, fixture.node_id)
    );
    assert_eq!(facts[0].aggregate_version, 1);
    let payload: GatewayCertificateExpiryRiskChanged =
        serde_json::from_value(facts[0].payload.clone()).expect("expiry-risk payload");
    assert_eq!(payload.route_id, route.id);
    assert_eq!(payload.workload_id, fixture.workload_id);
    assert_eq!(payload.node_id, fixture.node_id);
    assert_eq!(payload.active_certificate_id, certificate.id);
    assert_eq!(payload.active_certificate_expires_at, expires_at);
    assert_eq!(
        payload.risk_window_seconds,
        GATEWAY_CERTIFICATE_EXPIRY_RISK_WINDOW_SECONDS
    );
    assert_eq!(payload.state, GatewayCertificateExpiryRiskState::AtRisk);
    let serialized = facts[0].payload.to_string();
    for private_fragment in [
        "BEGIN CERTIFICATE",
        "dGVzdA==",
        "ca_bundle",
        "credential",
        "command_id",
        "failure",
    ] {
        assert!(!serialized.contains(private_fragment));
    }

    let replay = reconciler
        .run_once(scan_at + Duration::seconds(1))
        .await
        .expect("replay emergency scan");
    assert_eq!(replay.expiry_risk_targets, 0);
    assert_eq!(replay.projected_expiry_risks, 0);
    assert_eq!(
        expiry_risk_facts(fixture.repository.as_ref()).await.len(),
        1
    );
}

#[tokio::test]
async fn applied_short_lived_replacement_refreshes_risk_and_safe_replacement_clears_it() {
    let fixture = Fixture::new();
    let base = canonical_timestamp(Utc::now());
    let claim = fixture
        .verified_claim("risk-replacement.example.com", base)
        .await;
    let scan_at = base + Duration::hours(2);
    let (route, previous) = fixture
        .activate_route(
            &claim,
            "risk-replacement.example.com",
            base + Duration::seconds(1),
            scan_at + Duration::hours(24),
        )
        .await;
    let queue = Arc::new(RecordingGatewayQueue::default());
    let authority = Arc::new(RecordingGatewayCertificateAuthority::default());
    let reconciler = reconciler(&fixture, queue, authority);
    reconciler
        .run_once(scan_at - Duration::minutes(1))
        .await
        .expect("stage first renewal");
    reconciler
        .run_once(scan_at)
        .await
        .expect("project first risk");
    let first = fixture
        .repository
        .pending_gateway_certificate_convergences(10)
        .await
        .expect("first pending renewal")
        .pop()
        .expect("first renewal");
    let short_lived = first.certificate.as_ref().expect("short-lived replacement");
    let short_applied_at = scan_at + Duration::seconds(1);
    let short_expires_at = short_applied_at + Duration::hours(12);
    issue_certificate(
        fixture.repository.as_ref(),
        short_lived,
        scan_at + Duration::milliseconds(100),
        short_expires_at,
    )
    .await;
    apply_convergence(
        fixture.repository.as_ref(),
        &first.publication,
        short_applied_at,
    )
    .await;

    let refreshed = fixture
        .repository
        .find_gateway_certificate_expiry_risk(route.id, fixture.node_id)
        .await
        .expect("refreshed risk lookup")
        .expect("refreshed risk");
    assert_eq!(refreshed.state, GatewayCertificateExpiryRiskState::AtRisk);
    assert_eq!(refreshed.active_certificate_id, short_lived.id);
    assert_eq!(refreshed.active_certificate_expires_at, short_expires_at);
    assert_eq!(refreshed.generation, 2);
    assert_ne!(refreshed.active_certificate_id, previous.id);

    let next_at = short_applied_at + Duration::seconds(1);
    let next = reconciler
        .run_once(next_at)
        .await
        .expect("stage safe replacement");
    assert_eq!(next.expiry_risk_targets, 0);
    assert_eq!(next.staged_convergences, 1);
    let safe = fixture
        .repository
        .pending_gateway_certificate_convergences(10)
        .await
        .expect("safe pending renewal")
        .pop()
        .expect("safe renewal");
    let safe_certificate = safe.certificate.as_ref().expect("safe replacement");
    let safe_applied_at = next_at + Duration::seconds(1);
    let safe_expires_at = safe_applied_at + Duration::days(30);
    issue_certificate(
        fixture.repository.as_ref(),
        safe_certificate,
        next_at + Duration::milliseconds(100),
        safe_expires_at,
    )
    .await;
    let applied = acknowledgement(
        &safe.publication,
        GatewayAckState::Applied,
        safe_applied_at,
        None,
    );
    fixture
        .repository
        .project_gateway_acknowledgement(&applied, safe_applied_at + Duration::milliseconds(1))
        .await
        .expect("apply safe replacement");

    let clear = fixture
        .repository
        .find_gateway_certificate_expiry_risk(route.id, fixture.node_id)
        .await
        .expect("clear risk lookup")
        .expect("clear risk");
    assert_eq!(clear.state, GatewayCertificateExpiryRiskState::Clear);
    assert_eq!(clear.active_certificate_id, safe_certificate.id);
    assert_eq!(clear.active_certificate_expires_at, safe_expires_at);
    assert_eq!(clear.generation, 3);
    assert_eq!(clear.previous_at_risk_certificate_id, Some(short_lived.id));
    assert_eq!(
        clear.previous_at_risk_certificate_expires_at,
        Some(short_expires_at)
    );

    fixture
        .repository
        .project_gateway_acknowledgement(&applied, safe_applied_at + Duration::milliseconds(1))
        .await
        .expect("replay safe replacement");
    let facts = expiry_risk_facts(fixture.repository.as_ref()).await;
    assert_eq!(facts.len(), 3);
    assert_eq!(
        facts
            .iter()
            .map(|fact| fact.event_key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "edge.gateway-certificate.expiry-at-risk",
            "edge.gateway-certificate.expiry-at-risk",
            "edge.gateway-certificate.expiry-risk-cleared",
        ]
    );
    assert_eq!(
        facts
            .iter()
            .map(|fact| fact.aggregate_version)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
}

#[tokio::test]
async fn rejected_and_unavailable_replacements_cannot_clear_an_existing_risk() {
    let fixture = Fixture::new();
    let base = canonical_timestamp(Utc::now());
    let claim = fixture
        .verified_claim("risk-terminal.example.com", base)
        .await;
    let scan_at = base + Duration::hours(2);
    let (route, _) = fixture
        .activate_route(
            &claim,
            "risk-terminal.example.com",
            base + Duration::seconds(1),
            scan_at + Duration::hours(24),
        )
        .await;
    let queue = Arc::new(RecordingGatewayQueue::default());
    let authority = Arc::new(RecordingGatewayCertificateAuthority::default());
    let reconciler = reconciler(&fixture, queue, authority);
    reconciler
        .run_once(scan_at - Duration::minutes(1))
        .await
        .expect("stage rejected renewal");
    reconciler
        .run_once(scan_at)
        .await
        .expect("project risk before rejection");
    let rejected = fixture
        .repository
        .pending_gateway_certificate_convergences(10)
        .await
        .expect("rejected pending renewal")
        .pop()
        .expect("rejected renewal");
    let rejected_certificate = rejected.certificate.as_ref().expect("rejected replacement");
    issue_certificate(
        fixture.repository.as_ref(),
        rejected_certificate,
        scan_at + Duration::milliseconds(100),
        scan_at + Duration::days(30),
    )
    .await;
    let rejected_at = scan_at + Duration::seconds(1);
    let rejected_ack = acknowledgement(
        &rejected.publication,
        GatewayAckState::Rejected,
        rejected_at,
        Some("provider token=must-not-clear"),
    );
    fixture
        .repository
        .project_gateway_acknowledgement(&rejected_ack, rejected_at + Duration::milliseconds(1))
        .await
        .expect("reject replacement");

    let retry_at = scan_at + Duration::seconds(2);
    reconciler
        .run_once(retry_at)
        .await
        .expect("stage unavailable retry");
    let unavailable = fixture
        .repository
        .pending_gateway_certificate_convergences(10)
        .await
        .expect("unavailable pending renewal")
        .pop()
        .expect("unavailable renewal");
    fixture
        .repository
        .mark_gateway_certificate_convergence_unavailable(
            fixture.organization_id,
            fixture.node_id,
            unavailable.publication.revision,
            unavailable.publication.command_id,
            "provider credential=must-not-clear",
            unavailable.publication.command_not_after,
        )
        .await
        .expect("mark replacement unavailable");

    let risk = fixture
        .repository
        .find_gateway_certificate_expiry_risk(route.id, fixture.node_id)
        .await
        .expect("retained risk lookup")
        .expect("retained risk");
    assert_eq!(risk.state, GatewayCertificateExpiryRiskState::AtRisk);
    assert_eq!(risk.generation, 1);
    let facts = expiry_risk_facts(fixture.repository.as_ref()).await;
    assert_eq!(facts.len(), 1);
    assert_eq!(
        facts[0].event_key,
        "edge.gateway-certificate.expiry-at-risk"
    );
    assert!(!facts[0].payload.to_string().contains("must-not-clear"));
}
