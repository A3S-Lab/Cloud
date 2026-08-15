use super::*;
use crate::modules::connectors::{
    ConnectorDefinition, ConnectorHttpAuthentication, ConnectorHttpDefinition,
    ConnectorHttpDefinitionSpec, ConnectorHttpDestination, ConnectorHttpMethod,
    ConnectorHttpStatusPolicy, ConnectorProfile, ConnectorRecord, ConnectorRevision,
    ConnectorRevisionPublished, CreateConnectorProfileWrite, IConnectorProfileRepository,
    InMemoryConnectorProfileRepository,
};
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::notifications::{
    INotificationRepository, InMemoryNotificationRepository, Notification, NotificationScope,
    NotificationSeverity, OutboundNotificationChannel, OutboundNotificationConnectorTarget,
    OutboundNotificationSubscriptionDefinition, OutboundNotificationSubscriptionSpec,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ConnectorProfileId, ConnectorRevisionId, EnvironmentId,
    IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, ResourceName,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};

struct Fixture {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    actor: PrincipalId,
    definition_acl: String,
    notifications: Arc<InMemoryNotificationRepository>,
    create: CreateOutboundNotificationSubscriptionHandler,
}

async fn fixture() -> Fixture {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let actor = PrincipalId::new();
    let now = canonical_timestamp(Utc::now());
    let revision = ConnectorRevision::initial(
        organization_id,
        project_id,
        environment_id,
        ConnectorProfileId::new(),
        ConnectorRevisionId::new(),
        ConnectorDefinition::Http(
            ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
                destination: ConnectorHttpDestination::LiteralHttps {
                    endpoint: "https://hooks.example.test/notifications".into(),
                },
                method: ConnectorHttpMethod::Post,
                request_content_type: "application/json".into(),
                maximum_request_bytes: 16 * 1024,
                maximum_response_bytes: 1024,
                timeout_milliseconds: 1_000,
                status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
                authentication: ConnectorHttpAuthentication::None,
            })
            .expect("Connector definition"),
        ),
        actor,
        now,
    )
    .expect("Connector revision");
    let profile = ConnectorProfile::create(
        revision.profile_id,
        ResourceName::parse("Notification delivery").expect("Connector name"),
        &revision,
    )
    .expect("Connector profile");
    let record = ConnectorRecord::new(profile, revision.clone()).expect("Connector record");
    let connectors = Arc::new(InMemoryConnectorProfileRepository::new());
    let request_id = Uuid::now_v7();
    connectors
        .create(CreateConnectorProfileWrite {
            event: ConnectorRevisionPublished::created(
                &record.profile,
                &record.revision,
                request_id,
            )
            .expect("Connector event"),
            actor_principal_id: actor,
            request_id,
            idempotency: IdempotencyRequest::new(
                "notification-subscription-test",
                "connector",
                revision.definition.digest().as_str().as_bytes(),
            )
            .expect("Connector idempotency"),
            record,
        })
        .await
        .expect("store Connector");
    let definition_acl = OutboundNotificationSubscriptionDefinition::from_spec(
        OutboundNotificationSubscriptionSpec {
            channel: OutboundNotificationChannel::SlackCompatible,
            minimum_severity: NotificationSeverity::Information,
            target: OutboundNotificationConnectorTarget::new(
                project_id,
                environment_id,
                revision.profile_id,
                revision.id,
            )
            .expect("target"),
        },
    )
    .expect("subscription definition")
    .canonical_acl()
    .to_owned();
    let notifications = Arc::new(InMemoryNotificationRepository::new());
    let outbound: Arc<dyn IOutboundNotificationRepository> = notifications.clone();
    let connector_repository: Arc<dyn IConnectorProfileRepository> = connectors;
    Fixture {
        organization_id,
        project_id,
        environment_id,
        actor,
        definition_acl,
        create: CreateOutboundNotificationSubscriptionHandler::new(outbound, connector_repository),
        notifications,
    }
}

#[tokio::test]
async fn create_replay_projection_and_revoke_share_one_authority() {
    let fixture = fixture().await;
    let create = CreateOutboundNotificationSubscription {
        organization_id: fixture.organization_id,
        definition_acl: fixture.definition_acl.clone(),
        actor_principal_id: fixture.actor,
        resource_access: ResourceAccessEvaluator::restricted([ResourceGrantScope::Environment {
            project_id: fixture.project_id,
            environment_id: fixture.environment_id,
        }]),
        idempotency_key: "create-delivery".into(),
        request_id: Uuid::now_v7(),
    };
    let created = fixture
        .create
        .execute(create.clone(), context())
        .await
        .expect("command framework")
        .expect("create subscription");
    assert!(!created.replayed);
    assert!(
        fixture
            .create
            .execute(create.clone(), context())
            .await
            .expect("command framework")
            .expect("create replay")
            .replayed
    );
    let denied = fixture
        .create
        .execute(
            CreateOutboundNotificationSubscription {
                resource_access: ResourceAccessEvaluator::restricted([
                    ResourceGrantScope::Environment {
                        project_id: fixture.project_id,
                        environment_id: EnvironmentId::new(),
                    },
                ]),
                ..create
            },
            context(),
        )
        .await
        .expect("command framework");
    assert!(matches!(denied, Err(ApplicationError::NotFound(_))));

    let first = notification(&fixture, Uuid::now_v7());
    assert!(fixture
        .notifications
        .project(first)
        .await
        .expect("project notification"));
    assert_eq!(fixture.notifications.outbound_deliveries().await.len(), 1);
    assert_eq!(
        fixture
            .notifications
            .outbox_events()
            .await
            .iter()
            .filter(|event| event.event_key == "notification.delivery.requested")
            .count(),
        1
    );

    let revoke = RevokeOutboundNotificationSubscription {
        organization_id: fixture.organization_id,
        subscription_id: created.subscription.id,
        expected_version: 1,
        actor_principal_id: fixture.actor,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: "revoke-delivery".into(),
        request_id: Uuid::now_v7(),
    };
    let revoke_handler =
        RevokeOutboundNotificationSubscriptionHandler::new(fixture.notifications.clone());
    let revoked = revoke_handler
        .execute(revoke.clone(), context())
        .await
        .expect("command framework")
        .expect("revoke subscription");
    assert!(!revoked.subscription.is_active());
    assert!(
        revoke_handler
            .execute(revoke, context())
            .await
            .expect("command framework")
            .expect("revoke replay")
            .replayed
    );

    fixture
        .notifications
        .project(notification(&fixture, Uuid::now_v7()))
        .await
        .expect("project after revoke");
    assert_eq!(fixture.notifications.outbound_deliveries().await.len(), 1);
}

fn notification(fixture: &Fixture, source_event_id: Uuid) -> Notification {
    let now = canonical_timestamp(Utc::now());
    Notification::project(
        fixture.organization_id,
        fixture.actor,
        source_event_id,
        "identity.membership.role-changed".into(),
        1,
        Uuid::now_v7(),
        2,
        Uuid::now_v7(),
        NotificationSeverity::Information,
        "Organization role changed".into(),
        "Your organization role is now member.".into(),
        NotificationScope::Organization,
        now,
        now,
    )
    .expect("notification")
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
