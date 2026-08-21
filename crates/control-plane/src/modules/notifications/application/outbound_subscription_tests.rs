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
    GetOutboundNotificationSubscription, GetOutboundNotificationSubscriptionHandler,
    INotificationRepository, InMemoryNotificationRepository, ListOutboundNotificationSubscriptions,
    ListOutboundNotificationSubscriptionsHandler, Notification, NotificationScope,
    NotificationSeverity, OutboundNotificationChannel, OutboundNotificationConnectorTarget,
    OutboundNotificationSubscriptionDefinition, OutboundNotificationSubscriptionSpec,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ConnectorProfileId, ConnectorRevisionId, EnvironmentId,
    IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, ResourceName,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};

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
    let version_one_fact = fixture
        .notifications
        .outbox_events()
        .await
        .into_iter()
        .find(|event| event.event_key == "notification.outbound-subscription.created")
        .expect("version one subscription fact");
    assert_eq!(version_one_fact.schema_version, 1);
    for absent in [
        "definitionSchema",
        "maximumProviderAttempts",
        "suppressBefore",
    ] {
        assert!(
            version_one_fact.payload.get(absent).is_none(),
            "historic version one subscription fact gained {absent}"
        );
    }
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

#[tokio::test]
async fn version_three_suppression_keeps_inbox_and_admits_only_the_exact_boundary() {
    let fixture = fixture().await;
    let suppress_before = canonical_timestamp(Utc::now()) + chrono::Duration::days(1);
    let definition = OutboundNotificationSubscriptionDefinition::from_spec_with_suppression(
        OutboundNotificationSubscriptionDefinition::parse_acl(&fixture.definition_acl)
            .expect("base definition")
            .spec(),
        2,
        suppress_before,
    )
    .expect("suppression definition");
    let created = fixture
        .create
        .execute(
            CreateOutboundNotificationSubscription {
                organization_id: fixture.organization_id,
                definition_acl: definition.canonical_acl().into(),
                actor_principal_id: fixture.actor,
                resource_access: ResourceAccessEvaluator::restricted([
                    ResourceGrantScope::Environment {
                        project_id: fixture.project_id,
                        environment_id: fixture.environment_id,
                    },
                ]),
                idempotency_key: "create-suppressed-delivery".into(),
                request_id: Uuid::now_v7(),
            },
            context(),
        )
        .await
        .expect("command framework")
        .expect("create suppressed subscription")
        .subscription;
    assert_eq!(created.definition.schema_version(), 3);
    assert_eq!(created.definition.delivery_schema_version(), 2);
    let subscription_fact = fixture
        .notifications
        .outbox_events()
        .await
        .into_iter()
        .find(|event| event.event_key == "notification.outbound-subscription.created")
        .expect("version three subscription fact");
    assert_eq!(subscription_fact.schema_version, 3);
    assert_eq!(
        subscription_fact.payload["definitionSchema"],
        "cloud.notification.outbound-subscription.v3"
    );
    assert_eq!(subscription_fact.payload["maximumProviderAttempts"], 2);
    assert_eq!(
        subscription_fact.payload["suppressBefore"],
        serde_json::json!(suppress_before)
    );

    let suppressed = notification_at(
        &fixture,
        Uuid::now_v7(),
        suppress_before - chrono::Duration::microseconds(1),
    );
    assert!(fixture
        .notifications
        .project(suppressed)
        .await
        .expect("project suppressed notification"));
    assert_eq!(
        fixture
            .notifications
            .list_page(fixture.organization_id, fixture.actor, false, None, 50)
            .await
            .expect("personal inbox")
            .len(),
        1
    );
    assert!(fixture.notifications.outbound_deliveries().await.is_empty());

    let boundary = notification_at(&fixture, Uuid::now_v7(), suppress_before);
    assert!(fixture
        .notifications
        .project(boundary)
        .await
        .expect("project boundary notification"));
    let deliveries = fixture.notifications.outbound_deliveries().await;
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].schema_version(), 2);
    assert_eq!(deliveries[0].maximum_provider_attempts(), 2);
    assert_eq!(
        deliveries[0]
            .requested_event()
            .expect("delivery fact")
            .payload["schema"],
        "a3s.cloud.notification-delivery.v2"
    );
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
}

#[tokio::test]
async fn personal_queries_hide_foreign_or_ungranted_subscriptions_and_page_visible_records() {
    let fixture = fixture().await;
    let access = ResourceAccessEvaluator::restricted([ResourceGrantScope::Environment {
        project_id: fixture.project_id,
        environment_id: fixture.environment_id,
    }]);
    let first = fixture
        .create
        .execute(
            CreateOutboundNotificationSubscription {
                organization_id: fixture.organization_id,
                definition_acl: fixture.definition_acl.clone(),
                actor_principal_id: fixture.actor,
                resource_access: access.clone(),
                idempotency_key: "query-first".into(),
                request_id: Uuid::now_v7(),
            },
            context(),
        )
        .await
        .expect("command framework")
        .expect("first subscription")
        .subscription;
    let first_target = first.definition.spec().target;
    let second_acl = OutboundNotificationSubscriptionDefinition::from_spec(
        OutboundNotificationSubscriptionSpec {
            channel: OutboundNotificationChannel::SignedWebhook,
            minimum_severity: NotificationSeverity::Warning,
            target: first_target,
        },
    )
    .expect("second definition")
    .canonical_acl()
    .to_owned();
    let second = fixture
        .create
        .execute(
            CreateOutboundNotificationSubscription {
                organization_id: fixture.organization_id,
                definition_acl: second_acl,
                actor_principal_id: fixture.actor,
                resource_access: access.clone(),
                idempotency_key: "query-second".into(),
                request_id: Uuid::now_v7(),
            },
            context(),
        )
        .await
        .expect("command framework")
        .expect("second subscription")
        .subscription;
    let repository: Arc<dyn IOutboundNotificationRepository> = fixture.notifications.clone();
    let list = ListOutboundNotificationSubscriptionsHandler::new(repository.clone());
    let get = GetOutboundNotificationSubscriptionHandler::new(repository);

    let first_page = list
        .execute(
            ListOutboundNotificationSubscriptions {
                organization_id: fixture.organization_id,
                actor_principal_id: fixture.actor,
                resource_access: access.clone(),
                cursor: None,
                limit: 1,
            },
            context(),
        )
        .await
        .expect("query framework")
        .expect("first page");
    assert_eq!(first_page.subscriptions.len(), 1);
    assert!(first_page.next_cursor.is_some());
    let second_page = list
        .execute(
            ListOutboundNotificationSubscriptions {
                organization_id: fixture.organization_id,
                actor_principal_id: fixture.actor,
                resource_access: access.clone(),
                cursor: first_page.next_cursor,
                limit: 1,
            },
            context(),
        )
        .await
        .expect("query framework")
        .expect("second page");
    assert_eq!(second_page.subscriptions.len(), 1);
    assert!(second_page.next_cursor.is_none());
    assert_ne!(
        first_page.subscriptions[0].id,
        second_page.subscriptions[0].id
    );

    let foreign = get
        .execute(
            GetOutboundNotificationSubscription {
                organization_id: fixture.organization_id,
                subscription_id: first.id,
                actor_principal_id: PrincipalId::new(),
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework");
    assert!(matches!(foreign, Err(ApplicationError::NotFound(_))));
    let denied = get
        .execute(
            GetOutboundNotificationSubscription {
                organization_id: fixture.organization_id,
                subscription_id: second.id,
                actor_principal_id: fixture.actor,
                resource_access: ResourceAccessEvaluator::restricted([
                    ResourceGrantScope::Environment {
                        project_id: fixture.project_id,
                        environment_id: EnvironmentId::new(),
                    },
                ]),
            },
            context(),
        )
        .await
        .expect("query framework");
    assert!(matches!(denied, Err(ApplicationError::NotFound(_))));
}

fn notification(fixture: &Fixture, source_event_id: Uuid) -> Notification {
    let now = canonical_timestamp(Utc::now());
    notification_at(fixture, source_event_id, now)
}

fn notification_at(
    fixture: &Fixture,
    source_event_id: Uuid,
    occurred_at: chrono::DateTime<Utc>,
) -> Notification {
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
        occurred_at,
        occurred_at,
    )
    .expect("notification")
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
