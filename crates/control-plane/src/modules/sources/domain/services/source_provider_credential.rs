use crate::modules::shared_kernel::domain::canonical_timestamp;
use crate::modules::sources::domain::{GitProvider, GitRepository};
use chrono::{DateTime, Duration, Utc};
use std::fmt;
use zeroize::Zeroizing;

const MINIMUM_LIFETIME: Duration = Duration::minutes(1);
const MAXIMUM_LIFETIME: Duration = Duration::minutes(65);

pub struct SourceProviderCredential {
    provider: GitProvider,
    repository_identity: String,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    token: Zeroizing<String>,
}

impl SourceProviderCredential {
    pub fn new(
        repository: &GitRepository,
        token: Zeroizing<String>,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let issued_at = canonical_timestamp(issued_at);
        let expires_at = canonical_timestamp(expires_at);
        if token.is_empty()
            || token.len() > 2048
            || !token.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err("source provider credential value is invalid".into());
        }
        let lifetime = expires_at.signed_duration_since(issued_at);
        if lifetime < MINIMUM_LIFETIME || lifetime > MAXIMUM_LIFETIME {
            return Err("source provider credential lifetime is invalid".into());
        }
        Ok(Self {
            provider: repository.provider(),
            repository_identity: repository.identity().into(),
            issued_at,
            expires_at,
            token,
        })
    }

    pub const fn provider(&self) -> GitProvider {
        self.provider
    }

    pub fn repository_identity(&self) -> &str {
        &self.repository_identity
    }

    pub const fn issued_at(&self) -> DateTime<Utc> {
        self.issued_at
    }

    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    pub fn authorizes(
        &self,
        repository: &GitRepository,
        now: DateTime<Utc>,
        minimum_remaining: Duration,
    ) -> bool {
        minimum_remaining >= Duration::zero()
            && self.provider == repository.provider()
            && self.repository_identity == repository.identity()
            && canonical_timestamp(now) >= self.issued_at
            && canonical_timestamp(now)
                .checked_add_signed(minimum_remaining)
                .is_some_and(|required_until| required_until < self.expires_at)
    }

    pub(crate) fn expose_token(&self) -> &str {
        self.token.as_str()
    }
}

impl fmt::Debug for SourceProviderCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SourceProviderCredential")
            .field("provider", &self.provider)
            .field("repository_identity", &self.repository_identity)
            .field("issued_at", &self.issued_at)
            .field("expires_at", &self.expires_at)
            .field("token", &"<redacted>")
            .finish()
    }
}
