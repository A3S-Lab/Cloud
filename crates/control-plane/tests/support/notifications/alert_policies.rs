use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn exercise_notification_alert_policy_persistence(
    executor: PostgresExecutor,
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    recipient: PrincipalId,
    created_at: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let membership_id = MembershipId::new();
    database
        .execute(
            sql_query::<()>("insert into organization_memberships (id, organization_id, principal_id, role, aggregate_version, created_at, updated_at, revoked_at) values (")
                .bind(membership_id.as_uuid())
                .append(", ")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(recipient.as_uuid())
                .append(", 'member', 1, ")
                .bind(created_at)
                .append(", ")
                .bind(created_at)
                .append(", null)"),
        )
        .await?;

    let repository = Arc::new(PostgresNotificationRepository::new(executor.clone()));
    let definition = NotificationAlertPolicyDefinition::from_spec(NotificationAlertPolicySpec {
        source: NotificationAlertSource::EdgeDomainClaimStatusV1,
        project_id,
        environment_id,
        notify_on_recovery: true,
    })?;
    let policy = NotificationAlertPolicy::create(
        organization_id,
        NotificationAlertPolicyId::new(),
        recipient,
        definition.clone(),
        recipient,
        created_at,
    )?;
    let create_write = notification_alert_policy_create_write(&policy, "postgres:alert:create")?;
    let created = repository.create_alert_policy(create_write.clone()).await?;
    assert!(!created.replayed);
    assert_eq!(created.value, policy);
    let replayed = repository.create_alert_policy(create_write).await?;
    assert!(replayed.replayed);
    assert_eq!(replayed.value, policy);
    assert_eq!(
        repository
            .find_alert_policy(organization_id, recipient, policy.id)
            .await?,
        Some(policy.clone())
    );
    assert_eq!(
        repository
            .list_alert_policy_page(organization_id, recipient, None, 50)
            .await?,
        vec![policy.clone()]
    );
    assert_eq!(
        repository
            .list_active_alert_policies_for_source(
                organization_id,
                NotificationAlertSource::EdgeDomainClaimStatusV1,
                project_id,
                environment_id,
                policy.created_at,
            )
            .await?,
        vec![policy.clone()]
    );

    let duplicate = NotificationAlertPolicy::create(
        organization_id,
        NotificationAlertPolicyId::new(),
        recipient,
        definition,
        recipient,
        policy.created_at,
    )?;
    assert!(matches!(
        repository
            .create_alert_policy(notification_alert_policy_create_write(
                &duplicate,
                "postgres:alert:duplicate",
            )?)
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let certificate_definition =
        NotificationAlertPolicyDefinition::from_spec(NotificationAlertPolicySpec {
            source: NotificationAlertSource::EdgeGatewayCertificateRenewalStatusV1,
            project_id,
            environment_id,
            notify_on_recovery: true,
        })?;
    let certificate_policy = NotificationAlertPolicy::create(
        organization_id,
        NotificationAlertPolicyId::new(),
        recipient,
        certificate_definition,
        recipient,
        policy.created_at + ChronoDuration::milliseconds(1),
    )?;
    let certificate_create_write = notification_alert_policy_create_write(
        &certificate_policy,
        "postgres:certificate-alert:create",
    )?;
    let certificate_created = repository
        .create_alert_policy(certificate_create_write.clone())
        .await?;
    assert!(!certificate_created.replayed);
    assert_eq!(certificate_created.value, certificate_policy);
    assert!(
        repository
            .create_alert_policy(certificate_create_write)
            .await?
            .replayed
    );
    assert_eq!(
        repository
            .list_active_alert_policies_for_source(
                organization_id,
                NotificationAlertSource::EdgeGatewayCertificateRenewalStatusV1,
                project_id,
                environment_id,
                certificate_policy.created_at,
            )
            .await?,
        vec![certificate_policy.clone()]
    );

    let workload_definition =
        NotificationAlertPolicyDefinition::from_spec(NotificationAlertPolicySpec {
            source: NotificationAlertSource::WorkloadDeploymentHealthV1,
            project_id,
            environment_id,
            notify_on_recovery: true,
        })?;
    let workload_policy = NotificationAlertPolicy::create(
        organization_id,
        NotificationAlertPolicyId::new(),
        recipient,
        workload_definition,
        recipient,
        policy.created_at + ChronoDuration::milliseconds(2),
    )?;
    let workload_create_write =
        notification_alert_policy_create_write(&workload_policy, "postgres:workload-alert:create")?;
    let workload_created = repository
        .create_alert_policy(workload_create_write.clone())
        .await?;
    assert!(!workload_created.replayed);
    assert_eq!(workload_created.value, workload_policy);
    assert!(
        repository
            .create_alert_policy(workload_create_write)
            .await?
            .replayed
    );
    assert_eq!(
        repository
            .list_active_alert_policies_for_source(
                organization_id,
                NotificationAlertSource::WorkloadDeploymentHealthV1,
                project_id,
                environment_id,
                workload_policy.created_at,
            )
            .await?,
        vec![workload_policy.clone()]
    );

    let certificate_expiry_definition =
        NotificationAlertPolicyDefinition::from_spec(NotificationAlertPolicySpec {
            source: NotificationAlertSource::EdgeGatewayCertificateExpiryStatusV1,
            project_id,
            environment_id,
            notify_on_recovery: true,
        })?;
    let certificate_expiry_policy = NotificationAlertPolicy::create(
        organization_id,
        NotificationAlertPolicyId::new(),
        recipient,
        certificate_expiry_definition,
        recipient,
        policy.created_at + ChronoDuration::milliseconds(3),
    )?;
    let certificate_expiry_create_write = notification_alert_policy_create_write(
        &certificate_expiry_policy,
        "postgres:certificate-expiry-alert:create",
    )?;
    let certificate_expiry_created = repository
        .create_alert_policy(certificate_expiry_create_write.clone())
        .await?;
    assert!(!certificate_expiry_created.replayed);
    assert_eq!(certificate_expiry_created.value, certificate_expiry_policy);
    assert!(
        repository
            .create_alert_policy(certificate_expiry_create_write)
            .await?
            .replayed
    );
    assert_eq!(
        repository
            .list_active_alert_policies_for_source(
                organization_id,
                NotificationAlertSource::EdgeGatewayCertificateExpiryStatusV1,
                project_id,
                environment_id,
                certificate_expiry_policy.created_at,
            )
            .await?,
        vec![certificate_expiry_policy.clone()]
    );
    assert_eq!(
        repository
            .list_alert_policy_page(organization_id, recipient, None, 50)
            .await?
            .len(),
        4
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("insert into notification_alert_policies (organization_id, id, recipient_principal_id, source, project_id, environment_id, notify_on_recovery, definition_schema, canonical_acl, definition_digest, aggregate_version, created_by, created_at, revoked_at) select organization_id, ")
                    .bind(Uuid::now_v7())
                    .append(", recipient_principal_id, 'edge.unreviewed-event.v1', project_id, environment_id, notify_on_recovery, definition_schema, canonical_acl, definition_digest, aggregate_version, created_by, created_at, revoked_at from notification_alert_policies where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(certificate_policy.id.as_uuid()),
            )
            .await,
        "persist an unregistered notification alert source",
    );

    let identity = Arc::new(PostgresIdentityRepository::new(executor));
    let projector = OutboxNotificationProjector::new(repository.clone(), identity.clone())
        .with_alert_policies(repository.clone(), identity);
    let claim_id = DomainClaimId::new();
    let rejected = notification_domain_claim_message(
        organization_id,
        project_id,
        environment_id,
        claim_id,
        "edge.domain-claim.rejected",
        DomainClaimState::Rejected,
        Some("provider-private rejection detail"),
        2,
        policy.created_at + ChronoDuration::seconds(1),
    )?;
    persist_outbox_message(database, &rejected).await?;
    projector.project(&rejected).await?;
    projector.project(&rejected).await?;
    let projected = repository
        .list_page(organization_id, recipient, false, None, 50)
        .await?;
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].severity, NotificationSeverity::Warning);
    assert_eq!(projected[0].source_event_key, "edge.domain-claim.rejected");
    assert!(!projected[0].body.contains("provider-private"));

    let recovered = notification_domain_claim_message(
        organization_id,
        project_id,
        environment_id,
        claim_id,
        "edge.domain-claim.verified",
        DomainClaimState::Verified,
        None,
        3,
        policy.created_at + ChronoDuration::seconds(2),
    )?;
    persist_outbox_message(database, &recovered).await?;
    projector.project(&recovered).await?;
    projector.project(&recovered).await?;
    let projected = repository
        .list_page(organization_id, recipient, false, None, 50)
        .await?;
    assert_eq!(projected.len(), 2);
    assert_eq!(projected[0].source_event_key, "edge.domain-claim.verified");
    assert_eq!(projected[0].severity, NotificationSeverity::Information);
    assert_eq!(projected[1].source_event_key, "edge.domain-claim.rejected");

    let revoked = policy.revoke(1, recipient, policy.created_at + ChronoDuration::seconds(3))?;
    let revoke_request_id = Uuid::now_v7();
    let revoke_write = RevokeNotificationAlertPolicyWrite {
        event: NotificationAlertPolicyEvent::envelope(
            "notification.alert-policy.revoked",
            &revoked,
            revoke_request_id,
        )?,
        policy: revoked.clone(),
        expected_version: 1,
        actor_principal_id: recipient,
        request_id: revoke_request_id,
        idempotency: IdempotencyRequest::new(
            "notification-alert-policy-revoke",
            "postgres:alert:revoke",
            b"expected-version:1",
        )?,
    };
    let revoked_write = repository.revoke_alert_policy(revoke_write.clone()).await?;
    assert!(!revoked_write.replayed);
    assert_eq!(revoked_write.value, revoked);
    assert!(repository.revoke_alert_policy(revoke_write).await?.replayed);
    assert!(repository
        .list_active_alert_policies_for_source(
            organization_id,
            NotificationAlertSource::EdgeDomainClaimStatusV1,
            project_id,
            environment_id,
            revoked.revoked_at.expect("revoked at"),
        )
        .await?
        .is_empty());

    let late_rejected = notification_domain_claim_message(
        organization_id,
        project_id,
        environment_id,
        DomainClaimId::new(),
        "edge.domain-claim.rejected",
        DomainClaimState::Rejected,
        Some("late rejection"),
        2,
        revoked.revoked_at.expect("revoked at") + ChronoDuration::seconds(1),
    )?;
    persist_outbox_message(database, &late_rejected).await?;
    projector.project(&late_rejected).await?;
    assert_eq!(
        repository
            .list_page(organization_id, recipient, false, None, 50)
            .await?
            .len(),
        2
    );

    let route_id = RouteId::new();
    let node_id = NodeId::new();
    let initial_renewal = notification_gateway_certificate_renewal_message(
        organization_id,
        project_id,
        environment_id,
        route_id,
        node_id,
        "edge.gateway-certificate.renewed",
        GatewayCertificateRenewalStatus::Renewed,
        None,
        10,
        policy.created_at + ChronoDuration::seconds(5),
    )?;
    persist_outbox_message(database, &initial_renewal).await?;
    projector.project(&initial_renewal).await?;

    let unavailable = notification_gateway_certificate_renewal_message(
        organization_id,
        project_id,
        environment_id,
        route_id,
        node_id,
        "edge.gateway-certificate.renewal-failed",
        GatewayCertificateRenewalStatus::Failed,
        Some(GatewayCertificateRenewalFailureKind::Unavailable),
        11,
        policy.created_at + ChronoDuration::seconds(6),
    )?;
    persist_outbox_message(database, &unavailable).await?;
    projector.project(&unavailable).await?;
    projector.project(&unavailable).await?;

    let peer_recovery = notification_gateway_certificate_renewal_message(
        organization_id,
        project_id,
        environment_id,
        route_id,
        NodeId::new(),
        "edge.gateway-certificate.renewed",
        GatewayCertificateRenewalStatus::Renewed,
        None,
        12,
        policy.created_at + ChronoDuration::seconds(7),
    )?;
    persist_outbox_message(database, &peer_recovery).await?;
    projector.project(&peer_recovery).await?;

    let renewed = notification_gateway_certificate_renewal_message(
        organization_id,
        project_id,
        environment_id,
        route_id,
        node_id,
        "edge.gateway-certificate.renewed",
        GatewayCertificateRenewalStatus::Renewed,
        None,
        12,
        policy.created_at + ChronoDuration::seconds(8),
    )?;
    persist_outbox_message(database, &renewed).await?;
    projector.project(&renewed).await?;
    projector.project(&renewed).await?;

    let certificate_notifications = repository
        .list_page(organization_id, recipient, false, None, 50)
        .await?
        .into_iter()
        .filter(|notification| {
            notification
                .source_event_key
                .starts_with("edge.gateway-certificate.")
        })
        .collect::<Vec<_>>();
    assert_eq!(certificate_notifications.len(), 2);
    assert_eq!(
        certificate_notifications[0].source_event_key,
        "edge.gateway-certificate.renewed"
    );
    assert_eq!(
        certificate_notifications[0].severity,
        NotificationSeverity::Information
    );
    assert_eq!(
        certificate_notifications[1].source_event_key,
        "edge.gateway-certificate.renewal-failed"
    );
    assert_eq!(
        certificate_notifications[1].severity,
        NotificationSeverity::Critical
    );
    assert!(certificate_notifications.iter().all(|notification| {
        notification.scope
            == NotificationScope::Environment {
                project_id,
                environment_id,
            }
            && notification.body.contains("postgres-tls.example.com")
            && notification.body.contains(&route_id.to_string())
            && notification.body.contains(&node_id.to_string())
    }));

    let workload_id = WorkloadId::new();
    let initial_workload_health = notification_workload_deployment_health_message(
        organization_id,
        project_id,
        environment_id,
        workload_id,
        "workload.deployment.healthy",
        WorkloadDeploymentHealthStatus::Healthy,
        None,
        None,
        1,
        policy.created_at + ChronoDuration::seconds(9),
    )?;
    persist_outbox_message(database, &initial_workload_health).await?;
    projector.project(&initial_workload_health).await?;

    let retained_workload_failure = notification_workload_deployment_health_message(
        organization_id,
        project_id,
        environment_id,
        workload_id,
        "workload.deployment.failed",
        WorkloadDeploymentHealthStatus::Failed,
        Some(WorkloadDeploymentFailurePhase::Verifying),
        Some(WorkloadDeploymentAvailabilityImpact::PreviousRevisionRetained),
        2,
        policy.created_at + ChronoDuration::seconds(10),
    )?;
    persist_outbox_message(database, &retained_workload_failure).await?;
    projector.project(&retained_workload_failure).await?;
    projector.project(&retained_workload_failure).await?;

    let peer_workload_health = notification_workload_deployment_health_message(
        organization_id,
        project_id,
        environment_id,
        WorkloadId::new(),
        "workload.deployment.healthy",
        WorkloadDeploymentHealthStatus::Healthy,
        None,
        None,
        3,
        policy.created_at + ChronoDuration::seconds(11),
    )?;
    persist_outbox_message(database, &peer_workload_health).await?;
    projector.project(&peer_workload_health).await?;

    let recovered_workload = notification_workload_deployment_health_message(
        organization_id,
        project_id,
        environment_id,
        workload_id,
        "workload.deployment.healthy",
        WorkloadDeploymentHealthStatus::Healthy,
        None,
        None,
        3,
        policy.created_at + ChronoDuration::seconds(12),
    )?;
    persist_outbox_message(database, &recovered_workload).await?;
    projector.project(&recovered_workload).await?;
    projector.project(&recovered_workload).await?;

    let unavailable_workload = notification_workload_deployment_health_message(
        organization_id,
        project_id,
        environment_id,
        workload_id,
        "workload.deployment.failed",
        WorkloadDeploymentHealthStatus::Failed,
        Some(WorkloadDeploymentFailurePhase::Scheduled),
        Some(WorkloadDeploymentAvailabilityImpact::Unavailable),
        4,
        policy.created_at + ChronoDuration::seconds(13),
    )?;
    persist_outbox_message(database, &unavailable_workload).await?;
    projector.project(&unavailable_workload).await?;
    projector.project(&unavailable_workload).await?;

    let workload_notifications = repository
        .list_page(organization_id, recipient, false, None, 50)
        .await?
        .into_iter()
        .filter(|notification| {
            notification
                .source_event_key
                .starts_with("workload.deployment.")
        })
        .collect::<Vec<_>>();
    assert_eq!(workload_notifications.len(), 3);
    assert_eq!(
        workload_notifications[0].source_event_key,
        "workload.deployment.failed"
    );
    assert_eq!(
        workload_notifications[0].severity,
        NotificationSeverity::Critical
    );
    assert_eq!(
        workload_notifications[1].source_event_key,
        "workload.deployment.healthy"
    );
    assert_eq!(
        workload_notifications[1].severity,
        NotificationSeverity::Information
    );
    assert_eq!(
        workload_notifications[2].source_event_key,
        "workload.deployment.failed"
    );
    assert_eq!(
        workload_notifications[2].severity,
        NotificationSeverity::Warning
    );
    assert!(workload_notifications.iter().all(|notification| {
        notification.scope
            == NotificationScope::Environment {
                project_id,
                environment_id,
            }
            && notification.source_aggregate_id == workload_id.as_uuid()
            && notification.body.contains("postgres-checkout-api")
            && !notification.body.contains("provider-private")
    }));

    let expiry_route_id = RouteId::new();
    let expiry_node_id = NodeId::new();
    let initial_previous_certificate_id = GatewayCertificateId::new();
    let initial_replacement_certificate_id = GatewayCertificateId::new();
    let initial_expiry_resolution = notification_gateway_certificate_expiry_message(
        organization_id,
        project_id,
        environment_id,
        expiry_route_id,
        expiry_node_id,
        initial_previous_certificate_id,
        initial_replacement_certificate_id,
        GatewayCertificateExpiryStatus::Resolved,
        3,
        3,
        policy.created_at + ChronoDuration::seconds(14),
    )?;
    persist_outbox_message(database, &initial_expiry_resolution).await?;
    projector.project(&initial_expiry_resolution).await?;

    let previous_certificate_id = initial_replacement_certificate_id;
    let replacement_certificate_id = GatewayCertificateId::new();
    let expiry_firing = notification_gateway_certificate_expiry_message(
        organization_id,
        project_id,
        environment_id,
        expiry_route_id,
        expiry_node_id,
        previous_certificate_id,
        replacement_certificate_id,
        GatewayCertificateExpiryStatus::Expiring,
        3,
        5,
        policy.created_at + ChronoDuration::seconds(15),
    )?;
    persist_outbox_message(database, &expiry_firing).await?;
    projector.project(&expiry_firing).await?;
    projector.project(&expiry_firing).await?;

    let peer_expiry_resolution = notification_gateway_certificate_expiry_message(
        organization_id,
        project_id,
        environment_id,
        expiry_route_id,
        NodeId::new(),
        previous_certificate_id,
        replacement_certificate_id,
        GatewayCertificateExpiryStatus::Resolved,
        5,
        5,
        policy.created_at + ChronoDuration::seconds(16),
    )?;
    persist_outbox_message(database, &peer_expiry_resolution).await?;
    projector.project(&peer_expiry_resolution).await?;

    let expiry_resolution = notification_gateway_certificate_expiry_message(
        organization_id,
        project_id,
        environment_id,
        expiry_route_id,
        expiry_node_id,
        previous_certificate_id,
        replacement_certificate_id,
        GatewayCertificateExpiryStatus::Resolved,
        5,
        5,
        policy.created_at + ChronoDuration::seconds(17),
    )?;
    assert_ne!(
        initial_expiry_resolution.event_id,
        expiry_resolution.event_id
    );
    persist_outbox_message(database, &expiry_resolution).await?;
    projector.project(&expiry_resolution).await?;
    projector.project(&expiry_resolution).await?;

    let next_expiry_firing = notification_gateway_certificate_expiry_message(
        organization_id,
        project_id,
        environment_id,
        expiry_route_id,
        expiry_node_id,
        replacement_certificate_id,
        GatewayCertificateId::new(),
        GatewayCertificateExpiryStatus::Expiring,
        5,
        7,
        policy.created_at + ChronoDuration::seconds(18),
    )?;
    persist_outbox_message(database, &next_expiry_firing).await?;
    projector.project(&next_expiry_firing).await?;
    projector.project(&next_expiry_firing).await?;

    let expiry_notifications = repository
        .list_page(organization_id, recipient, false, None, 50)
        .await?
        .into_iter()
        .filter(|notification| {
            matches!(
                notification.source_event_key.as_str(),
                "edge.gateway-certificate.expiring" | "edge.gateway-certificate.expiry-resolved"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(expiry_notifications.len(), 3);
    assert_eq!(
        expiry_notifications
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
    assert!(expiry_notifications.iter().all(|notification| {
        notification.scope
            == NotificationScope::Environment {
                project_id,
                environment_id,
            }
            && notification.body.contains("postgres-expiry.example.com")
            && notification.body.contains(&expiry_route_id.to_string())
            && notification.body.contains(&expiry_node_id.to_string())
            && !notification.body.contains("private")
    }));

    assert_rejected(
        database
            .execute(
                sql_query::<()>(
                    "update notification_alert_policies set canonical_acl = canonical_acl || ' ' where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(policy.id.as_uuid()),
            )
            .await,
        "mutate a notification alert policy ACL",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("delete from notification_alert_policies where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(policy.id.as_uuid()),
            )
            .await,
        "delete a notification alert policy",
    );
    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64)>(
                "select (select count(*) from notification_alert_policies where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and recipient_principal_id = ")
            .bind(recipient.as_uuid())
            .append("), (select count(*) from outbox_events where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and event_key = 'notification.alert-policy.created'), (select count(*) from outbox_events where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and event_key = 'notification.alert-policy.revoked'), (select count(*) from audit_records where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and action in ('notification.alert-policy.created', 'notification.alert-policy.revoked')), (select count(*) from notifications where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and recipient_principal_id = ")
            .bind(recipient.as_uuid())
            .append(" and source_event_key in ('edge.domain-claim.rejected', 'edge.domain-claim.verified', 'edge.gateway-certificate.renewal-failed', 'edge.gateway-certificate.renewed', 'workload.deployment.failed', 'workload.deployment.healthy', 'edge.gateway-certificate.expiring', 'edge.gateway-certificate.expiry-resolved'))"),
        )
        .await?;
    assert_eq!(evidence, (4, 4, 1, 5, 10));
    Ok(())
}

fn notification_alert_policy_create_write(
    policy: &NotificationAlertPolicy,
    idempotency_key: &str,
) -> Result<CreateNotificationAlertPolicyWrite, Box<dyn std::error::Error>> {
    let request_id = Uuid::now_v7();
    Ok(CreateNotificationAlertPolicyWrite {
        event: NotificationAlertPolicyEvent::envelope(
            "notification.alert-policy.created",
            policy,
            request_id,
        )?,
        policy: policy.clone(),
        actor_principal_id: policy.recipient_principal_id,
        request_id,
        idempotency: IdempotencyRequest::new(
            "notification-alert-policy-create",
            idempotency_key,
            policy.definition.digest().as_str().as_bytes(),
        )?,
    })
}

#[allow(clippy::too_many_arguments)]
fn notification_domain_claim_message(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    claim_id: DomainClaimId,
    event_key: &str,
    state: DomainClaimState,
    failure: Option<&str>,
    aggregate_version: u64,
    occurred_at: chrono::DateTime<Utc>,
) -> Result<OutboxMessage, Box<dyn std::error::Error>> {
    Ok(OutboxMessage {
        event_id: Uuid::now_v7(),
        event_key: event_key.into(),
        schema_version: 1,
        organization_id: organization_id.as_uuid(),
        aggregate_id: claim_id.as_uuid(),
        aggregate_version,
        occurred_at,
        correlation_id: Uuid::now_v7(),
        causation_id: None,
        payload: serde_json::to_value(DomainClaimChanged {
            organization_id,
            project_id,
            environment_id,
            domain_claim_id: claim_id,
            pattern: "postgres.example.com".into(),
            state,
            failure: failure.map(str::to_owned),
        })?,
        delivery_attempts: 1,
    })
}

#[allow(clippy::too_many_arguments)]
fn notification_gateway_certificate_renewal_message(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    route_id: RouteId,
    node_id: NodeId,
    event_key: &str,
    status: GatewayCertificateRenewalStatus,
    failure_kind: Option<GatewayCertificateRenewalFailureKind>,
    aggregate_version: u64,
    occurred_at: chrono::DateTime<Utc>,
) -> Result<OutboxMessage, Box<dyn std::error::Error>> {
    let previous_certificate_id = GatewayCertificateId::new();
    let replacement_certificate_id = GatewayCertificateId::new();
    let active_certificate_id = match status {
        GatewayCertificateRenewalStatus::Failed => previous_certificate_id,
        GatewayCertificateRenewalStatus::Renewed => replacement_certificate_id,
    };
    let raw_expiry = occurred_at + ChronoDuration::days(30);
    let active_certificate_expires_at = raw_expiry
        - ChronoDuration::nanoseconds(i64::from(raw_expiry.timestamp_subsec_nanos() % 1_000));
    Ok(OutboxMessage {
        event_id: Uuid::now_v7(),
        event_key: event_key.into(),
        schema_version: 1,
        organization_id: organization_id.as_uuid(),
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
            hostname: "postgres-tls.example.com".into(),
            path_prefix: "/service".into(),
            gateway_revision: aggregate_version,
            previous_certificate_id,
            replacement_certificate_id,
            active_certificate_id,
            active_certificate_expires_at,
            status,
            failure_kind,
        })?,
        delivery_attempts: 1,
    })
}

#[allow(clippy::too_many_arguments)]
fn notification_gateway_certificate_expiry_message(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    route_id: RouteId,
    node_id: NodeId,
    previous_certificate_id: GatewayCertificateId,
    replacement_certificate_id: GatewayCertificateId,
    status: GatewayCertificateExpiryStatus,
    certificate_gateway_revision: u64,
    renewal_gateway_revision: u64,
    occurred_at: chrono::DateTime<Utc>,
) -> Result<OutboxMessage, Box<dyn std::error::Error>> {
    let (event_key, active_certificate_id) = match status {
        GatewayCertificateExpiryStatus::Expiring => {
            ("edge.gateway-certificate.expiring", previous_certificate_id)
        }
        GatewayCertificateExpiryStatus::Resolved => (
            "edge.gateway-certificate.expiry-resolved",
            replacement_certificate_id,
        ),
    };
    let raw_expiry = occurred_at + ChronoDuration::days(30);
    let active_certificate_expires_at = raw_expiry
        - ChronoDuration::nanoseconds(i64::from(raw_expiry.timestamp_subsec_nanos() % 1_000));
    let aggregate_id = renewal_subject_id(route_id, node_id);
    Ok(OutboxMessage {
        event_id: Uuid::new_v5(
            &aggregate_id,
            format!("{event_key}:{active_certificate_id}").as_bytes(),
        ),
        event_key: event_key.into(),
        schema_version: 1,
        organization_id: organization_id.as_uuid(),
        aggregate_id,
        aggregate_version: certificate_expiry_aggregate_version(
            certificate_gateway_revision,
            status,
        )?,
        occurred_at,
        correlation_id: Uuid::now_v7(),
        causation_id: None,
        payload: serde_json::to_value(GatewayCertificateExpiryChanged {
            organization_id,
            project_id,
            environment_id,
            route_id,
            workload_id: WorkloadId::new(),
            node_id,
            hostname: "postgres-expiry.example.com".into(),
            path_prefix: "/service".into(),
            certificate_gateway_revision,
            renewal_gateway_revision,
            previous_certificate_id,
            replacement_certificate_id,
            active_certificate_id,
            active_certificate_expires_at,
            status,
        })?,
        delivery_attempts: 1,
    })
}

#[allow(clippy::too_many_arguments)]
fn notification_workload_deployment_health_message(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    workload_id: WorkloadId,
    event_key: &str,
    status: WorkloadDeploymentHealthStatus,
    failure_phase: Option<WorkloadDeploymentFailurePhase>,
    availability_impact: Option<WorkloadDeploymentAvailabilityImpact>,
    aggregate_version: u64,
    occurred_at: chrono::DateTime<Utc>,
) -> Result<OutboxMessage, Box<dyn std::error::Error>> {
    let operation_id = OperationId::new();
    Ok(OutboxMessage {
        event_id: Uuid::now_v7(),
        event_key: event_key.into(),
        schema_version: 1,
        organization_id: organization_id.as_uuid(),
        aggregate_id: workload_id.as_uuid(),
        aggregate_version,
        occurred_at,
        correlation_id: operation_id.as_uuid(),
        causation_id: None,
        payload: serde_json::to_value(WorkloadDeploymentHealthChanged {
            organization_id,
            project_id,
            environment_id,
            workload_id,
            workload_name: "postgres-checkout-api".into(),
            deployment_id: DeploymentId::new(),
            revision_id: WorkloadRevisionId::new(),
            revision_generation: aggregate_version,
            operation_id,
            node_id: Some(NodeId::new()),
            status,
            failure_phase,
            availability_impact,
        })?,
        delivery_attempts: 1,
    })
}

async fn persist_outbox_message(
    database: &Database<PostgresDialect, PostgresExecutor>,
    message: &OutboxMessage,
) -> Result<(), Box<dyn std::error::Error>> {
    database
        .execute(
            sql_query::<()>("insert into outbox_events (event_id, event_key, schema_version, organization_id, aggregate_id, aggregate_version, occurred_at, correlation_id, causation_id, payload) values (")
                .bind(message.event_id)
                .append(", ")
                .bind(message.event_key.clone())
                .append(", ")
                .bind(message.schema_version)
                .append(", ")
                .bind(message.organization_id)
                .append(", ")
                .bind(message.aggregate_id)
                .append(", ")
                .bind(message.aggregate_version)
                .append(", ")
                .bind(message.occurred_at)
                .append(", ")
                .bind(message.correlation_id)
                .append(", ")
                .bind(message.causation_id)
                .append(", ")
                .bind(message.payload.clone())
                .append(")"),
        )
        .await?;
    Ok(())
}
