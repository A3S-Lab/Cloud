use super::*;
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_cloud_control_plane::modules::notifications::{
    INotificationRepository, MarkNotificationReadWrite, Notification, NotificationScope,
    NotificationSeverity, PostgresNotificationRepository,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    IdempotencyRequest, OrganizationId, PrincipalId,
};

pub(super) async fn exercise_notification_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = connect_and_migrate(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("106"),
        )
        .await?;
    assert_eq!(
        migration_state,
        (1, "deduplicated notification inbox projections".into())
    );

    let organization_id = OrganizationId::new();
    let recipient = PrincipalId::new();
    let other_recipient = PrincipalId::new();
    let created_at = Utc::now();
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", 'Notification tenant', ")
            .bind(format!("notification-{organization_id}"))
            .append(", 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    for (principal_id, name) in [
        (recipient, "Notification recipient"),
        (other_recipient, "Other recipient"),
    ] {
        database
            .execute(
                sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (")
                    .bind(principal_id.as_uuid())
                    .append(", 'human', ")
                    .bind(name)
                    .append(", 1, ")
                    .bind(created_at)
                    .append(", null)"),
            )
            .await?;
    }

    let first = source_notification(
        &database,
        organization_id,
        recipient,
        "Organization access granted",
        created_at,
    )
    .await?;
    let repository = PostgresNotificationRepository::new(executor.clone());
    assert!(repository.project(first.clone()).await?);
    assert!(!repository.project(first.clone()).await?);
    let mut drift = first.clone();
    drift.title = "Changed title".into();
    assert!(repository.project(drift).await.is_err());

    let concurrent = source_notification(
        &database,
        organization_id,
        recipient,
        "Organization role changed",
        created_at + chrono::Duration::seconds(1),
    )
    .await?;
    let (left, right) = tokio::join!(
        repository.project(concurrent.clone()),
        repository.project(concurrent.clone())
    );
    let outcomes = [left?, right?];
    assert_eq!(outcomes.into_iter().filter(|inserted| *inserted).count(), 1);

    assert!(repository
        .find(organization_id, other_recipient, first.id)
        .await?
        .is_none());
    assert_eq!(
        repository
            .list_page(organization_id, recipient, false, None, 50)
            .await?
            .len(),
        2
    );

    let request_id = Uuid::now_v7();
    let read = first.mark_read(1, created_at + chrono::Duration::seconds(2))?;
    let idempotency = IdempotencyRequest::new(
        format!(
            "organizations/{organization_id}/notifications/{}/read",
            first.id
        ),
        "postgres:notification:read",
        b"expected-version:1",
    )?;
    let event = DomainEventEnvelope {
        event_id: Uuid::now_v7(),
        event_key: "notification.inbox.read".into(),
        schema_version: 1,
        organization_id: organization_id.as_uuid(),
        aggregate_id: read.id.as_uuid(),
        aggregate_version: read.aggregate_version,
        occurred_at: read.read_at.expect("read time"),
        correlation_id: request_id,
        causation_id: Some(read.source_event_id),
        payload: json!({
            "notificationId": read.id,
            "sourceEventId": read.source_event_id
        }),
    };
    let write = MarkNotificationReadWrite {
        notification: read.clone(),
        expected_version: 1,
        actor_principal_id: recipient,
        event,
        idempotency: idempotency.clone(),
        request_id,
    };
    let written = repository.mark_read(write.clone()).await?;
    assert!(!written.replayed);
    let replayed = repository.mark_read(write).await?;
    assert!(replayed.replayed);
    assert_eq!(replayed.value, read);
    assert!(repository
        .replay_mark_read(&idempotency)
        .await?
        .is_some_and(|value| value.replayed && value.value == read));
    assert_eq!(
        repository
            .list_page(organization_id, recipient, true, None, 50)
            .await?
            .len(),
        1
    );

    assert_rejected(
        database
            .execute(
                sql_query::<()>(
                    "update notifications set title = 'Tampered' where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(first.id.as_uuid()),
            )
            .await,
        "mutate immutable notification content",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("delete from notifications where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(first.id.as_uuid()),
            )
            .await,
        "delete a notification",
    );

    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64)>("select (select count(*) from notifications where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and recipient_principal_id = ")
                .bind(recipient.as_uuid())
                .append("), (select count(*) from notifications where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and recipient_principal_id = ")
                .bind(recipient.as_uuid())
                .append(" and read_at is not null), (select count(*) from outbox_events where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and event_key = 'notification.inbox.read'), (select count(*) from audit_records where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and action = 'notification.inbox.read')"),
        )
        .await?;
    assert_eq!(evidence, (2, 1, 1, 1));
    Ok(())
}

async fn source_notification(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    recipient: PrincipalId,
    title: &str,
    occurred_at: chrono::DateTime<Utc>,
) -> Result<Notification, Box<dyn std::error::Error>> {
    let event_id = Uuid::now_v7();
    let aggregate_id = Uuid::now_v7();
    let correlation_id = Uuid::now_v7();
    let notification = Notification::project(
        organization_id,
        recipient,
        event_id,
        "identity.membership.role-changed".into(),
        1,
        aggregate_id,
        1,
        correlation_id,
        NotificationSeverity::Information,
        title.into(),
        format!("{title}."),
        NotificationScope::Organization,
        occurred_at,
        occurred_at,
    )?;
    database
        .execute(
            sql_query::<()>("insert into outbox_events (event_id, event_key, schema_version, organization_id, aggregate_id, aggregate_version, occurred_at, correlation_id, causation_id, payload) values (")
                .bind(event_id)
                .append(", 'identity.membership.role-changed', 1, ")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(aggregate_id)
                .append(", 1, ")
                .bind(notification.occurred_at)
                .append(", ")
                .bind(correlation_id)
                .append(", null, ")
                .bind(json!({
                    "membership_id": aggregate_id,
                    "principal_id": recipient,
                    "role": "member"
                }))
                .append(")"),
        )
        .await?;
    Ok(notification)
}

fn assert_rejected<T, E: std::fmt::Debug>(result: Result<T, E>, label: &str) {
    assert!(result.is_err(), "database must reject {label}");
}
