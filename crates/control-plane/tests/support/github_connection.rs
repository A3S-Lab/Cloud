use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    OrganizationId, RepositoryError, SourceConnectionId,
};
use a3s_cloud_control_plane::modules::sources::domain::{
    CompleteGithubConnection, GithubAccountId, GithubAccountKind, GithubConnection,
    GithubConnectionCreated, GithubConnectionFlow, GithubConnectionFlowStage,
    GithubConnectionLifecycleChange, GithubConnectionStatus, GithubInstallationAccount,
    GithubInstallationId, GithubLogin, IGithubConnectionRepository, NewGithubConnection,
    ReconcileGithubConnectionLifecycle, VerifiedGithubConnectionLifecycle, WebhookDeliveryId,
};
use a3s_cloud_control_plane::modules::sources::PostgresGithubConnectionRepository;
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub(super) async fn exercise_github_connection_persistence(
    executor: &PostgresExecutor,
    connected_organization_id: OrganizationId,
    installation_conflict_organization_id: OrganizationId,
    account_conflict_organization_id: OrganizationId,
) -> Result<(), Box<dyn std::error::Error>> {
    let repository = PostgresGithubConnectionRepository::new(executor.clone());
    let database = Database::new(PostgresDialect, executor.clone());
    let connected_at = Utc::now();

    let (flow, oauth_state_digest, pkce_verifier_digest) = prepare_github_flow(
        &repository,
        connected_organization_id,
        42,
        "connected",
        connected_at,
    )
    .await?;
    let connection = github_connection(
        connected_organization_id,
        42,
        100,
        "A3S-Lab",
        GithubAccountKind::Organization,
        connected_at,
    )?;
    let event = GithubConnectionCreated::envelope(&connection, Uuid::now_v7())?;
    let completed = repository
        .complete(CompleteGithubConnection {
            flow_id: flow.id,
            connection: connection.clone(),
            event: event.clone(),
            completed_at: connected_at + Duration::seconds(1),
        })
        .await?;
    assert_eq!(completed, connection);
    assert_eq!(
        repository.find(connected_organization_id).await?,
        Some(connection.clone())
    );
    assert!(matches!(
        repository
            .find_oauth_flow(
                &oauth_state_digest,
                &pkce_verifier_digest,
                connected_at + Duration::seconds(1),
            )
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert!(matches!(
        repository
            .complete(CompleteGithubConnection {
                flow_id: flow.id,
                connection: connection.clone(),
                event: event.clone(),
                completed_at: connected_at + Duration::seconds(2),
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    let replacement_flow = GithubConnectionFlow::begin(
        Uuid::now_v7(),
        connected_organization_id,
        github_test_digest("connected-replacement"),
        connected_at,
        connected_at + Duration::minutes(10),
    )
    .map_err(std::io::Error::other)?;
    assert!(matches!(
        repository.begin_flow(replacement_flow).await,
        Err(RepositoryError::Conflict(_))
    ));

    let (installation_conflict_flow, installation_oauth_digest, installation_pkce_digest) =
        prepare_github_flow(
            &repository,
            installation_conflict_organization_id,
            42,
            "installation-conflict",
            connected_at,
        )
        .await?;
    let installation_conflict = github_connection(
        installation_conflict_organization_id,
        42,
        101,
        "Other-Account",
        GithubAccountKind::Organization,
        connected_at,
    )?;
    let installation_conflict_event =
        GithubConnectionCreated::envelope(&installation_conflict, Uuid::now_v7())?;
    assert!(matches!(
        repository
            .complete(CompleteGithubConnection {
                flow_id: installation_conflict_flow.id,
                connection: installation_conflict,
                event: installation_conflict_event,
                completed_at: connected_at + Duration::seconds(1),
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert_eq!(
        repository
            .find(installation_conflict_organization_id)
            .await?,
        None
    );
    assert!(repository
        .find_oauth_flow(
            &installation_oauth_digest,
            &installation_pkce_digest,
            connected_at + Duration::seconds(1),
        )
        .await
        .is_ok());

    let (account_conflict_flow, account_oauth_digest, account_pkce_digest) = prepare_github_flow(
        &repository,
        account_conflict_organization_id,
        43,
        "account-conflict",
        connected_at,
    )
    .await?;
    let account_conflict = github_connection(
        account_conflict_organization_id,
        43,
        100,
        "A3S-Lab",
        GithubAccountKind::Organization,
        connected_at,
    )?;
    let account_conflict_event =
        GithubConnectionCreated::envelope(&account_conflict, Uuid::now_v7())?;
    assert!(matches!(
        repository
            .complete(CompleteGithubConnection {
                flow_id: account_conflict_flow.id,
                connection: account_conflict,
                event: account_conflict_event,
                completed_at: connected_at + Duration::seconds(1),
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert_eq!(
        repository.find(account_conflict_organization_id).await?,
        None
    );
    assert!(repository
        .find_oauth_flow(
            &account_oauth_digest,
            &account_pkce_digest,
            connected_at + Duration::seconds(1),
        )
        .await
        .is_ok());

    let suspend = lifecycle_request(
        "postgres-github-lifecycle-suspend",
        GithubConnectionLifecycleChange::InstallationSuspended {
            installation_id: connection.installation_id,
            account: installation_account(&connection),
        },
        connected_at + Duration::seconds(3),
    )?;
    let suspended = repository.reconcile_lifecycle(suspend.clone()).await?;
    assert!(!suspended.replayed);
    assert_eq!(suspended.connections.len(), 1);
    assert_eq!(
        suspended.connections[0].status,
        GithubConnectionStatus::Suspended
    );
    assert!(repository
        .find_authoritative_by_installation(connection.installation_id)
        .await?
        .is_none());
    assert!(repository.reconcile_lifecycle(suspend).await?.replayed);
    let changed_replay = lifecycle_request(
        "postgres-github-lifecycle-suspend",
        GithubConnectionLifecycleChange::InstallationDeleted {
            installation_id: connection.installation_id,
            account: installation_account(&connection),
        },
        connected_at + Duration::seconds(3),
    )?;
    assert!(matches!(
        repository.reconcile_lifecycle(changed_replay).await,
        Err(RepositoryError::Conflict(_))
    ));
    repository
        .reconcile_lifecycle(lifecycle_request(
            "postgres-github-lifecycle-unsuspend",
            GithubConnectionLifecycleChange::InstallationUnsuspended {
                installation_id: connection.installation_id,
                account: installation_account(&connection),
            },
            connected_at + Duration::seconds(4),
        )?)
        .await?;
    assert!(repository
        .find_authoritative_by_installation(connection.installation_id)
        .await?
        .is_some());
    repository
        .reconcile_lifecycle(lifecycle_request(
            "postgres-github-lifecycle-verification-revoked",
            GithubConnectionLifecycleChange::UserAuthorizationRevoked {
                user_id: connection.verified_by_user_id,
                user_login: connection.verified_by_user_login.clone(),
            },
            connected_at + Duration::seconds(5),
        )?)
        .await?;
    assert_eq!(
        repository
            .find(connected_organization_id)
            .await?
            .map(|connection| connection.status),
        Some(GithubConnectionStatus::VerificationRevoked)
    );

    let (reconnect_flow, _, _) = prepare_github_flow(
        &repository,
        connected_organization_id,
        42,
        "connected-after-revocation",
        connected_at + Duration::seconds(6),
    )
    .await?;
    let reconnected = github_connection(
        connected_organization_id,
        42,
        100,
        "A3S-Lab",
        GithubAccountKind::Organization,
        connected_at + Duration::seconds(6),
    )?;
    let reconnected_event = GithubConnectionCreated::envelope(&reconnected, Uuid::now_v7())?;
    repository
        .complete(CompleteGithubConnection {
            flow_id: reconnect_flow.id,
            connection: reconnected.clone(),
            event: reconnected_event,
            completed_at: connected_at + Duration::seconds(7),
        })
        .await?;
    assert_eq!(
        repository.find(connected_organization_id).await?,
        Some(reconnected.clone())
    );
    assert_eq!(
        repository
            .find_authoritative_by_installation(reconnected.installation_id)
            .await?,
        Some(reconnected)
    );

    let connection_rows = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from github_source_connections",
        ))
        .await?;
    assert_eq!(connection_rows, 2);
    let event_payload = database
        .fetch_one_as(
            sql_query::<Value>(
                "select payload from outbox_events where event_key = 'source.github-connection.created' and event_id = ",
            )
            .bind(event.event_id),
        )
        .await?;
    assert_eq!(event_payload["installation_id"], 42);
    assert_eq!(event_payload["account_id"], 100);
    assert_eq!(event_payload["verified_by_user_id"], 200);
    assert!(!event_payload.to_string().contains("token"));
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from outbox_events where event_key = 'source.github-connection.created'",
            ))
            .await?,
        2
    );
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from outbox_events where event_key = 'source.github-connection.reconciled'",
            ))
            .await?,
        3
    );
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from github_connection_lifecycle_inbox",
            ))
            .await?,
        3
    );
    Ok(())
}

fn installation_account(connection: &GithubConnection) -> GithubInstallationAccount {
    GithubInstallationAccount {
        id: connection.account_id,
        login: connection.account_login.clone(),
        kind: connection.account_kind,
    }
}

fn lifecycle_request(
    delivery_id: &str,
    change: GithubConnectionLifecycleChange,
    received_at: DateTime<Utc>,
) -> Result<ReconcileGithubConnectionLifecycle, Box<dyn std::error::Error>> {
    Ok(ReconcileGithubConnectionLifecycle {
        lifecycle: VerifiedGithubConnectionLifecycle {
            provider: a3s_cloud_control_plane::modules::sources::domain::GitProvider::Github,
            delivery_id: WebhookDeliveryId::parse(delivery_id).map_err(std::io::Error::other)?,
            change,
            payload_digest: github_test_digest(delivery_id),
        },
        correlation_id: Uuid::now_v7(),
        received_at,
    })
}

async fn prepare_github_flow(
    repository: &PostgresGithubConnectionRepository,
    organization_id: OrganizationId,
    installation_id: u64,
    label: &str,
    created_at: DateTime<Utc>,
) -> Result<(GithubConnectionFlow, String, String), Box<dyn std::error::Error>> {
    let installation_state_digest = github_test_digest(&format!("{label}-installation-state"));
    let oauth_state_digest = github_test_digest(&format!("{label}-oauth-state"));
    let pkce_verifier_digest = github_test_digest(&format!("{label}-pkce-verifier"));
    let flow = GithubConnectionFlow::begin(
        Uuid::now_v7(),
        organization_id,
        installation_state_digest.clone(),
        created_at,
        created_at + Duration::minutes(10),
    )
    .map_err(std::io::Error::other)?;
    repository.begin_flow(flow).await?;
    let prepared = repository
        .prepare_oauth(
            &installation_state_digest,
            GithubInstallationId::parse(installation_id).map_err(std::io::Error::other)?,
            oauth_state_digest.clone(),
            pkce_verifier_digest.clone(),
            created_at,
        )
        .await?;
    assert_eq!(prepared.stage, GithubConnectionFlowStage::AwaitingOauth);
    assert_eq!(prepared.state_digest, oauth_state_digest);
    assert_eq!(
        repository
            .find_oauth_flow(&oauth_state_digest, &pkce_verifier_digest, created_at)
            .await?,
        prepared
    );
    Ok((prepared, oauth_state_digest, pkce_verifier_digest))
}

fn github_connection(
    organization_id: OrganizationId,
    installation_id: u64,
    account_id: u64,
    account_login: &str,
    account_kind: GithubAccountKind,
    connected_at: DateTime<Utc>,
) -> Result<GithubConnection, Box<dyn std::error::Error>> {
    GithubConnection::connect(NewGithubConnection {
        id: SourceConnectionId::new(),
        organization_id,
        installation_id: GithubInstallationId::parse(installation_id)
            .map_err(std::io::Error::other)?,
        account_id: GithubAccountId::parse(account_id).map_err(std::io::Error::other)?,
        account_login: GithubLogin::parse(account_login).map_err(std::io::Error::other)?,
        account_kind,
        verified_by_user_id: GithubAccountId::parse(200).map_err(std::io::Error::other)?,
        verified_by_user_login: GithubLogin::parse("octocat").map_err(std::io::Error::other)?,
        connected_at,
    })
    .map_err(|error| std::io::Error::other(error).into())
}

fn github_test_digest(label: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(label.as_bytes()))
}
