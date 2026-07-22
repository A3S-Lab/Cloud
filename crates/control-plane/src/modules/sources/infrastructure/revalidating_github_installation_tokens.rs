use crate::modules::sources::domain::{
    GithubConnectionAuthorityError, GithubConnectionAuthorityRequest, GithubInstallationTokenError,
    GithubInstallationTokenRequest, IGithubConnectionAuthorityService,
    IGithubInstallationTokenService, SourceProviderCredential,
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct RevalidatingGithubInstallationTokens {
    authority: Arc<dyn IGithubConnectionAuthorityService>,
    issuer: Arc<dyn IGithubInstallationTokenService>,
}

impl RevalidatingGithubInstallationTokens {
    pub fn new(
        authority: Arc<dyn IGithubConnectionAuthorityService>,
        issuer: Arc<dyn IGithubInstallationTokenService>,
    ) -> Self {
        Self { authority, issuer }
    }
}

#[async_trait]
impl IGithubInstallationTokenService for RevalidatingGithubInstallationTokens {
    async fn issue(
        &self,
        request: GithubInstallationTokenRequest,
    ) -> Result<SourceProviderCredential, GithubInstallationTokenError> {
        let connection = self
            .authority
            .require_current(GithubConnectionAuthorityRequest {
                organization_id: request.organization_id,
                connection_id: request.connection_id,
                checked_at: request.requested_at,
            })
            .await
            .map_err(map_authority_error)?;
        if connection.organization_id != request.organization_id
            || connection.id != request.connection_id
            || connection.installation_id != request.installation_id
            || !connection.is_authoritative()
        {
            return Err(GithubInstallationTokenError::Forbidden);
        }
        self.issuer.issue(request).await
    }
}

fn map_authority_error(error: GithubConnectionAuthorityError) -> GithubInstallationTokenError {
    match error {
        GithubConnectionAuthorityError::NotFound | GithubConnectionAuthorityError::Forbidden => {
            GithubInstallationTokenError::Forbidden
        }
        GithubConnectionAuthorityError::Unavailable => GithubInstallationTokenError::Unavailable,
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
    use chrono::{Duration, Utc};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use zeroize::Zeroizing;

    struct AuthorityStub {
        result: Result<GithubConnection, GithubConnectionAuthorityError>,
    }

    #[async_trait]
    impl IGithubConnectionAuthorityService for AuthorityStub {
        async fn require_current(
            &self,
            _request: GithubConnectionAuthorityRequest,
        ) -> Result<GithubConnection, GithubConnectionAuthorityError> {
            self.result.clone()
        }
    }

    #[derive(Default)]
    struct IssuerStub {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl IGithubInstallationTokenService for IssuerStub {
        async fn issue(
            &self,
            request: GithubInstallationTokenRequest,
        ) -> Result<SourceProviderCredential, GithubInstallationTokenError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            SourceProviderCredential::new(
                &request.repository,
                Zeroizing::new("fixture-token".into()),
                request.requested_at,
                request.requested_at + Duration::hours(1),
            )
            .map_err(GithubInstallationTokenError::Protocol)
        }
    }

    #[tokio::test]
    async fn provider_authority_is_required_before_credential_issuance() {
        let connection = connection();
        let issuer = Arc::new(IssuerStub::default());
        let blocked = RevalidatingGithubInstallationTokens::new(
            Arc::new(AuthorityStub {
                result: Err(GithubConnectionAuthorityError::Forbidden),
            }),
            issuer.clone(),
        );
        assert!(matches!(
            blocked.issue(request(&connection)).await,
            Err(GithubInstallationTokenError::Forbidden)
        ));
        assert_eq!(issuer.calls.load(Ordering::SeqCst), 0);

        let allowed = RevalidatingGithubInstallationTokens::new(
            Arc::new(AuthorityStub {
                result: Ok(connection.clone()),
            }),
            issuer.clone(),
        );
        let credential = allowed
            .issue(request(&connection))
            .await
            .expect("credential");
        assert!(credential.authorizes(
            &repository(),
            connection.connected_at,
            Duration::minutes(1)
        ));
        assert_eq!(issuer.calls.load(Ordering::SeqCst), 1);
    }

    fn connection() -> GithubConnection {
        GithubConnection::connect(NewGithubConnection {
            id: SourceConnectionId::new(),
            organization_id: OrganizationId::new(),
            installation_id: GithubInstallationId::parse(42).expect("installation ID"),
            account_id: GithubAccountId::parse(100).expect("account ID"),
            account_login: GithubLogin::parse("A3S-Lab").expect("account login"),
            account_kind: GithubAccountKind::Organization,
            verified_by_user_id: GithubAccountId::parse(200).expect("user ID"),
            verified_by_user_login: GithubLogin::parse("octocat").expect("user login"),
            connected_at: Utc::now(),
        })
        .expect("connection")
    }

    fn request(connection: &GithubConnection) -> GithubInstallationTokenRequest {
        GithubInstallationTokenRequest {
            organization_id: connection.organization_id,
            connection_id: connection.id,
            installation_id: connection.installation_id,
            repository: repository(),
            requested_at: connection.connected_at,
        }
    }

    fn repository() -> GitRepository {
        GitRepository::parse(
            GitProvider::Github,
            "https://github.com/a3s-lab/private-cloud",
        )
        .expect("repository")
    }
}
