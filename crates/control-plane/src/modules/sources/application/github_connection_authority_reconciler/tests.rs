use super::*;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OrganizationId, SourceConnectionId,
};
use crate::modules::sources::domain::{
    CompleteGithubConnection, GitProvider, GithubAccountId, GithubAccountKind,
    GithubConnectionFlow, GithubConnectionLifecycleChange, GithubConnectionStatus,
    GithubInstallationAccount, GithubInstallationId, GithubLogin, GithubProviderAuthority,
    IGithubConnectionRepository, NewGithubConnection, ReconcileGithubConnectionLifecycle,
    VerifiedGithubConnectionLifecycle, WebhookDeliveryId,
};
use crate::modules::sources::InMemoryGithubConnectionRepository;
use std::collections::VecDeque;
use std::sync::Mutex;

struct ProviderFixture {
    results: Mutex<VecDeque<Result<GithubProviderAuthority, GithubInstallationAuthorityError>>>,
}

impl ProviderFixture {
    fn new(
        results: impl IntoIterator<
            Item = Result<GithubProviderAuthority, GithubInstallationAuthorityError>,
        >,
    ) -> Self {
        Self {
            results: Mutex::new(results.into_iter().collect()),
        }
    }
}

#[async_trait]
impl IGithubInstallationAuthorityProvider for ProviderFixture {
    async fn inspect(
        &self,
        _request: GithubInstallationAuthorityRequest,
    ) -> Result<GithubProviderAuthority, GithubInstallationAuthorityError> {
        self.results
            .lock()
            .map_err(|_| GithubInstallationAuthorityError::Unavailable)?
            .pop_front()
            .unwrap_or(Err(GithubInstallationAuthorityError::Unavailable))
    }
}

#[tokio::test]
async fn periodic_authority_repairs_missed_suspend_and_unsuspend() {
    let connected_at = canonical_timestamp(Utc::now());
    let repository = Arc::new(InMemoryGithubConnectionRepository::new());
    let connection = connect(&repository, connected_at).await;
    let account = account(&connection, "A3S-Platform");
    let provider = Arc::new(ProviderFixture::new([
        Ok(GithubProviderAuthority::available(
            connection.installation_id,
            account.clone(),
            true,
        )),
        Ok(GithubProviderAuthority::available(
            connection.installation_id,
            account,
            false,
        )),
    ]));
    let reconciler = reconciler(repository.clone(), provider);

    let first_at = connected_at + ChronoDuration::seconds(1);
    let first = reconciler
        .run_once(first_at, 10)
        .await
        .expect("suspension poll");
    assert_eq!(first.scanned, 1);
    assert_eq!(first.checked, 1);
    assert_eq!(first.lifecycle_changes, 1);
    assert!(first.failures.is_empty());
    let suspended = repository
        .find(connection.organization_id)
        .await
        .expect("find suspended")
        .expect("suspended connection");
    assert_eq!(suspended.status, GithubConnectionStatus::Suspended);
    assert_eq!(suspended.account_login.as_str(), "A3S-Platform");
    assert_eq!(suspended.provider_checked_at, first_at);
    assert_eq!(
        suspended.provider_next_check_at,
        first_at + ChronoDuration::minutes(5)
    );

    let second = reconciler
        .run_once(suspended.provider_next_check_at, 10)
        .await
        .expect("unsuspension poll");
    assert_eq!(second.checked, 1);
    assert_eq!(second.lifecycle_changes, 1);
    let active = repository
        .find(connection.organization_id)
        .await
        .expect("find active")
        .expect("active connection");
    assert_eq!(active.status, GithubConnectionStatus::Active);
    assert_eq!(repository.outbox_events().await.len(), 3);
}

#[tokio::test]
async fn on_demand_authority_fails_closed_and_persists_bounded_backoff() {
    let connected_at = canonical_timestamp(Utc::now());
    let repository = Arc::new(InMemoryGithubConnectionRepository::new());
    let connection = connect(&repository, connected_at).await;
    let provider = Arc::new(ProviderFixture::new([
        Err(GithubInstallationAuthorityError::Unavailable),
        Err(GithubInstallationAuthorityError::Protocol("fixture".into())),
    ]));
    let reconciler = reconciler(repository.clone(), provider);

    let first_at = connected_at + ChronoDuration::seconds(1);
    assert_eq!(
        reconciler
            .require_current(GithubConnectionAuthorityRequest {
                organization_id: connection.organization_id,
                connection_id: connection.id,
                checked_at: first_at,
            })
            .await,
        Err(GithubConnectionAuthorityError::Unavailable)
    );
    let first = repository
        .find(connection.organization_id)
        .await
        .expect("find first failure")
        .expect("connection");
    assert_eq!(first.provider_check_failures, 1);
    assert_eq!(
        first.provider_check_error,
        Some(GithubProviderCheckError::Unavailable)
    );
    assert_eq!(
        first.provider_next_check_at,
        first_at + ChronoDuration::seconds(1)
    );
    assert_eq!(first.provider_checked_at, connected_at);

    let second_at = first.provider_next_check_at;
    assert_eq!(
        reconciler
            .require_current(GithubConnectionAuthorityRequest {
                organization_id: connection.organization_id,
                connection_id: connection.id,
                checked_at: second_at,
            })
            .await,
        Err(GithubConnectionAuthorityError::Unavailable)
    );
    let second = repository
        .find(connection.organization_id)
        .await
        .expect("find second failure")
        .expect("connection");
    assert_eq!(second.provider_check_failures, 2);
    assert_eq!(
        second.provider_check_error,
        Some(GithubProviderCheckError::Protocol)
    );
    assert_eq!(
        second.provider_next_check_at,
        second_at + ChronoDuration::seconds(2)
    );
    assert_eq!(second.status, GithubConnectionStatus::Active);
}

#[tokio::test]
async fn authoritative_deletion_and_account_change_are_terminal() {
    for (authority, expected_status) in [
        (
            AuthorityCase::Deleted,
            GithubConnectionStatus::InstallationDeleted,
        ),
        (
            AuthorityCase::ChangedAccount,
            GithubConnectionStatus::AccountChanged,
        ),
    ] {
        let connected_at = canonical_timestamp(Utc::now());
        let repository = Arc::new(InMemoryGithubConnectionRepository::new());
        let connection = connect(&repository, connected_at).await;
        let observation = match authority {
            AuthorityCase::Deleted => GithubProviderAuthority::deleted(connection.installation_id),
            AuthorityCase::ChangedAccount => GithubProviderAuthority::available(
                connection.installation_id,
                GithubInstallationAccount {
                    id: GithubAccountId::parse(101).expect("changed account ID"),
                    login: GithubLogin::parse("Other-Account").expect("changed login"),
                    kind: connection.account_kind,
                },
                false,
            ),
        };
        let reconciler = reconciler(
            repository.clone(),
            Arc::new(ProviderFixture::new([Ok(observation)])),
        );
        assert_eq!(
            reconciler
                .require_current(GithubConnectionAuthorityRequest {
                    organization_id: connection.organization_id,
                    connection_id: connection.id,
                    checked_at: connected_at + ChronoDuration::seconds(1),
                })
                .await,
            Err(GithubConnectionAuthorityError::Forbidden)
        );
        let terminal = repository
            .find(connection.organization_id)
            .await
            .expect("find terminal")
            .expect("terminal connection");
        assert_eq!(terminal.status, expected_status);
        assert!(!terminal.blocks_reconnection());
    }
}

#[tokio::test]
async fn provider_repairs_an_unconfirmed_delayed_terminal_webhook() {
    let connected_at = canonical_timestamp(Utc::now());
    let repository = Arc::new(InMemoryGithubConnectionRepository::new());
    let connection = connect(&repository, connected_at).await;
    let lifecycle_at = connected_at + ChronoDuration::seconds(1);
    repository
        .reconcile_lifecycle(ReconcileGithubConnectionLifecycle {
            lifecycle: VerifiedGithubConnectionLifecycle {
                provider: GitProvider::Github,
                delivery_id: WebhookDeliveryId::parse("delayed-deletion").expect("delivery ID"),
                change: GithubConnectionLifecycleChange::InstallationDeleted {
                    installation_id: connection.installation_id,
                    account: account(&connection, "A3S-Lab"),
                },
                payload_digest: format!("sha256:{}", "d".repeat(64)),
            },
            correlation_id: Uuid::now_v7(),
            received_at: lifecycle_at,
        })
        .await
        .expect("delayed terminal lifecycle");
    let unconfirmed = repository
        .find(connection.organization_id)
        .await
        .expect("find unconfirmed terminal")
        .expect("connection");
    assert_eq!(
        unconfirmed.status,
        GithubConnectionStatus::InstallationDeleted
    );
    assert!(unconfirmed.needs_provider_check());

    let reconciler = reconciler(
        repository.clone(),
        Arc::new(ProviderFixture::new([Ok(
            GithubProviderAuthority::available(
                connection.installation_id,
                account(&connection, "A3S-Lab"),
                false,
            ),
        )])),
    );
    let report = reconciler
        .run_once(lifecycle_at, 10)
        .await
        .expect("authoritative repair");
    assert_eq!(report.checked, 1);
    assert_eq!(report.lifecycle_changes, 1);
    let repaired = repository
        .find(connection.organization_id)
        .await
        .expect("find repaired connection")
        .expect("connection");
    assert_eq!(repaired.status, GithubConnectionStatus::Active);
    assert_eq!(repaired.provider_checked_at, lifecycle_at);
    assert_eq!(
        repaired.provider_next_check_at,
        lifecycle_at + ChronoDuration::minutes(5)
    );
}

enum AuthorityCase {
    Deleted,
    ChangedAccount,
}

fn reconciler(
    repository: Arc<InMemoryGithubConnectionRepository>,
    provider: Arc<ProviderFixture>,
) -> GithubConnectionAuthorityReconciler {
    GithubConnectionAuthorityReconciler::new(
        repository,
        provider,
        Duration::from_millis(10),
        Duration::from_secs(300),
        Duration::from_secs(1),
        Duration::from_secs(8),
        100,
    )
    .expect("reconciler")
}

async fn connect(
    repository: &Arc<InMemoryGithubConnectionRepository>,
    connected_at: DateTime<Utc>,
) -> GithubConnection {
    let organization_id = OrganizationId::new();
    let flow_id = Uuid::now_v7();
    let installation_id = GithubInstallationId::parse(42).expect("installation ID");
    let installation_state = format!("sha256:{}", "a".repeat(64));
    repository
        .begin_flow(
            GithubConnectionFlow::begin(
                flow_id,
                organization_id,
                installation_state.clone(),
                connected_at - ChronoDuration::seconds(2),
                connected_at + ChronoDuration::minutes(10),
            )
            .expect("flow"),
        )
        .await
        .expect("begin flow");
    repository
        .prepare_oauth(
            &installation_state,
            installation_id,
            format!("sha256:{}", "b".repeat(64)),
            format!("sha256:{}", "c".repeat(64)),
            connected_at - ChronoDuration::seconds(1),
        )
        .await
        .expect("prepare OAuth");
    let connection = GithubConnection::connect(NewGithubConnection {
        id: SourceConnectionId::new(),
        organization_id,
        installation_id,
        account_id: GithubAccountId::parse(100).expect("account ID"),
        account_login: GithubLogin::parse("A3S-Lab").expect("account login"),
        account_kind: GithubAccountKind::Organization,
        verified_by_user_id: GithubAccountId::parse(200).expect("user ID"),
        verified_by_user_login: GithubLogin::parse("octocat").expect("user login"),
        connected_at,
    })
    .expect("connection");
    let event =
        GithubConnectionReconciled::envelope(&connection, Uuid::now_v7()).expect("fixture event");
    repository
        .complete(CompleteGithubConnection {
            flow_id,
            connection: connection.clone(),
            event,
            completed_at: connected_at,
        })
        .await
        .expect("complete connection");
    connection
}

fn account(connection: &GithubConnection, login: &str) -> GithubInstallationAccount {
    GithubInstallationAccount {
        id: connection.account_id,
        login: GithubLogin::parse(login).expect("account login"),
        kind: connection.account_kind,
    }
}
