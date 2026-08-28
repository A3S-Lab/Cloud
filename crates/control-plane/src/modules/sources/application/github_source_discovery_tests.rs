use super::*;
use crate::modules::sources::domain::{
    CompleteGithubConnection, GithubAccountId, GithubAccountKind, GithubConnectionFlow,
    GithubConnectionReconciled, GithubLogin, IGithubConnectionRepository, NewGithubConnection,
};
use crate::modules::sources::InMemoryGithubConnectionRepository;
use async_trait::async_trait;
use chrono::Duration as ChronoDuration;
use std::collections::VecDeque;
use std::sync::Mutex;
use uuid::Uuid;

#[tokio::test]
async fn repository_discovery_filters_policy_and_advances_a_scope_bound_cursor() {
    let connections = Arc::new(InMemoryGithubConnectionRepository::new());
    let connection = connect(&connections).await;
    let allowed = discovered_repository("a3s-lab/allowed");
    let denied = discovered_repository("a3s-lab/denied");
    let provider = Arc::new(DiscoveryProviderFixture::with_repository_pages(vec![
        Ok(GithubSourceDiscoveryProviderPage {
            entries: vec![allowed.clone(), denied.clone()],
            has_next: true,
        }),
        Ok(GithubSourceDiscoveryProviderPage {
            entries: Vec::new(),
            has_next: false,
        }),
    ]));
    let service = GithubSourceDiscoveryQueryService::new(
        connections,
        provider.clone(),
        repository_policy(&allowed, &denied),
    );
    let requested_at = connection.connected_at + ChronoDuration::minutes(1);

    let first = service
        .list_repositories(connection.organization_id, None, 2, requested_at)
        .await
        .expect("first repository page");
    assert_eq!(first.repositories, vec![allowed]);
    let cursor = first.next_cursor.expect("next cursor");
    assert!(cursor.len() <= MAXIMUM_GITHUB_SOURCE_DISCOVERY_CURSOR_BYTES);

    let second = service
        .list_repositories(connection.organization_id, Some(&cursor), 2, requested_at)
        .await
        .expect("second repository page");
    assert!(second.repositories.is_empty());
    assert!(second.next_cursor.is_none());
    assert!(matches!(
        service
            .list_repositories(connection.organization_id, Some(&cursor), 1, requested_at,)
            .await,
        Err(ApplicationError::Invalid(_))
    ));

    let requests = provider.repository_requests.lock().expect("requests");
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].page, 1);
    assert_eq!(requests[1].page, 2);
    assert_eq!(requests[0].scope.connection_id, connection.id);
    assert_eq!(
        requests[0].scope.installation_id,
        connection.installation_id
    );
}

#[tokio::test]
async fn reference_discovery_enforces_repository_policy_and_provider_projection() {
    let connections = Arc::new(InMemoryGithubConnectionRepository::new());
    let connection = connect(&connections).await;
    let allowed = discovered_repository("a3s-lab/allowed");
    let denied = discovered_repository("a3s-lab/denied");
    let provider = Arc::new(DiscoveryProviderFixture::with_reference_pages(vec![Ok(
        GithubSourceDiscoveryProviderPage {
            entries: vec![GithubDiscoveredReference {
                kind: GithubDiscoveredReferenceKind::Branch,
                name: "main".into(),
                commit_sha: GitCommitSha::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                    .expect("commit SHA"),
                protected: Some(true),
            }],
            has_next: false,
        },
    )]));
    let service = GithubSourceDiscoveryQueryService::new(
        connections,
        provider.clone(),
        repository_policy(&allowed, &denied),
    );

    let page = service
        .list_references(
            connection.organization_id,
            allowed.repository.canonical_url(),
            "branch",
            None,
            50,
            connection.connected_at + ChronoDuration::minutes(1),
        )
        .await
        .expect("reference page");
    assert_eq!(page.repository, allowed.repository);
    assert_eq!(page.kind, GithubDiscoveredReferenceKind::Branch);
    assert_eq!(page.references[0].name, "main");
    assert!(matches!(
        service
            .list_references(
                connection.organization_id,
                denied.repository.canonical_url(),
                "branch",
                None,
                50,
                connection.connected_at + ChronoDuration::minutes(1),
            )
            .await,
        Err(ApplicationError::Forbidden(_))
    ));
    assert_eq!(
        provider.reference_requests.lock().expect("requests").len(),
        1
    );
}

#[tokio::test]
async fn provider_cannot_change_reference_kind_or_duplicate_a_page() {
    let connections = Arc::new(InMemoryGithubConnectionRepository::new());
    let connection = connect(&connections).await;
    let repository = discovered_repository("a3s-lab/allowed");
    let duplicate = GithubDiscoveredReference {
        kind: GithubDiscoveredReferenceKind::Branch,
        name: "main".into(),
        commit_sha: GitCommitSha::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .expect("commit SHA"),
        protected: Some(false),
    };
    let provider = Arc::new(DiscoveryProviderFixture::with_reference_pages(vec![Ok(
        GithubSourceDiscoveryProviderPage {
            entries: vec![duplicate.clone(), duplicate],
            has_next: false,
        },
    )]));
    let service = GithubSourceDiscoveryQueryService::new(
        connections,
        provider,
        Arc::new(
            SourceRepositoryPolicy::github(
                &[repository.repository.canonical_url().to_owned()],
                &[],
            )
            .expect("repository policy"),
        ),
    );

    assert!(matches!(
        service
            .list_references(
                connection.organization_id,
                repository.repository.canonical_url(),
                "branch",
                None,
                50,
                connection.connected_at + ChronoDuration::minutes(1),
            )
            .await,
        Err(ApplicationError::Internal(_))
    ));
}

#[test]
fn discovery_cursor_rejects_malformed_scope_page_size_and_page_number_drift() {
    let cursor = encode_next_cursor(1, 50, "scope-a").expect("cursor");
    assert_eq!(decode_cursor(Some(&cursor), 50, "scope-a"), Ok(2));
    assert!(matches!(
        decode_cursor(Some(&cursor), 25, "scope-a"),
        Err(ApplicationError::Invalid(_))
    ));
    assert!(matches!(
        decode_cursor(Some(&cursor), 50, "scope-b"),
        Err(ApplicationError::Invalid(_))
    ));
    let mut tampered = cursor;
    tampered.replace_range(..1, if tampered.starts_with('A') { "B" } else { "A" });
    assert!(matches!(
        decode_cursor(Some(&tampered), 50, "scope-a"),
        Err(ApplicationError::Invalid(_))
    ));
    let oversized_page = URL_SAFE_NO_PAD.encode(format!(
        "v1:{}:50:scope-a",
        MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_NUMBER + 1
    ));
    assert!(matches!(
        decode_cursor(Some(&oversized_page), 50, "scope-a"),
        Err(ApplicationError::Invalid(_))
    ));
}

#[tokio::test]
async fn repository_provider_cannot_exceed_the_requested_page_bound() {
    let connections = Arc::new(InMemoryGithubConnectionRepository::new());
    let connection = connect(&connections).await;
    let first = discovered_repository("a3s-lab/first");
    let second = discovered_repository("a3s-lab/second");
    let provider = Arc::new(DiscoveryProviderFixture::with_repository_pages(vec![Ok(
        GithubSourceDiscoveryProviderPage {
            entries: vec![first.clone(), second],
            has_next: false,
        },
    )]));
    let service = GithubSourceDiscoveryQueryService::new(
        connections,
        provider,
        Arc::new(
            SourceRepositoryPolicy::github(&[first.repository.canonical_url().to_owned()], &[])
                .expect("repository policy"),
        ),
    );

    assert!(matches!(
        service
            .list_repositories(
                connection.organization_id,
                None,
                1,
                connection.connected_at + ChronoDuration::minutes(1),
            )
            .await,
        Err(ApplicationError::Internal(_))
    ));
}

#[tokio::test]
async fn invalid_public_inputs_fail_before_connection_or_provider_access() {
    let provider = Arc::new(DiscoveryProviderFixture::with_repository_pages(Vec::new()));
    let service = GithubSourceDiscoveryQueryService::new(
        Arc::new(InMemoryGithubConnectionRepository::new()),
        provider.clone(),
        Arc::new(
            SourceRepositoryPolicy::github(&["https://github.com/a3s-lab/cloud".into()], &[])
                .expect("repository policy"),
        ),
    );
    let organization_id = OrganizationId::new();

    assert!(matches!(
        service
            .list_repositories(organization_id, None, 0, Utc::now())
            .await,
        Err(ApplicationError::Invalid(_))
    ));
    assert!(matches!(
        service
            .list_references(
                organization_id,
                "https://github.com/a3s-lab/cloud.git",
                "branch",
                None,
                50,
                Utc::now(),
            )
            .await,
        Err(ApplicationError::Invalid(_))
    ));
    assert!(provider
        .repository_requests
        .lock()
        .expect("repository requests")
        .is_empty());
    assert!(provider
        .reference_requests
        .lock()
        .expect("reference requests")
        .is_empty());
}

struct DiscoveryProviderFixture {
    repository_pages: Mutex<
        VecDeque<
            Result<
                GithubSourceDiscoveryProviderPage<GithubDiscoveredRepository>,
                GithubSourceDiscoveryProviderError,
            >,
        >,
    >,
    reference_pages: Mutex<
        VecDeque<
            Result<
                GithubSourceDiscoveryProviderPage<GithubDiscoveredReference>,
                GithubSourceDiscoveryProviderError,
            >,
        >,
    >,
    repository_requests: Mutex<Vec<GithubRepositoryDiscoveryProviderRequest>>,
    reference_requests: Mutex<Vec<GithubRepositoryReferenceDiscoveryProviderRequest>>,
}

impl DiscoveryProviderFixture {
    fn with_repository_pages(
        pages: Vec<
            Result<
                GithubSourceDiscoveryProviderPage<GithubDiscoveredRepository>,
                GithubSourceDiscoveryProviderError,
            >,
        >,
    ) -> Self {
        Self {
            repository_pages: Mutex::new(pages.into()),
            reference_pages: Mutex::new(VecDeque::new()),
            repository_requests: Mutex::new(Vec::new()),
            reference_requests: Mutex::new(Vec::new()),
        }
    }

    fn with_reference_pages(
        pages: Vec<
            Result<
                GithubSourceDiscoveryProviderPage<GithubDiscoveredReference>,
                GithubSourceDiscoveryProviderError,
            >,
        >,
    ) -> Self {
        Self {
            repository_pages: Mutex::new(VecDeque::new()),
            reference_pages: Mutex::new(pages.into()),
            repository_requests: Mutex::new(Vec::new()),
            reference_requests: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl IGithubSourceDiscoveryProvider for DiscoveryProviderFixture {
    async fn list_repositories(
        &self,
        request: GithubRepositoryDiscoveryProviderRequest,
    ) -> Result<
        GithubSourceDiscoveryProviderPage<GithubDiscoveredRepository>,
        GithubSourceDiscoveryProviderError,
    > {
        self.repository_requests
            .lock()
            .expect("requests")
            .push(request);
        self.repository_pages
            .lock()
            .expect("repository pages")
            .pop_front()
            .expect("repository page fixture")
    }

    async fn list_references(
        &self,
        request: GithubRepositoryReferenceDiscoveryProviderRequest,
    ) -> Result<
        GithubSourceDiscoveryProviderPage<GithubDiscoveredReference>,
        GithubSourceDiscoveryProviderError,
    > {
        self.reference_requests
            .lock()
            .expect("requests")
            .push(request);
        self.reference_pages
            .lock()
            .expect("reference pages")
            .pop_front()
            .expect("reference page fixture")
    }
}

async fn connect(repository: &Arc<InMemoryGithubConnectionRepository>) -> GithubConnection {
    let organization_id = OrganizationId::new();
    let connected_at = Utc::now();
    let flow_id = Uuid::now_v7();
    let installation_id = GithubInstallationId::parse(42).expect("installation ID");
    let installation_state = format!("sha256:{}", "a".repeat(64));
    repository
        .begin_flow(
            GithubConnectionFlow::begin(
                flow_id,
                organization_id,
                installation_state.clone(),
                connected_at - ChronoDuration::minutes(2),
                connected_at + ChronoDuration::minutes(8),
            )
            .expect("connection flow"),
        )
        .await
        .expect("begin flow");
    repository
        .prepare_oauth(
            &installation_state,
            installation_id,
            format!("sha256:{}", "b".repeat(64)),
            format!("sha256:{}", "c".repeat(64)),
            connected_at - ChronoDuration::minutes(1),
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
        GithubConnectionReconciled::envelope(&connection, Uuid::now_v7()).expect("event envelope");
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

fn discovered_repository(identity: &str) -> GithubDiscoveredRepository {
    GithubDiscoveredRepository {
        repository: GitRepository::parse(
            GitProvider::Github,
            &format!("https://github.com/{identity}"),
        )
        .expect("repository"),
        default_branch: "main".into(),
        private: true,
        fork: false,
        archived: false,
        disabled: false,
    }
}

fn repository_policy(
    allowed: &GithubDiscoveredRepository,
    denied: &GithubDiscoveredRepository,
) -> Arc<SourceRepositoryPolicy> {
    Arc::new(
        SourceRepositoryPolicy::github(
            &[
                allowed.repository.canonical_url().to_owned(),
                denied.repository.canonical_url().to_owned(),
            ],
            &[denied.repository.canonical_url().to_owned()],
        )
        .expect("repository policy"),
    )
}
