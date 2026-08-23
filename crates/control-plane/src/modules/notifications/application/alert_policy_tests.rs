use super::*;
use crate::modules::fleet::infrastructure::persistence::InMemoryNodeRepository;
use crate::modules::fleet::{
    domain::entities::EnrollmentToken,
    domain::repositories::NodeEnrollmentDraft,
    domain::value_objects::{EnrollmentTokenCredential, NodeCapabilities, NodeName},
};
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::notifications::domain::{
    CreateNotificationAlertPolicyWrite, INotificationAlertPolicyRepository,
    NotificationAlertPolicySpec, NotificationAlertPolicyTarget, NotificationAlertSource,
};
use crate::modules::notifications::InMemoryNotificationRepository;
use crate::modules::projects::InMemoryProjectsRepository;
use crate::modules::shared_kernel::domain::{EnrollmentTokenId, EnvironmentId, NodeId, ProjectId};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use a3s_cloud_contracts::DomainEventEnvelope;

#[tokio::test]
async fn create_replay_requires_the_environment_to_still_exist() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let actor = PrincipalId::new();
    let definition = NotificationAlertPolicyDefinition::from_spec(NotificationAlertPolicySpec {
        source: NotificationAlertSource::EdgeDomainClaimStatusV1,
        target: NotificationAlertPolicyTarget::Environment {
            project_id,
            environment_id,
        },
        notify_on_recovery: true,
    })
    .expect("alert policy definition");
    let policy = NotificationAlertPolicy::create(
        organization_id,
        NotificationAlertPolicyId::new(),
        actor,
        definition.clone(),
        actor,
        Utc::now(),
    )
    .expect("alert policy");
    let canonical_request = serde_json::to_vec(&serde_json::json!({
        "organizationId": organization_id,
        "recipientPrincipalId": actor,
        "definitionDigest": definition.digest(),
    }))
    .expect("canonical request");
    let idempotency = IdempotencyRequest::new(
        format!("organizations/{organization_id}/principals/{actor}/notification-alert-policies"),
        "missing-environment-replay",
        &canonical_request,
    )
    .expect("idempotency");
    let request_id = Uuid::now_v7();
    let notifications = Arc::new(InMemoryNotificationRepository::new());
    notifications
        .create_alert_policy(CreateNotificationAlertPolicyWrite {
            event: NotificationAlertPolicyEvent::envelope(
                "notification.alert-policy.created",
                &policy,
                request_id,
            )
            .expect("alert policy event"),
            policy,
            actor_principal_id: actor,
            request_id,
            idempotency,
        })
        .await
        .expect("seed replay");

    let result = CreateNotificationAlertPolicyHandler::new(
        notifications,
        Arc::new(InMemoryProjectsRepository::new()),
        Arc::new(InMemoryNodeRepository::new()),
    )
    .execute(
        CreateNotificationAlertPolicy {
            organization_id,
            definition_acl: definition.canonical_acl().into(),
            actor_principal_id: actor,
            resource_access: ResourceAccessEvaluator::restricted([
                ResourceGrantScope::Environment {
                    project_id,
                    environment_id,
                },
            ]),
            idempotency_key: "missing-environment-replay".into(),
            request_id: Uuid::now_v7(),
        },
        CqrsContext::new(ModuleRef::new()),
    )
    .await
    .expect("command framework");

    assert!(matches!(result, Err(ApplicationError::NotFound(_))));
}

#[tokio::test]
async fn create_node_policy_requires_the_exact_existing_node_and_node_grant() {
    let organization_id = OrganizationId::new();
    let node_id = NodeId::new();
    let actor = PrincipalId::new();
    let definition = NotificationAlertPolicyDefinition::from_spec(NotificationAlertPolicySpec {
        source: NotificationAlertSource::FleetNodeAvailabilityStatusV1,
        target: NotificationAlertPolicyTarget::Node { node_id },
        notify_on_recovery: true,
    })
    .expect("Node alert policy definition");
    let notifications = Arc::new(InMemoryNotificationRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    seed_node(nodes.as_ref(), organization_id, node_id).await;
    let handler = CreateNotificationAlertPolicyHandler::new(
        notifications,
        Arc::new(InMemoryProjectsRepository::new()),
        nodes,
    );
    let command = |resource_access, idempotency_key: &str| CreateNotificationAlertPolicy {
        organization_id,
        definition_acl: definition.canonical_acl().into(),
        actor_principal_id: actor,
        resource_access,
        idempotency_key: idempotency_key.into(),
        request_id: Uuid::now_v7(),
    };

    let environment_only = handler
        .execute(
            command(
                ResourceAccessEvaluator::restricted([ResourceGrantScope::Environment {
                    project_id: ProjectId::new(),
                    environment_id: EnvironmentId::new(),
                }]),
                "node-policy-environment-denied",
            ),
            CqrsContext::new(ModuleRef::new()),
        )
        .await
        .expect("command framework");
    assert!(matches!(
        environment_only,
        Err(ApplicationError::NotFound(_))
    ));

    let other_node_only = handler
        .execute(
            command(
                ResourceAccessEvaluator::restricted([ResourceGrantScope::Node {
                    node_id: NodeId::new(),
                }]),
                "node-policy-other-node-denied",
            ),
            CqrsContext::new(ModuleRef::new()),
        )
        .await
        .expect("command framework");
    assert!(matches!(
        other_node_only,
        Err(ApplicationError::NotFound(_))
    ));

    let created = handler
        .execute(
            command(
                ResourceAccessEvaluator::restricted([ResourceGrantScope::Node { node_id }]),
                "node-policy-create",
            ),
            CqrsContext::new(ModuleRef::new()),
        )
        .await
        .expect("command framework")
        .expect("create exact Node policy");
    assert_eq!(
        created.policy.definition.spec().target,
        NotificationAlertPolicyTarget::Node { node_id }
    );
}

async fn seed_node(
    nodes: &InMemoryNodeRepository,
    organization_id: OrganizationId,
    node_id: NodeId,
) {
    let now = crate::modules::shared_kernel::domain::canonical_timestamp(Utc::now());
    let credential = EnrollmentTokenCredential::from_secret(&format!("a3sn_{}", "a".repeat(64)))
        .expect("enrollment credential");
    let token = EnrollmentToken::new(
        EnrollmentTokenId::new(),
        organization_id,
        "Notification policy node",
        credential.clone(),
        now,
        now + chrono::Duration::hours(1),
    )
    .expect("enrollment token");
    nodes
        .issue_enrollment_token(
            token,
            DomainEventEnvelope {
                event_id: Uuid::now_v7(),
                event_key: "fleet.enrollment-token.issued".into(),
                schema_version: 1,
                organization_id: organization_id.as_uuid(),
                aggregate_id: Uuid::now_v7(),
                aggregate_version: 1,
                occurred_at: now,
                correlation_id: Uuid::now_v7(),
                causation_id: None,
                payload: serde_json::json!({}),
            },
            IdempotencyRequest::new(
                "notification-policy-node-token",
                Uuid::now_v7().to_string(),
                b"notification policy Node token",
            )
            .expect("token idempotency"),
        )
        .await
        .expect("store enrollment token");
    nodes
        .reserve_enrollment(
            &credential,
            NodeEnrollmentDraft {
                proposed_node_id: node_id,
                name: NodeName::new("Notification policy node").expect("Node name"),
                agent_instance_id: Uuid::now_v7(),
                agent_version: "1.0.0".into(),
                capabilities: NodeCapabilities::new(
                    "test-runtime",
                    "test-build",
                    serde_json::json!({}),
                )
                .expect("Node capabilities"),
                request_digest: "notification-policy-node-enrollment".into(),
                requested_at: now,
            },
        )
        .await
        .expect("reserve Node enrollment");
}
