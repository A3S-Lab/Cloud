use super::*;

#[allow(clippy::too_many_arguments)]
fn gateway_certificate_renewal_message(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    route_id: RouteId,
    node_id: NodeId,
    event_key: &str,
    status: GatewayCertificateRenewalStatus,
    failure_kind: Option<GatewayCertificateRenewalFailureKind>,
    aggregate_version: u64,
    occurred_at: DateTime<Utc>,
) -> OutboxMessage {
    let previous_certificate_id = GatewayCertificateId::new();
    let replacement_certificate_id = GatewayCertificateId::new();
    let active_certificate_id = match status {
        GatewayCertificateRenewalStatus::Failed => previous_certificate_id,
        GatewayCertificateRenewalStatus::Renewed => replacement_certificate_id,
    };
    OutboxMessage {
        event_id: Uuid::now_v7(),
        event_key: event_key.into(),
        schema_version: 1,
        scope: crate::modules::shared_kernel::domain::ScopeContext::organization(
            crate::modules::shared_kernel::domain::InstallationId::new(),
            crate::modules::shared_kernel::domain::OrganizationId::from_uuid(
                organization_id.as_uuid(),
            ),
        )
        .expect("scope"),
        aggregate_id: renewal_subject_id(route_id, node_id),
        aggregate_version,
        occurred_at,
        correlation_id: Uuid::now_v7(),
        causation_id: None,
        payload: serde_json::to_value(GatewayCertificateRenewalChanged {
            organization_id,
            project_id,
            environment_id,
            route_id,
            workload_id: WorkloadId::new(),
            node_id,
            hostname: "managed-tls.example.com".into(),
            path_prefix: "/service".into(),
            gateway_revision: aggregate_version,
            previous_certificate_id,
            replacement_certificate_id,
            active_certificate_id,
            active_certificate_expires_at: canonical_timestamp(
                occurred_at + chrono::Duration::days(30),
            ),
            status,
            failure_kind,
        })
        .expect("Gateway certificate renewal payload"),
        delivery_attempts: 1,
    }
}

#[tokio::test]
async fn gateway_certificate_failures_and_recovery_are_node_local_projections() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let recipient = PrincipalId::new();
    let membership_id = MembershipId::new();
    let route_id = RouteId::new();
    let node_id = NodeId::new();
    let peer_node_id = NodeId::new();
    let created_at = Utc::now();
    let notifications = Arc::new(InMemoryNotificationRepository::new());
    create_alert_policy_for_source(
        notifications.as_ref(),
        organization_id,
        recipient,
        NotificationAlertSource::EdgeGatewayCertificateRenewalStatusV1,
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

    let historical_failure = gateway_certificate_renewal_message(
        organization_id,
        project_id,
        environment_id,
        route_id,
        node_id,
        "edge.gateway-certificate.renewal-failed",
        GatewayCertificateRenewalStatus::Failed,
        Some(GatewayCertificateRenewalFailureKind::Rejected),
        1,
        created_at - chrono::Duration::seconds(1),
    );
    projector
        .project(&historical_failure)
        .await
        .expect("pre-policy renewal failure is silent");

    let initial_success = gateway_certificate_renewal_message(
        organization_id,
        project_id,
        environment_id,
        route_id,
        node_id,
        "edge.gateway-certificate.renewed",
        GatewayCertificateRenewalStatus::Renewed,
        None,
        1,
        created_at + chrono::Duration::seconds(1),
    );
    projector
        .project(&initial_success)
        .await
        .expect("routine initial renewal is silent");

    let rejected = gateway_certificate_renewal_message(
        organization_id,
        project_id,
        environment_id,
        route_id,
        node_id,
        "edge.gateway-certificate.renewal-failed",
        GatewayCertificateRenewalStatus::Failed,
        Some(GatewayCertificateRenewalFailureKind::Rejected),
        2,
        created_at + chrono::Duration::seconds(2),
    );
    projector
        .project(&rejected)
        .await
        .expect("project rejected renewal");
    projector
        .project(&rejected)
        .await
        .expect("replay rejected renewal");

    let peer_success = gateway_certificate_renewal_message(
        organization_id,
        project_id,
        environment_id,
        route_id,
        peer_node_id,
        "edge.gateway-certificate.renewed",
        GatewayCertificateRenewalStatus::Renewed,
        None,
        3,
        created_at + chrono::Duration::seconds(3),
    );
    projector
        .project(&peer_success)
        .await
        .expect("peer renewal cannot recover another node");

    let unavailable = gateway_certificate_renewal_message(
        organization_id,
        project_id,
        environment_id,
        route_id,
        node_id,
        "edge.gateway-certificate.renewal-failed",
        GatewayCertificateRenewalStatus::Failed,
        Some(GatewayCertificateRenewalFailureKind::Unavailable),
        3,
        created_at + chrono::Duration::seconds(4),
    );
    projector
        .project(&unavailable)
        .await
        .expect("project unavailable renewal");

    let recovered = gateway_certificate_renewal_message(
        organization_id,
        project_id,
        environment_id,
        route_id,
        node_id,
        "edge.gateway-certificate.renewed",
        GatewayCertificateRenewalStatus::Renewed,
        None,
        4,
        created_at + chrono::Duration::seconds(5),
    );
    projector
        .project(&recovered)
        .await
        .expect("project covered recovery");
    projector
        .project(&recovered)
        .await
        .expect("replay covered recovery");

    let routine_success = gateway_certificate_renewal_message(
        organization_id,
        project_id,
        environment_id,
        route_id,
        node_id,
        "edge.gateway-certificate.renewed",
        GatewayCertificateRenewalStatus::Renewed,
        None,
        5,
        created_at + chrono::Duration::seconds(6),
    );
    projector
        .project(&routine_success)
        .await
        .expect("success after recovery is silent");

    let projected = notifications
        .list_page(organization_id, recipient, false, None, 50)
        .await
        .expect("certificate notifications");
    assert_eq!(projected.len(), 3);
    assert_eq!(
        projected
            .iter()
            .map(|notification| (
                notification.source_event_key.as_str(),
                notification.severity
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "edge.gateway-certificate.renewed",
                NotificationSeverity::Information
            ),
            (
                "edge.gateway-certificate.renewal-failed",
                NotificationSeverity::Critical
            ),
            (
                "edge.gateway-certificate.renewal-failed",
                NotificationSeverity::Warning
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
async fn gateway_certificate_recovery_respects_policy_opt_out() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let recipient = PrincipalId::new();
    let membership_id = MembershipId::new();
    let route_id = RouteId::new();
    let node_id = NodeId::new();
    let created_at = Utc::now();
    let notifications = Arc::new(InMemoryNotificationRepository::new());
    create_alert_policy_for_source(
        notifications.as_ref(),
        organization_id,
        recipient,
        NotificationAlertSource::EdgeGatewayCertificateRenewalStatusV1,
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

    projector
        .project(&gateway_certificate_renewal_message(
            organization_id,
            project_id,
            environment_id,
            route_id,
            node_id,
            "edge.gateway-certificate.renewal-failed",
            GatewayCertificateRenewalStatus::Failed,
            Some(GatewayCertificateRenewalFailureKind::Rejected),
            1,
            created_at + chrono::Duration::seconds(1),
        ))
        .await
        .expect("project certificate failure");
    projector
        .project(&gateway_certificate_renewal_message(
            organization_id,
            project_id,
            environment_id,
            route_id,
            node_id,
            "edge.gateway-certificate.renewed",
            GatewayCertificateRenewalStatus::Renewed,
            None,
            2,
            created_at + chrono::Duration::seconds(2),
        ))
        .await
        .expect("recovery opt-out is silent");

    let projected = notifications
        .list_page(organization_id, recipient, false, None, 50)
        .await
        .expect("certificate notifications");
    assert_eq!(projected.len(), 1);
    assert_eq!(
        projected[0].source_event_key,
        "edge.gateway-certificate.renewal-failed"
    );
    assert_eq!(projected[0].severity, NotificationSeverity::Warning);
}

#[tokio::test]
async fn gateway_certificate_alerts_recheck_resource_grants() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let recipient = PrincipalId::new();
    let membership_id = MembershipId::new();
    let created_at = Utc::now();
    let notifications = Arc::new(InMemoryNotificationRepository::new());
    create_alert_policy_for_source(
        notifications.as_ref(),
        organization_id,
        recipient,
        NotificationAlertSource::EdgeGatewayCertificateRenewalStatusV1,
        project_id,
        environment_id,
        true,
        created_at,
    )
    .await;
    let failure = gateway_certificate_renewal_message(
        organization_id,
        project_id,
        environment_id,
        RouteId::new(),
        NodeId::new(),
        "edge.gateway-certificate.renewal-failed",
        GatewayCertificateRenewalStatus::Failed,
        Some(GatewayCertificateRenewalFailureKind::Rejected),
        1,
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
        .project(&failure)
        .await
        .expect("missing grant is ignored");
    assert!(notifications
        .list_page(organization_id, recipient, false, None, 50)
        .await
        .expect("notifications")
        .is_empty());

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
    authorized
        .project(&failure)
        .await
        .expect("matching grant projects alert");

    let projected = notifications
        .list_page(organization_id, recipient, false, None, 50)
        .await
        .expect("notifications");
    assert_eq!(projected.len(), 1);
    assert_eq!(
        projected[0].source_event_key,
        "edge.gateway-certificate.renewal-failed"
    );
}

#[test]
fn malformed_gateway_certificate_renewal_payloads_fail_closed() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let route_id = RouteId::new();
    let node_id = NodeId::new();
    let occurred_at = Utc::now();
    let message = gateway_certificate_renewal_message(
        organization_id,
        project_id,
        environment_id,
        route_id,
        node_id,
        "edge.gateway-certificate.renewal-failed",
        GatewayCertificateRenewalStatus::Failed,
        Some(GatewayCertificateRenewalFailureKind::Rejected),
        2,
        occurred_at,
    );
    assert!(decode_gateway_certificate_renewal(&message).is_ok());

    let mut unexpected = message.clone();
    unexpected.payload["providerPrivateFailure"] = serde_json::json!("secret");
    assert!(decode_gateway_certificate_renewal(&unexpected).is_err());

    let mut wrong_subject = message.clone();
    wrong_subject.aggregate_id = Uuid::now_v7();
    assert!(decode_gateway_certificate_renewal(&wrong_subject).is_err());

    let mut wrong_revision = message.clone();
    wrong_revision.payload["gateway_revision"] = serde_json::json!(3);
    assert!(decode_gateway_certificate_renewal(&wrong_revision).is_err());

    let mut wrong_status = message.clone();
    wrong_status.payload["status"] = serde_json::json!("renewed");
    wrong_status.payload["failure_kind"] = serde_json::Value::Null;
    assert!(decode_gateway_certificate_renewal(&wrong_status).is_err());

    let mut wrong_active_certificate = message;
    wrong_active_certificate.payload["active_certificate_id"] =
        wrong_active_certificate.payload["replacement_certificate_id"].clone();
    assert!(decode_gateway_certificate_renewal(&wrong_active_certificate).is_err());
}
