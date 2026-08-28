use super::*;

#[allow(clippy::too_many_arguments)]
fn domain_claim_message(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    claim_id: DomainClaimId,
    event_key: &str,
    state: DomainClaimState,
    failure: Option<&str>,
    aggregate_version: u64,
    occurred_at: DateTime<Utc>,
) -> OutboxMessage {
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
            pattern: "app.example.com".into(),
            state,
            failure: failure.map(str::to_owned),
        })
        .expect("domain claim payload"),
        delivery_attempts: 1,
    }
}

#[tokio::test]
async fn domain_claim_rejection_and_recovery_are_personal_deterministic_projections() {
    let organization_id = OrganizationId::new();
    let recipient = PrincipalId::new();
    let membership_id = MembershipId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let claim_id = DomainClaimId::new();
    let created_at = Utc::now();
    let notifications = Arc::new(InMemoryNotificationRepository::new());
    create_alert_policy(
        notifications.as_ref(),
        organization_id,
        recipient,
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
    let rejected = domain_claim_message(
        organization_id,
        project_id,
        environment_id,
        claim_id,
        "edge.domain-claim.rejected",
        DomainClaimState::Rejected,
        Some("private provider detail must not leak"),
        2,
        created_at + chrono::Duration::seconds(1),
    );

    projector
        .project(&rejected)
        .await
        .expect("project rejection");
    projector
        .project(&rejected)
        .await
        .expect("replay rejection projection");
    let projected = notifications
        .list_page(organization_id, recipient, false, None, 50)
        .await
        .expect("rejection notifications");
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].severity, NotificationSeverity::Warning);
    assert_eq!(projected[0].title, "Domain claim rejected");
    assert!(!projected[0].body.contains("private provider detail"));
    assert_eq!(
        projected[0].scope,
        NotificationScope::Environment {
            project_id,
            environment_id,
        }
    );

    let recovered = domain_claim_message(
        organization_id,
        project_id,
        environment_id,
        claim_id,
        "edge.domain-claim.verified",
        DomainClaimState::Verified,
        None,
        3,
        created_at + chrono::Duration::seconds(2),
    );
    projector
        .project(&recovered)
        .await
        .expect("project recovery");
    projector
        .project(&recovered)
        .await
        .expect("replay recovery projection");
    let projected = notifications
        .list_page(organization_id, recipient, false, None, 50)
        .await
        .expect("recovery notifications");
    assert_eq!(projected.len(), 2);
    assert_eq!(projected[0].severity, NotificationSeverity::Information);
    assert_eq!(projected[0].title, "Domain claim recovered");
    assert_eq!(projected[0].source_aggregate_version, 3);
    assert_eq!(projected[1].source_aggregate_version, 2);
}

#[tokio::test]
async fn domain_claim_recovery_requires_a_post_policy_rejection_and_opt_in() {
    let organization_id = OrganizationId::new();
    let recipient = PrincipalId::new();
    let membership_id = MembershipId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let created_at = Utc::now();
    let notifications = Arc::new(InMemoryNotificationRepository::new());
    create_alert_policy(
        notifications.as_ref(),
        organization_id,
        recipient,
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

    let historical_claim = DomainClaimId::new();
    projector
        .project(&domain_claim_message(
            organization_id,
            project_id,
            environment_id,
            historical_claim,
            "edge.domain-claim.rejected",
            DomainClaimState::Rejected,
            Some("historical rejection"),
            2,
            created_at - chrono::Duration::seconds(1),
        ))
        .await
        .expect("historical rejection is ignored");
    projector
        .project(&domain_claim_message(
            organization_id,
            project_id,
            environment_id,
            historical_claim,
            "edge.domain-claim.verified",
            DomainClaimState::Verified,
            None,
            3,
            created_at + chrono::Duration::seconds(1),
        ))
        .await
        .expect("recovery without projected rejection is ignored");
    projector
        .project(&domain_claim_message(
            organization_id,
            project_id,
            environment_id,
            DomainClaimId::new(),
            "edge.domain-claim.verified",
            DomainClaimState::Verified,
            None,
            2,
            created_at + chrono::Duration::seconds(1),
        ))
        .await
        .expect("initial verification is ignored");
    assert!(notifications
        .list_page(organization_id, recipient, false, None, 50)
        .await
        .expect("notifications")
        .is_empty());

    let no_recovery_recipient = PrincipalId::new();
    let no_recovery_membership_id = MembershipId::new();
    let no_recovery_environment_id = EnvironmentId::new();
    create_alert_policy(
        notifications.as_ref(),
        organization_id,
        no_recovery_recipient,
        project_id,
        no_recovery_environment_id,
        false,
        created_at,
    )
    .await;
    let no_recovery_projector = OutboxNotificationProjector::new(
        notifications.clone(),
        membership_lookup(
            organization_id,
            no_recovery_membership_id,
            no_recovery_recipient,
            created_at,
        ),
    )
    .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));
    let claim_id = DomainClaimId::new();
    no_recovery_projector
        .project(&domain_claim_message(
            organization_id,
            project_id,
            no_recovery_environment_id,
            claim_id,
            "edge.domain-claim.rejected",
            DomainClaimState::Rejected,
            Some("rejected"),
            2,
            created_at + chrono::Duration::seconds(1),
        ))
        .await
        .expect("project rejection");
    no_recovery_projector
        .project(&domain_claim_message(
            organization_id,
            project_id,
            no_recovery_environment_id,
            claim_id,
            "edge.domain-claim.verified",
            DomainClaimState::Verified,
            None,
            3,
            created_at + chrono::Duration::seconds(2),
        ))
        .await
        .expect("recovery opt-out is ignored");
    let projected = notifications
        .list_page(organization_id, no_recovery_recipient, false, None, 50)
        .await
        .expect("notifications");
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].source_event_key, "edge.domain-claim.rejected");
}

#[tokio::test]
async fn domain_claim_alerts_recheck_policy_membership_and_resource_grants() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let created_at = Utc::now();
    let notifications = Arc::new(InMemoryNotificationRepository::new());

    let restricted_recipient = PrincipalId::new();
    let restricted_membership_id = MembershipId::new();
    create_alert_policy(
        notifications.as_ref(),
        organization_id,
        restricted_recipient,
        project_id,
        environment_id,
        true,
        created_at,
    )
    .await;
    let restricted_projector = OutboxNotificationProjector::new(
        notifications.clone(),
        membership_lookup_with_role(
            organization_id,
            restricted_membership_id,
            restricted_recipient,
            MembershipRole::Restricted,
            true,
            created_at,
        ),
    )
    .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));
    restricted_projector
        .project(&domain_claim_message(
            organization_id,
            project_id,
            environment_id,
            DomainClaimId::new(),
            "edge.domain-claim.rejected",
            DomainClaimState::Rejected,
            Some("rejected"),
            2,
            created_at + chrono::Duration::seconds(1),
        ))
        .await
        .expect("missing grant is ignored");
    assert!(notifications
        .list_page(organization_id, restricted_recipient, false, None, 50,)
        .await
        .expect("notifications")
        .is_empty());

    let granted_recipient = PrincipalId::new();
    let granted_membership_id = MembershipId::new();
    create_alert_policy(
        notifications.as_ref(),
        organization_id,
        granted_recipient,
        project_id,
        environment_id,
        true,
        created_at,
    )
    .await;
    let grant = ResourceGrant::create(
        ResourceGrantId::new(),
        organization_id,
        granted_membership_id,
        ResourceGrantScope::Environment {
            project_id,
            environment_id,
        },
        created_at,
    );
    let granted_projector = OutboxNotificationProjector::new(
        notifications.clone(),
        membership_lookup_with_role(
            organization_id,
            granted_membership_id,
            granted_recipient,
            MembershipRole::Restricted,
            true,
            created_at,
        ),
    )
    .with_alert_policies(notifications.clone(), resource_grants(vec![grant]));
    granted_projector
        .project(&domain_claim_message(
            organization_id,
            project_id,
            environment_id,
            DomainClaimId::new(),
            "edge.domain-claim.rejected",
            DomainClaimState::Rejected,
            Some("rejected"),
            2,
            created_at + chrono::Duration::seconds(1),
        ))
        .await
        .expect("matching grant projects alert");
    assert_eq!(
        notifications
            .list_page(organization_id, granted_recipient, false, None, 50)
            .await
            .expect("notifications")
            .len(),
        1
    );

    let revoked_recipient = PrincipalId::new();
    let revoked_membership_id = MembershipId::new();
    create_alert_policy(
        notifications.as_ref(),
        organization_id,
        revoked_recipient,
        project_id,
        environment_id,
        true,
        created_at,
    )
    .await;
    let revoked_member_projector = OutboxNotificationProjector::new(
        notifications.clone(),
        membership_lookup_with_role(
            organization_id,
            revoked_membership_id,
            revoked_recipient,
            MembershipRole::Member,
            false,
            created_at,
        ),
    )
    .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));
    revoked_member_projector
        .project(&domain_claim_message(
            organization_id,
            project_id,
            environment_id,
            DomainClaimId::new(),
            "edge.domain-claim.rejected",
            DomainClaimState::Rejected,
            Some("rejected"),
            2,
            created_at + chrono::Duration::seconds(1),
        ))
        .await
        .expect("revoked membership is ignored");
    assert!(notifications
        .list_page(organization_id, revoked_recipient, false, None, 50)
        .await
        .expect("notifications")
        .is_empty());

    let revoked_policy_recipient = PrincipalId::new();
    let policy = create_alert_policy(
        notifications.as_ref(),
        organization_id,
        revoked_policy_recipient,
        project_id,
        environment_id,
        true,
        created_at,
    )
    .await;
    revoke_alert_policy(
        notifications.as_ref(),
        &policy,
        created_at + chrono::Duration::seconds(1),
    )
    .await;
    let revoked_policy_projector = OutboxNotificationProjector::new(
        notifications.clone(),
        membership_lookup(
            organization_id,
            MembershipId::new(),
            revoked_policy_recipient,
            created_at,
        ),
    )
    .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));
    revoked_policy_projector
        .project(&domain_claim_message(
            organization_id,
            project_id,
            environment_id,
            DomainClaimId::new(),
            "edge.domain-claim.rejected",
            DomainClaimState::Rejected,
            Some("rejected"),
            2,
            created_at + chrono::Duration::seconds(2),
        ))
        .await
        .expect("revoked policy is ignored");
    assert!(notifications
        .list_page(organization_id, revoked_policy_recipient, false, None, 50,)
        .await
        .expect("notifications")
        .is_empty());
}

#[test]
fn malformed_domain_claim_payloads_fail_closed() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let claim_id = DomainClaimId::new();
    let mut message = domain_claim_message(
        organization_id,
        project_id,
        environment_id,
        claim_id,
        "edge.domain-claim.rejected",
        DomainClaimState::Rejected,
        Some("rejected"),
        2,
        Utc::now(),
    );
    message.payload["unexpected"] = serde_json::json!(true);
    assert!(decode_domain_claim(&message).is_err());

    let mut inconsistent = domain_claim_message(
        organization_id,
        project_id,
        environment_id,
        claim_id,
        "edge.domain-claim.verified",
        DomainClaimState::Rejected,
        Some("rejected"),
        3,
        Utc::now(),
    );
    assert!(decode_domain_claim(&inconsistent).is_err());
    inconsistent.payload["state"] = serde_json::json!("verified");
    assert!(decode_domain_claim(&inconsistent).is_err());
}
