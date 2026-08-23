use super::*;
use crate::modules::fleet::domain::events::{
    NodeAvailabilityChanged, NodeAvailabilityResolutionReason, NodeAvailabilitySnapshot,
};
use crate::modules::fleet::domain::value_objects::NodeState;
use a3s_cloud_contracts::DomainEventEnvelope;

fn message(event: DomainEventEnvelope) -> OutboxMessage {
    OutboxMessage {
        event_id: event.event_id,
        event_key: event.event_key,
        schema_version: event.schema_version,
        organization_id: event.organization_id,
        aggregate_id: event.aggregate_id,
        aggregate_version: event.aggregate_version,
        occurred_at: event.occurred_at,
        correlation_id: event.correlation_id,
        causation_id: event.causation_id,
        payload: event.payload,
        delivery_attempts: 1,
    }
}

fn firing(
    organization_id: OrganizationId,
    node_id: NodeId,
    node_version: u64,
    last_observed_at: DateTime<Utc>,
) -> OutboxMessage {
    message(
        NodeAvailabilityChanged::unavailable_envelope(
            NodeAvailabilitySnapshot {
                organization_id,
                node_id,
                state: NodeState::Ready,
                node_aggregate_version: node_version,
                last_observed_at,
            },
            last_observed_at + chrono::Duration::seconds(10),
            last_observed_at + chrono::Duration::seconds(11),
        )
        .expect("Node unavailable fact"),
    )
}

fn resolution(
    firing: &OutboxMessage,
    node_version: u64,
    last_observed_at: DateTime<Utc>,
    resolved_at: DateTime<Utc>,
) -> OutboxMessage {
    resolution_with_reason(
        firing,
        node_version,
        last_observed_at,
        NodeState::Ready,
        NodeAvailabilityResolutionReason::HeartbeatRestored,
        resolved_at,
    )
}

fn resolution_with_reason(
    firing: &OutboxMessage,
    node_version: u64,
    last_observed_at: DateTime<Utc>,
    state: NodeState,
    reason: NodeAvailabilityResolutionReason,
    resolved_at: DateTime<Utc>,
) -> OutboxMessage {
    let firing_envelope = DomainEventEnvelope {
        event_id: firing.event_id,
        event_key: firing.event_key.clone(),
        schema_version: firing.schema_version,
        organization_id: firing.organization_id,
        aggregate_id: firing.aggregate_id,
        aggregate_version: firing.aggregate_version,
        occurred_at: firing.occurred_at,
        correlation_id: firing.correlation_id,
        causation_id: firing.causation_id,
        payload: firing.payload.clone(),
    };
    let firing_identity =
        NodeAvailabilityChanged::firing(&firing_envelope).expect("open Node firing");
    message(
        NodeAvailabilityChanged::resolved_envelope(
            NodeAvailabilitySnapshot {
                organization_id: OrganizationId::from_uuid(firing.organization_id),
                node_id: NodeId::from_uuid(firing.aggregate_id),
                state,
                node_aggregate_version: node_version,
                last_observed_at,
            },
            firing_identity,
            reason,
            resolved_at,
        )
        .expect("Node availability resolution"),
    )
}

#[tokio::test]
async fn node_revocation_resolves_without_claiming_the_node_recovered() {
    let organization_id = OrganizationId::new();
    let node_id = NodeId::new();
    let recipient = PrincipalId::new();
    let membership_id = MembershipId::new();
    let created_at = canonical_timestamp(Utc::now());
    let notifications = Arc::new(InMemoryNotificationRepository::new());
    create_node_policy(
        notifications.as_ref(),
        organization_id,
        recipient,
        node_id,
        true,
        created_at,
    )
    .await;
    let projector = OutboxNotificationProjector::new(
        notifications.clone(),
        membership_lookup(organization_id, membership_id, recipient, created_at),
    )
    .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));
    let last_observed_at = created_at + chrono::Duration::seconds(1);
    let source = firing(organization_id, node_id, 2, last_observed_at);
    projector
        .project(&source)
        .await
        .expect("project Node unavailable firing");

    let revoked = resolution_with_reason(
        &source,
        3,
        last_observed_at,
        NodeState::Revoked,
        NodeAvailabilityResolutionReason::NodeRevoked,
        created_at + chrono::Duration::seconds(13),
    );
    projector
        .project(&revoked)
        .await
        .expect("project Node revocation resolution");
    projector
        .project(&revoked)
        .await
        .expect("replay Node revocation resolution");

    let projected = notifications
        .list_page(organization_id, recipient, false, None, 50)
        .await
        .expect("Node revocation notifications");
    assert_eq!(projected.len(), 2);
    assert_eq!(
        projected[0].source_event_key,
        "fleet.node.availability-resolved"
    );
    assert!(projected[0].body.contains("was revoked"));
    assert!(!projected[0].body.contains("recovered"));
}

async fn create_node_policy(
    notifications: &InMemoryNotificationRepository,
    organization_id: OrganizationId,
    recipient: PrincipalId,
    node_id: NodeId,
    notify_on_recovery: bool,
    created_at: DateTime<Utc>,
) -> NotificationAlertPolicy {
    let definition = NotificationAlertPolicyDefinition::from_spec(NotificationAlertPolicySpec {
        source: NotificationAlertSource::FleetNodeAvailabilityStatusV1,
        target: NotificationAlertPolicyTarget::Node { node_id },
        notify_on_recovery,
    })
    .expect("Node alert policy definition");
    let policy = NotificationAlertPolicy::create(
        organization_id,
        NotificationAlertPolicyId::new(),
        recipient,
        definition,
        recipient,
        created_at,
    )
    .expect("Node alert policy");
    let request_id = Uuid::now_v7();
    notifications
        .create_alert_policy(CreateNotificationAlertPolicyWrite {
            event: NotificationAlertPolicyEvent::envelope(
                "notification.alert-policy.created",
                &policy,
                request_id,
            )
            .expect("Node alert policy event"),
            policy,
            actor_principal_id: recipient,
            request_id,
            idempotency: IdempotencyRequest::new(
                "notification-node-alert-policy-create",
                request_id.to_string(),
                b"canonical Node alert policy create",
            )
            .expect("Node alert policy idempotency"),
        })
        .await
        .expect("store Node alert policy")
        .value
}

#[tokio::test]
async fn node_unavailable_and_resolution_are_exact_ordered_replay_safe_projections() {
    let organization_id = OrganizationId::new();
    let node_id = NodeId::new();
    let recipient = PrincipalId::new();
    let membership_id = MembershipId::new();
    let created_at = canonical_timestamp(Utc::now());
    let notifications = Arc::new(InMemoryNotificationRepository::new());
    create_node_policy(
        notifications.as_ref(),
        organization_id,
        recipient,
        node_id,
        true,
        created_at,
    )
    .await;
    let projector = OutboxNotificationProjector::new(
        notifications.clone(),
        membership_lookup(organization_id, membership_id, recipient, created_at),
    )
    .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));

    let stale_firing = firing(
        organization_id,
        node_id,
        2,
        created_at - chrono::Duration::seconds(30),
    );
    projector
        .project(&stale_firing)
        .await
        .expect("pre-policy firing is silent");
    let initial_resolution = resolution(
        &stale_firing,
        3,
        created_at + chrono::Duration::seconds(1),
        created_at + chrono::Duration::seconds(2),
    );
    projector
        .project(&initial_resolution)
        .await
        .expect("resolution after a stale firing is silent");

    let current_firing = firing(
        organization_id,
        node_id,
        4,
        created_at + chrono::Duration::seconds(3),
    );
    projector
        .project(&current_firing)
        .await
        .expect("project Node firing");
    projector
        .project(&current_firing)
        .await
        .expect("replay Node firing");

    let other_node_firing = firing(
        organization_id,
        NodeId::new(),
        4,
        created_at + chrono::Duration::seconds(4),
    );
    projector
        .project(&other_node_firing)
        .await
        .expect("another Node firing is silent");
    projector
        .project(&resolution(
            &other_node_firing,
            5,
            created_at + chrono::Duration::seconds(16),
            created_at + chrono::Duration::seconds(17),
        ))
        .await
        .expect("another Node resolution is silent");

    let resolved = resolution(
        &current_firing,
        5,
        created_at + chrono::Duration::seconds(15),
        created_at + chrono::Duration::seconds(16),
    );
    projector
        .project(&resolved)
        .await
        .expect("project Node resolution");
    projector
        .project(&resolved)
        .await
        .expect("replay Node resolution");

    let projected = notifications
        .list_page(organization_id, recipient, false, None, 50)
        .await
        .expect("Node availability notifications");
    assert_eq!(projected.len(), 2);
    assert_eq!(
        projected[0].source_event_key,
        "fleet.node.availability-resolved"
    );
    assert_eq!(projected[0].severity, NotificationSeverity::Information);
    assert_eq!(projected[1].source_event_key, "fleet.node.unavailable");
    assert_eq!(projected[1].severity, NotificationSeverity::Critical);
    assert!(projected
        .iter()
        .all(|notification| notification.scope == NotificationScope::Node { node_id }));
    assert!(projected.iter().all(|notification| {
        notification.body.contains(&node_id.to_string())
            && !notification
                .body
                .to_ascii_lowercase()
                .contains("capabilities")
            && !notification
                .body
                .to_ascii_lowercase()
                .contains("credential")
    }));
}

#[tokio::test]
async fn node_alerts_require_the_current_exact_node_grant() {
    let organization_id = OrganizationId::new();
    let node_id = NodeId::new();
    let recipient = PrincipalId::new();
    let membership_id = MembershipId::new();
    let created_at = canonical_timestamp(Utc::now());
    let notifications = Arc::new(InMemoryNotificationRepository::new());
    create_node_policy(
        notifications.as_ref(),
        organization_id,
        recipient,
        node_id,
        true,
        created_at,
    )
    .await;
    let source = firing(
        organization_id,
        node_id,
        2,
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
    let environment_grant = ResourceGrant::create(
        ResourceGrantId::new(),
        organization_id,
        membership_id,
        ResourceGrantScope::Environment {
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
        },
        created_at,
    );
    let unauthorized = OutboxNotificationProjector::new(notifications.clone(), membership())
        .with_alert_policies(
            notifications.clone(),
            resource_grants(vec![environment_grant]),
        );
    unauthorized
        .project(&source)
        .await
        .expect("an Environment grant cannot authorize a Node");

    let node_grant = ResourceGrant::create(
        ResourceGrantId::new(),
        organization_id,
        membership_id,
        ResourceGrantScope::Node { node_id },
        created_at,
    );
    let authorized = OutboxNotificationProjector::new(notifications.clone(), membership())
        .with_alert_policies(notifications.clone(), resource_grants(vec![node_grant]));
    let mut schema_drift = source.clone();
    schema_drift.schema_version = 2;
    authorized
        .project(&schema_drift)
        .await
        .expect("unsupported schema is silent");
    authorized
        .project(&source)
        .await
        .expect("the exact Node grant projects the alert");

    let authority_lost = OutboxNotificationProjector::new(notifications.clone(), membership())
        .with_alert_policies(notifications.clone(), resource_grants(Vec::new()));
    let recovered = resolution(
        &source,
        3,
        created_at + chrono::Duration::seconds(2),
        created_at + chrono::Duration::seconds(13),
    );
    authority_lost
        .project(&recovered)
        .await
        .expect("grant loss before recovery projection is silent");

    assert_eq!(
        notifications
            .list_page(organization_id, recipient, false, None, 50)
            .await
            .expect("Node notifications")
            .len(),
        1
    );
}

#[test]
fn malformed_node_availability_facts_fail_closed() {
    let occurred_at = canonical_timestamp(Utc::now());
    let valid = firing(OrganizationId::new(), NodeId::new(), 2, occurred_at);
    assert!(decode_node_availability(&valid).is_ok());

    let mut unknown_field = valid.clone();
    unknown_field.payload["capabilities"] = serde_json::json!(["private"]);
    assert!(decode_node_availability(&unknown_field).is_err());

    let mut wrong_subject = valid.clone();
    wrong_subject.aggregate_id = Uuid::now_v7();
    assert!(decode_node_availability(&wrong_subject).is_err());

    let mut wrong_phase = valid.clone();
    wrong_phase.aggregate_version += 1;
    assert!(decode_node_availability(&wrong_phase).is_err());

    let mut forged_identity = valid;
    forged_identity.event_id = Uuid::now_v7();
    assert!(decode_node_availability(&forged_identity).is_err());
}
