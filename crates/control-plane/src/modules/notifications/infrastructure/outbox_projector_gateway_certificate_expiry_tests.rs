use super::*;
use crate::modules::edge::domain::events::{
    certificate_expiry_aggregate_version, renewal_subject_id, GatewayCertificateExpiryChanged,
    GatewayCertificateExpiryStatus,
};

#[derive(Clone)]
struct GatewayCertificateExpiryFixture {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    route_id: RouteId,
    workload_id: WorkloadId,
    node_id: NodeId,
    previous_certificate_id: GatewayCertificateId,
    replacement_certificate_id: GatewayCertificateId,
}

impl GatewayCertificateExpiryFixture {
    fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        route_id: RouteId,
        node_id: NodeId,
    ) -> Self {
        Self {
            organization_id,
            project_id,
            environment_id,
            route_id,
            workload_id: WorkloadId::new(),
            node_id,
            previous_certificate_id: GatewayCertificateId::new(),
            replacement_certificate_id: GatewayCertificateId::new(),
        }
    }

    fn message(
        &self,
        status: GatewayCertificateExpiryStatus,
        certificate_gateway_revision: u64,
        renewal_gateway_revision: u64,
        occurred_at: DateTime<Utc>,
    ) -> OutboxMessage {
        let event_key = match status {
            GatewayCertificateExpiryStatus::Expiring => "edge.gateway-certificate.expiring",
            GatewayCertificateExpiryStatus::Resolved => "edge.gateway-certificate.expiry-resolved",
        };
        let active_certificate_id = match status {
            GatewayCertificateExpiryStatus::Expiring => self.previous_certificate_id,
            GatewayCertificateExpiryStatus::Resolved => self.replacement_certificate_id,
        };
        let occurred_at = canonical_timestamp(occurred_at);
        let aggregate_id = renewal_subject_id(self.route_id, self.node_id);
        OutboxMessage {
            event_id: GatewayCertificateExpiryChanged::deterministic_event_id(
                aggregate_id,
                event_key,
                active_certificate_id,
            ),
            event_key: event_key.into(),
            schema_version: 1,
            organization_id: self.organization_id.as_uuid(),
            aggregate_id,
            aggregate_version: certificate_expiry_aggregate_version(
                certificate_gateway_revision,
                status,
            )
            .expect("certificate expiry phase"),
            occurred_at,
            correlation_id: Uuid::now_v7(),
            causation_id: None,
            payload: serde_json::to_value(GatewayCertificateExpiryChanged {
                organization_id: self.organization_id,
                project_id: self.project_id,
                environment_id: self.environment_id,
                route_id: self.route_id,
                workload_id: self.workload_id,
                node_id: self.node_id,
                hostname: "managed-tls.example.com".into(),
                path_prefix: "/service".into(),
                certificate_gateway_revision,
                renewal_gateway_revision,
                previous_certificate_id: self.previous_certificate_id,
                replacement_certificate_id: self.replacement_certificate_id,
                active_certificate_id,
                active_certificate_expires_at: canonical_timestamp(
                    occurred_at + chrono::Duration::days(30),
                ),
                status,
            })
            .expect("Gateway certificate expiry payload"),
            delivery_attempts: 1,
        }
    }
}

#[tokio::test]
async fn gateway_certificate_expiry_firing_and_recovery_are_node_local_projections() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let recipient = PrincipalId::new();
    let membership_id = MembershipId::new();
    let route_id = RouteId::new();
    let node_id = NodeId::new();
    let created_at = canonical_timestamp(Utc::now());
    let notifications = Arc::new(InMemoryNotificationRepository::new());
    create_alert_policy_for_source(
        notifications.as_ref(),
        organization_id,
        recipient,
        NotificationAlertSource::EdgeGatewayCertificateExpiryStatusV1,
        project_id,
        environment_id,
        true,
        created_at,
    )
    .await;
    let projector = OutboxNotificationProjector::new(
        notifications.clone(),
        membership_lookup(organization_id, membership_id, recipient, created_at),
    )
    .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));
    let lifecycle = GatewayCertificateExpiryFixture::new(
        organization_id,
        project_id,
        environment_id,
        route_id,
        node_id,
    );

    projector
        .project(&lifecycle.message(
            GatewayCertificateExpiryStatus::Expiring,
            1,
            3,
            created_at - chrono::Duration::seconds(1),
        ))
        .await
        .expect("pre-policy firing is silent");
    projector
        .project(&lifecycle.message(
            GatewayCertificateExpiryStatus::Resolved,
            3,
            3,
            created_at + chrono::Duration::seconds(1),
        ))
        .await
        .expect("initial resolution is silent");

    let firing = lifecycle.message(
        GatewayCertificateExpiryStatus::Expiring,
        3,
        5,
        created_at + chrono::Duration::seconds(2),
    );
    projector.project(&firing).await.expect("project firing");
    projector.project(&firing).await.expect("replay firing");

    let peer = GatewayCertificateExpiryFixture::new(
        organization_id,
        project_id,
        environment_id,
        route_id,
        NodeId::new(),
    );
    projector
        .project(&peer.message(
            GatewayCertificateExpiryStatus::Resolved,
            5,
            5,
            created_at + chrono::Duration::seconds(3),
        ))
        .await
        .expect("peer node cannot resolve this firing");

    let resolved = lifecycle.message(
        GatewayCertificateExpiryStatus::Resolved,
        5,
        5,
        created_at + chrono::Duration::seconds(4),
    );
    projector
        .project(&resolved)
        .await
        .expect("project resolution");
    projector
        .project(&resolved)
        .await
        .expect("replay resolution");

    let mut next_lifecycle = lifecycle.clone();
    next_lifecycle.previous_certificate_id = lifecycle.replacement_certificate_id;
    next_lifecycle.replacement_certificate_id = GatewayCertificateId::new();
    let next_firing = next_lifecycle.message(
        GatewayCertificateExpiryStatus::Expiring,
        5,
        7,
        created_at + chrono::Duration::seconds(5),
    );
    projector
        .project(&next_firing)
        .await
        .expect("later certificate may fire again");

    let projected = notifications
        .list_page(organization_id, recipient, false, None, 50)
        .await
        .expect("certificate expiry notifications");
    assert_eq!(projected.len(), 3);
    assert_eq!(
        projected
            .iter()
            .map(|notification| (
                notification.source_event_key.as_str(),
                notification.severity,
                notification.source_aggregate_version,
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "edge.gateway-certificate.expiring",
                NotificationSeverity::Warning,
                10,
            ),
            (
                "edge.gateway-certificate.expiry-resolved",
                NotificationSeverity::Information,
                9,
            ),
            (
                "edge.gateway-certificate.expiring",
                NotificationSeverity::Warning,
                6,
            ),
        ]
    );
    assert!(projected.iter().all(|notification| notification.scope
        == NotificationScope::Environment {
            project_id,
            environment_id,
        }));
    assert!(projected.iter().all(
        |notification| notification.body.contains(&route_id.to_string())
            && notification.body.contains(&node_id.to_string())
            && !notification.body.contains("private")
    ));
}

#[tokio::test]
async fn gateway_certificate_expiry_recovery_requires_opt_in_and_active_policy() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let recipient = PrincipalId::new();
    let membership_id = MembershipId::new();
    let created_at = canonical_timestamp(Utc::now());
    let notifications = Arc::new(InMemoryNotificationRepository::new());
    let policy = create_alert_policy_for_source(
        notifications.as_ref(),
        organization_id,
        recipient,
        NotificationAlertSource::EdgeGatewayCertificateExpiryStatusV1,
        project_id,
        environment_id,
        false,
        created_at,
    )
    .await;
    let projector = OutboxNotificationProjector::new(
        notifications.clone(),
        membership_lookup(organization_id, membership_id, recipient, created_at),
    )
    .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));
    let lifecycle = GatewayCertificateExpiryFixture::new(
        organization_id,
        project_id,
        environment_id,
        RouteId::new(),
        NodeId::new(),
    );

    projector
        .project(&lifecycle.message(
            GatewayCertificateExpiryStatus::Expiring,
            1,
            3,
            created_at + chrono::Duration::seconds(1),
        ))
        .await
        .expect("firing projects despite recovery opt-out");
    projector
        .project(&lifecycle.message(
            GatewayCertificateExpiryStatus::Resolved,
            3,
            3,
            created_at + chrono::Duration::seconds(2),
        ))
        .await
        .expect("recovery opt-out is silent");
    revoke_alert_policy(
        notifications.as_ref(),
        &policy,
        created_at + chrono::Duration::seconds(3),
    )
    .await;
    projector
        .project(&lifecycle.message(
            GatewayCertificateExpiryStatus::Expiring,
            3,
            5,
            created_at + chrono::Duration::seconds(4),
        ))
        .await
        .expect("revoked policy remains silent");

    let projected = notifications
        .list_page(organization_id, recipient, false, None, 50)
        .await
        .expect("certificate expiry notifications");
    assert_eq!(projected.len(), 1);
    assert_eq!(
        projected[0].source_event_key,
        "edge.gateway-certificate.expiring"
    );
}

#[tokio::test]
async fn gateway_certificate_expiry_alerts_recheck_grants_and_ignore_schema_drift() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let recipient = PrincipalId::new();
    let membership_id = MembershipId::new();
    let created_at = canonical_timestamp(Utc::now());
    let notifications = Arc::new(InMemoryNotificationRepository::new());
    create_alert_policy_for_source(
        notifications.as_ref(),
        organization_id,
        recipient,
        NotificationAlertSource::EdgeGatewayCertificateExpiryStatusV1,
        project_id,
        environment_id,
        true,
        created_at,
    )
    .await;
    let lifecycle = GatewayCertificateExpiryFixture::new(
        organization_id,
        project_id,
        environment_id,
        RouteId::new(),
        NodeId::new(),
    );
    let firing = lifecycle.message(
        GatewayCertificateExpiryStatus::Expiring,
        1,
        3,
        created_at + chrono::Duration::seconds(1),
    );
    let membership = || {
        membership_lookup_with_role(
            organization_id,
            membership_id,
            recipient,
            MembershipRole::Restricted,
            true,
            created_at,
        )
    };

    let unauthorized = OutboxNotificationProjector::new(notifications.clone(), membership())
        .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));
    unauthorized
        .project(&firing)
        .await
        .expect("missing grant is ignored");

    let grant = ResourceGrant::create(
        ResourceGrantId::new(),
        organization_id,
        membership_id,
        ResourceGrantScope::Environment {
            project_id,
            environment_id,
        },
        created_at,
    );
    let authorized = OutboxNotificationProjector::new(notifications.clone(), membership())
        .with_alert_policies(notifications.clone(), resource_grants(vec![grant]));
    let mut schema_drift = firing.clone();
    schema_drift.schema_version = 2;
    authorized
        .project(&schema_drift)
        .await
        .expect("schema drift is ignored");
    authorized
        .project(&firing)
        .await
        .expect("matching grant projects alert");

    let projected = notifications
        .list_page(organization_id, recipient, false, None, 50)
        .await
        .expect("certificate expiry notifications");
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].severity, NotificationSeverity::Warning);
}

#[test]
fn malformed_gateway_certificate_expiry_payloads_fail_closed() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let occurred_at = canonical_timestamp(Utc::now());
    let lifecycle = GatewayCertificateExpiryFixture::new(
        organization_id,
        project_id,
        environment_id,
        RouteId::new(),
        NodeId::new(),
    );
    let message = lifecycle.message(GatewayCertificateExpiryStatus::Expiring, 3, 5, occurred_at);
    assert!(decode_gateway_certificate_expiry(&message).is_ok());

    let mut unexpected = message.clone();
    unexpected.payload["provider_private_failure"] = serde_json::json!("secret");
    assert!(decode_gateway_certificate_expiry(&unexpected).is_err());

    let mut wrong_subject = message.clone();
    wrong_subject.aggregate_id = Uuid::now_v7();
    assert!(decode_gateway_certificate_expiry(&wrong_subject).is_err());

    let mut wrong_phase = message.clone();
    wrong_phase.aggregate_version += 1;
    assert!(decode_gateway_certificate_expiry(&wrong_phase).is_err());

    let mut wrong_status = message.clone();
    wrong_status.payload["status"] = serde_json::json!("resolved");
    assert!(decode_gateway_certificate_expiry(&wrong_status).is_err());

    let mut wrong_active_certificate = message.clone();
    wrong_active_certificate.payload["active_certificate_id"] =
        wrong_active_certificate.payload["replacement_certificate_id"].clone();
    assert!(decode_gateway_certificate_expiry(&wrong_active_certificate).is_err());

    let mut wrong_revisions = message.clone();
    wrong_revisions.payload["renewal_gateway_revision"] = serde_json::json!(3);
    assert!(decode_gateway_certificate_expiry(&wrong_revisions).is_err());

    let mut noncanonical_hostname = message.clone();
    noncanonical_hostname.payload["hostname"] = serde_json::json!("Managed-TLS.example.com");
    assert!(decode_gateway_certificate_expiry(&noncanonical_hostname).is_err());

    let mut noncanonical_path = message.clone();
    noncanonical_path.payload["path_prefix"] = serde_json::json!("/service//private");
    assert!(decode_gateway_certificate_expiry(&noncanonical_path).is_err());

    let mut noncanonical_expiry = message.clone();
    noncanonical_expiry.payload["active_certificate_expires_at"] =
        serde_json::json!("2026-08-22T00:00:00.123456789Z");
    assert!(decode_gateway_certificate_expiry(&noncanonical_expiry).is_err());

    let mut nil_workload = message.clone();
    nil_workload.payload["workload_id"] = serde_json::json!(Uuid::nil());
    assert!(decode_gateway_certificate_expiry(&nil_workload).is_err());

    let mut nil_event = message.clone();
    nil_event.event_id = Uuid::nil();
    assert!(decode_gateway_certificate_expiry(&nil_event).is_err());

    let mut wrong_event_id = message.clone();
    wrong_event_id.event_id = Uuid::now_v7();
    assert!(decode_gateway_certificate_expiry(&wrong_event_id).is_err());

    let mut nil_correlation = message.clone();
    nil_correlation.correlation_id = Uuid::nil();
    assert!(decode_gateway_certificate_expiry(&nil_correlation).is_err());

    let mut caused = message.clone();
    caused.causation_id = Some(Uuid::now_v7());
    assert!(decode_gateway_certificate_expiry(&caused).is_err());

    let resolved = lifecycle.message(GatewayCertificateExpiryStatus::Resolved, 5, 5, occurred_at);
    assert!(decode_gateway_certificate_expiry(&resolved).is_ok());

    let mut resolved_with_old_active = resolved;
    resolved_with_old_active.payload["active_certificate_id"] =
        resolved_with_old_active.payload["previous_certificate_id"].clone();
    assert!(decode_gateway_certificate_expiry(&resolved_with_old_active).is_err());
}
