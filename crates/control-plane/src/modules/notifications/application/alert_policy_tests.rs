use super::*;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::notifications::domain::{
    CreateNotificationAlertPolicyWrite, INotificationAlertPolicyRepository,
    NotificationAlertPolicySpec, NotificationAlertSource,
};
use crate::modules::notifications::InMemoryNotificationRepository;
use crate::modules::projects::InMemoryProjectsRepository;
use crate::modules::shared_kernel::domain::{EnvironmentId, ProjectId};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};

#[tokio::test]
async fn create_replay_requires_the_environment_to_still_exist() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let actor = PrincipalId::new();
    let definition = NotificationAlertPolicyDefinition::from_spec(NotificationAlertPolicySpec {
        source: NotificationAlertSource::EdgeDomainClaimStatusV1,
        project_id,
        environment_id,
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
