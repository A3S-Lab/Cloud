use crate::modules::sources::application::{
    GithubDiscoveredReference, GithubDiscoveredRepository,
    GithubRepositoryDiscoveryProviderRequest, GithubRepositoryReferenceDiscoveryProviderRequest,
    GithubSourceDiscoveryProviderError, GithubSourceDiscoveryProviderPage,
    GithubSourceDiscoveryScope, IGithubSourceDiscoveryProvider,
};
use crate::modules::sources::domain::{
    GithubConnectionAuthorityError, GithubConnectionAuthorityRequest,
    IGithubConnectionAuthorityService,
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct RevalidatingGithubSourceDiscovery {
    authority: Arc<dyn IGithubConnectionAuthorityService>,
    provider: Arc<dyn IGithubSourceDiscoveryProvider>,
}

impl RevalidatingGithubSourceDiscovery {
    pub fn new(
        authority: Arc<dyn IGithubConnectionAuthorityService>,
        provider: Arc<dyn IGithubSourceDiscoveryProvider>,
    ) -> Self {
        Self {
            authority,
            provider,
        }
    }

    async fn require_current(
        &self,
        scope: GithubSourceDiscoveryScope,
    ) -> Result<(), GithubSourceDiscoveryProviderError> {
        let connection = self
            .authority
            .require_current(GithubConnectionAuthorityRequest {
                organization_id: scope.organization_id,
                connection_id: scope.connection_id,
                checked_at: scope.requested_at,
            })
            .await
            .map_err(map_authority_error)?;
        if connection.organization_id != scope.organization_id
            || connection.id != scope.connection_id
            || connection.installation_id != scope.installation_id
            || !connection.is_authoritative()
        {
            return Err(GithubSourceDiscoveryProviderError::Forbidden);
        }
        Ok(())
    }
}

#[async_trait]
impl IGithubSourceDiscoveryProvider for RevalidatingGithubSourceDiscovery {
    async fn list_repositories(
        &self,
        request: GithubRepositoryDiscoveryProviderRequest,
    ) -> Result<
        GithubSourceDiscoveryProviderPage<GithubDiscoveredRepository>,
        GithubSourceDiscoveryProviderError,
    > {
        request
            .validate()
            .map_err(GithubSourceDiscoveryProviderError::Protocol)?;
        self.require_current(request.scope).await?;
        self.provider.list_repositories(request).await
    }

    async fn list_references(
        &self,
        request: GithubRepositoryReferenceDiscoveryProviderRequest,
    ) -> Result<
        GithubSourceDiscoveryProviderPage<GithubDiscoveredReference>,
        GithubSourceDiscoveryProviderError,
    > {
        request
            .validate()
            .map_err(GithubSourceDiscoveryProviderError::Protocol)?;
        self.require_current(request.scope).await?;
        self.provider.list_references(request).await
    }
}

fn map_authority_error(
    error: GithubConnectionAuthorityError,
) -> GithubSourceDiscoveryProviderError {
    match error {
        GithubConnectionAuthorityError::NotFound | GithubConnectionAuthorityError::Forbidden => {
            GithubSourceDiscoveryProviderError::Forbidden
        }
        GithubConnectionAuthorityError::Unavailable => {
            GithubSourceDiscoveryProviderError::Unavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{OrganizationId, SourceConnectionId};
    use crate::modules::sources::domain::{
        GitProvider, GitRepository, GithubAccountId, GithubAccountKind, GithubConnection,
        GithubInstallationId, GithubLogin, NewGithubConnection,
    };
    use chrono::Utc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct AuthorityStub {
        result: Result<GithubConnection, GithubConnectionAuthorityError>,
        calls: AtomicUsize,
    }

    impl AuthorityStub {
        fn new(result: Result<GithubConnection, GithubConnectionAuthorityError>) -> Self {
            Self {
                result,
                calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl IGithubConnectionAuthorityService for AuthorityStub {
        async fn require_current(
            &self,
            _request: GithubConnectionAuthorityRequest,
        ) -> Result<GithubConnection, GithubConnectionAuthorityError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.result.clone()
        }
    }

    #[derive(Default)]
    struct ProviderStub {
        repository_calls: AtomicUsize,
        reference_calls: AtomicUsize,
    }

    #[async_trait]
    impl IGithubSourceDiscoveryProvider for ProviderStub {
        async fn list_repositories(
            &self,
            _request: GithubRepositoryDiscoveryProviderRequest,
        ) -> Result<
            GithubSourceDiscoveryProviderPage<GithubDiscoveredRepository>,
            GithubSourceDiscoveryProviderError,
        > {
            self.repository_calls.fetch_add(1, Ordering::SeqCst);
            Ok(GithubSourceDiscoveryProviderPage {
                entries: Vec::new(),
                has_next: false,
            })
        }

        async fn list_references(
            &self,
            _request: GithubRepositoryReferenceDiscoveryProviderRequest,
        ) -> Result<
            GithubSourceDiscoveryProviderPage<GithubDiscoveredReference>,
            GithubSourceDiscoveryProviderError,
        > {
            self.reference_calls.fetch_add(1, Ordering::SeqCst);
            Ok(GithubSourceDiscoveryProviderPage {
                entries: Vec::new(),
                has_next: false,
            })
        }
    }

    #[tokio::test]
    async fn current_exact_connection_authority_precedes_every_provider_call() {
        let expected_connection = connection(42);
        let provider = Arc::new(ProviderStub::default());
        let blocked = RevalidatingGithubSourceDiscovery::new(
            Arc::new(AuthorityStub::new(Err(
                GithubConnectionAuthorityError::Forbidden,
            ))),
            provider.clone(),
        );
        assert_eq!(
            blocked
                .list_repositories(repository_request(&expected_connection))
                .await,
            Err(GithubSourceDiscoveryProviderError::Forbidden)
        );
        assert_eq!(provider.repository_calls.load(Ordering::SeqCst), 0);

        let drifted = RevalidatingGithubSourceDiscovery::new(
            Arc::new(AuthorityStub::new(Ok(connection(43)))),
            provider.clone(),
        );
        assert_eq!(
            drifted
                .list_references(reference_request(&expected_connection))
                .await,
            Err(GithubSourceDiscoveryProviderError::Forbidden)
        );
        assert_eq!(provider.reference_calls.load(Ordering::SeqCst), 0);

        let allowed = RevalidatingGithubSourceDiscovery::new(
            Arc::new(AuthorityStub::new(Ok(expected_connection.clone()))),
            provider.clone(),
        );
        allowed
            .list_repositories(repository_request(&expected_connection))
            .await
            .expect("repository discovery");
        allowed
            .list_references(reference_request(&expected_connection))
            .await
            .expect("reference discovery");
        assert_eq!(provider.repository_calls.load(Ordering::SeqCst), 1);
        assert_eq!(provider.reference_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn malformed_provider_requests_fail_before_authority_or_provider_access() {
        let connection = connection(42);
        let authority = Arc::new(AuthorityStub::new(Ok(connection.clone())));
        let provider = Arc::new(ProviderStub::default());
        let discovery = RevalidatingGithubSourceDiscovery::new(authority.clone(), provider.clone());
        let mut request = repository_request(&connection);
        request.page = 0;

        assert!(matches!(
            discovery.list_repositories(request).await,
            Err(GithubSourceDiscoveryProviderError::Protocol(_))
        ));
        assert_eq!(authority.calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.repository_calls.load(Ordering::SeqCst), 0);
    }

    fn connection(installation_id: u64) -> GithubConnection {
        GithubConnection::connect(NewGithubConnection {
            id: SourceConnectionId::new(),
            organization_id: OrganizationId::new(),
            installation_id: GithubInstallationId::parse(installation_id).expect("installation ID"),
            account_id: GithubAccountId::parse(100).expect("account ID"),
            account_login: GithubLogin::parse("A3S-Lab").expect("account login"),
            account_kind: GithubAccountKind::Organization,
            verified_by_user_id: GithubAccountId::parse(200).expect("user ID"),
            verified_by_user_login: GithubLogin::parse("octocat").expect("user login"),
            connected_at: Utc::now(),
        })
        .expect("connection")
    }

    fn scope(connection: &GithubConnection) -> GithubSourceDiscoveryScope {
        GithubSourceDiscoveryScope {
            organization_id: connection.organization_id,
            connection_id: connection.id,
            installation_id: connection.installation_id,
            requested_at: connection.connected_at,
        }
    }

    fn repository_request(
        connection: &GithubConnection,
    ) -> GithubRepositoryDiscoveryProviderRequest {
        GithubRepositoryDiscoveryProviderRequest {
            scope: scope(connection),
            page: 1,
            limit: 50,
        }
    }

    fn reference_request(
        connection: &GithubConnection,
    ) -> GithubRepositoryReferenceDiscoveryProviderRequest {
        GithubRepositoryReferenceDiscoveryProviderRequest {
            scope: scope(connection),
            repository: GitRepository::parse(
                GitProvider::Github,
                "https://github.com/a3s-lab/cloud",
            )
            .expect("repository"),
            kind: crate::modules::sources::application::GithubDiscoveredReferenceKind::Branch,
            page: 1,
            limit: 50,
        }
    }
}
