use crate::modules::edge::domain::repositories::IMcpCredentialRepository;
use crate::modules::edge::domain::McpCredential;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, McpCredentialId, OrganizationId, ProjectId, RepositoryError,
};
use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::Argon2;
use chrono::{DateTime, Duration, Utc};
use std::fmt;
use std::sync::Arc;
use tokio::sync::Semaphore;
use zeroize::Zeroizing;

const PREFIX_RANDOM_BYTES: usize = 8;
const SECRET_RANDOM_BYTES: usize = 32;
const SALT_RANDOM_BYTES: usize = 16;
const MAX_CREDENTIAL_LIFETIME_DAYS: i64 = 365;
const MAX_ISSUANCE_ATTEMPTS: usize = 4;
const MAX_CONCURRENT_HASHES: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCredentialIssueRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub expires_at: DateTime<Utc>,
    pub issued_at: DateTime<Utc>,
}

impl McpCredentialIssueRequest {
    fn canonicalize(self) -> Result<Self, McpCredentialIssuanceError> {
        let issued_at = canonical_timestamp(self.issued_at);
        let expires_at = canonical_timestamp(self.expires_at);
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || expires_at <= issued_at
            || expires_at - issued_at > Duration::days(MAX_CREDENTIAL_LIFETIME_DAYS)
        {
            return Err(McpCredentialIssuanceError::InvalidRequest(
                "scope must be non-nil and lifetime must be positive and at most 365 days".into(),
            ));
        }
        Ok(Self {
            expires_at,
            issued_at,
            ..self
        })
    }
}

/// A persisted credential plus the only owned bearer value returned by the
/// issuer.
///
/// This type is deliberately neither cloneable nor serializable. A
/// future presentation boundary must consume it to produce one credential
/// response and separately close commit-before-response recovery.
pub struct IssuedMcpCredential {
    credential: McpCredential,
    secret: Zeroizing<String>,
}

impl IssuedMcpCredential {
    pub fn credential(&self) -> &McpCredential {
        &self.credential
    }

    pub fn into_parts(self) -> (McpCredential, Zeroizing<String>) {
        (self.credential, self.secret)
    }
}

impl fmt::Debug for IssuedMcpCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IssuedMcpCredential")
            .field("credential", &self.credential)
            .field("secret", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum McpCredentialIssuanceError {
    #[error("MCP credential issuance request is invalid: {0}")]
    InvalidRequest(String),
    #[error("MCP credential issuance is temporarily unavailable")]
    Unavailable,
    #[error("MCP credential issuance exhausted its bounded identity retries")]
    IdentityCollision,
    #[error("MCP credential repository rejected issuance: {0}")]
    Repository(RepositoryError),
}

/// Generates and persists one Cloud-owned hosted MCP bearer credential.
///
/// The random bearer value is hashed on the blocking pool under a bounded
/// semaphore. Repository uniqueness is authoritative, and a collision causes
/// a completely new credential, secret, salt, and verifier to be generated.
#[derive(Clone)]
pub struct McpCredentialIssuer {
    repository: Arc<dyn IMcpCredentialRepository>,
    hashing_permits: Arc<Semaphore>,
}

impl McpCredentialIssuer {
    pub fn new(repository: Arc<dyn IMcpCredentialRepository>) -> Self {
        Self {
            repository,
            hashing_permits: Arc::new(Semaphore::new(MAX_CONCURRENT_HASHES)),
        }
    }

    pub async fn issue(
        &self,
        request: McpCredentialIssueRequest,
    ) -> Result<IssuedMcpCredential, McpCredentialIssuanceError> {
        let request = request.canonicalize()?;
        for attempt in 0..MAX_ISSUANCE_ATTEMPTS {
            let (prefix, secret, salt) = generate_material()?;
            let permit = Arc::clone(&self.hashing_permits)
                .try_acquire_owned()
                .map_err(|_| McpCredentialIssuanceError::Unavailable)?;
            let (secret, verifier_hash) = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                let verifier_hash = Argon2::default()
                    .hash_password(secret.as_bytes(), &salt)
                    .map_err(|_| ())?
                    .to_string();
                Ok::<_, ()>((secret, verifier_hash))
            })
            .await
            .map_err(|_| McpCredentialIssuanceError::Unavailable)?
            .map_err(|_| McpCredentialIssuanceError::Unavailable)?;
            let credential = McpCredential::issue(
                McpCredentialId::new(),
                request.organization_id,
                request.project_id,
                request.environment_id,
                prefix,
                verifier_hash,
                request.expires_at,
                request.issued_at,
            )
            .map_err(McpCredentialIssuanceError::InvalidRequest)?;
            match self
                .repository
                .create_mcp_credential(credential.clone())
                .await
            {
                Ok(stored) if stored == credential => {
                    return Ok(IssuedMcpCredential {
                        credential: stored,
                        secret,
                    });
                }
                Ok(_) => return Err(McpCredentialIssuanceError::Unavailable),
                Err(RepositoryError::Conflict(_)) if attempt + 1 < MAX_ISSUANCE_ATTEMPTS => {}
                Err(RepositoryError::Conflict(_)) => {
                    return Err(McpCredentialIssuanceError::IdentityCollision)
                }
                Err(error) => return Err(McpCredentialIssuanceError::Repository(error)),
            }
        }
        Err(McpCredentialIssuanceError::IdentityCollision)
    }
}

fn generate_material() -> Result<(String, Zeroizing<String>, SaltString), McpCredentialIssuanceError>
{
    let mut prefix_random = Zeroizing::new([0_u8; PREFIX_RANDOM_BYTES]);
    let mut secret_random = Zeroizing::new([0_u8; SECRET_RANDOM_BYTES]);
    let mut salt_random = Zeroizing::new([0_u8; SALT_RANDOM_BYTES]);
    getrandom::fill(&mut *prefix_random).map_err(|_| McpCredentialIssuanceError::Unavailable)?;
    getrandom::fill(&mut *secret_random).map_err(|_| McpCredentialIssuanceError::Unavailable)?;
    getrandom::fill(&mut *salt_random).map_err(|_| McpCredentialIssuanceError::Unavailable)?;

    let mut prefix = String::with_capacity("a3s_mcp_".len() + PREFIX_RANDOM_BYTES * 2);
    prefix.push_str("a3s_mcp_");
    push_lower_hex(&mut prefix, &prefix_random[..]);
    let mut secret = Zeroizing::new(String::with_capacity(
        prefix.len() + SECRET_RANDOM_BYTES * 2,
    ));
    secret.push_str(&prefix);
    push_lower_hex(&mut secret, &secret_random[..]);
    let salt = SaltString::encode_b64(&salt_random[..])
        .map_err(|_| McpCredentialIssuanceError::Unavailable)?;
    Ok((prefix, secret, salt))
}

fn push_lower_hex(output: &mut String, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::InMemoryEdgeRepository;
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    use async_trait::async_trait;
    use chrono::TimeZone;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 30, 12, 0, 0)
            .single()
            .expect("time")
    }

    fn request() -> McpCredentialIssueRequest {
        McpCredentialIssueRequest {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            expires_at: now() + Duration::days(30),
            issued_at: now(),
        }
    }

    #[tokio::test]
    async fn returns_one_redacted_secret_and_persists_only_its_verifier() {
        let repository = Arc::new(InMemoryEdgeRepository::new());
        let issued = McpCredentialIssuer::new(repository.clone())
            .issue(request())
            .await
            .expect("issue");
        let debug = format!("{issued:?}");
        let credential_id = issued.credential().id;
        let organization_id = issued.credential().organization_id;
        let (credential, secret) = issued.into_parts();

        assert_eq!(credential.prefix().len(), 24);
        assert_eq!(secret.len(), 88);
        assert!(secret.starts_with(credential.prefix()));
        assert!(secret
            .bytes()
            .skip("a3s_mcp_".len())
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        assert!(!debug.contains(secret.as_str()));
        assert!(debug.contains("<redacted>"));
        let projection = credential.gateway_projection();
        let verifier = PasswordHash::new(projection.verifier_hash()).expect("verifier");
        assert!(Argon2::default()
            .verify_password(secret.as_bytes(), &verifier)
            .is_ok());
        assert_eq!(
            repository
                .find_mcp_credential(organization_id, credential_id)
                .await
                .expect("find"),
            Some(credential)
        );
    }

    #[tokio::test]
    async fn retries_identity_collisions_with_fresh_material() {
        let repository = Arc::new(ConflictingRepository::new(1));
        let issued = McpCredentialIssuer::new(repository.clone())
            .issue(request())
            .await
            .expect("retry issuance");

        assert_eq!(repository.create_attempts.load(Ordering::SeqCst), 2);
        let (credential, secret) = issued.into_parts();
        let verifier = credential.gateway_projection();
        let verifier = PasswordHash::new(verifier.verifier_hash()).expect("verifier");
        assert!(Argon2::default()
            .verify_password(secret.as_bytes(), &verifier)
            .is_ok());
    }

    #[tokio::test]
    async fn stops_after_the_bounded_identity_collision_budget() {
        let repository = Arc::new(ConflictingRepository::new(MAX_ISSUANCE_ATTEMPTS));
        assert!(matches!(
            McpCredentialIssuer::new(repository.clone())
                .issue(request())
                .await,
            Err(McpCredentialIssuanceError::IdentityCollision)
        ));
        assert_eq!(
            repository.create_attempts.load(Ordering::SeqCst),
            MAX_ISSUANCE_ATTEMPTS
        );
    }

    #[tokio::test]
    async fn rejects_invalid_or_unbounded_lifetimes_before_persistence() {
        let repository = Arc::new(InMemoryEdgeRepository::new());
        let issuer = McpCredentialIssuer::new(repository.clone());
        let valid = request();
        let mut invalid = valid.clone();
        invalid.expires_at = invalid.issued_at + Duration::days(366);

        assert!(matches!(
            issuer.issue(invalid.clone()).await,
            Err(McpCredentialIssuanceError::InvalidRequest(_))
        ));
        invalid.expires_at = invalid.issued_at;
        assert!(matches!(
            issuer.issue(invalid).await,
            Err(McpCredentialIssuanceError::InvalidRequest(_))
        ));
        assert!(repository
            .list_mcp_credentials(
                valid.organization_id,
                valid.project_id,
                valid.environment_id,
            )
            .await
            .expect("list")
            .is_empty());
    }

    struct ConflictingRepository {
        inner: InMemoryEdgeRepository,
        conflicts: usize,
        create_attempts: AtomicUsize,
    }

    impl ConflictingRepository {
        fn new(conflicts: usize) -> Self {
            Self {
                inner: InMemoryEdgeRepository::new(),
                conflicts,
                create_attempts: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl IMcpCredentialRepository for ConflictingRepository {
        async fn create_mcp_credential(
            &self,
            credential: McpCredential,
        ) -> Result<McpCredential, RepositoryError> {
            if self.create_attempts.fetch_add(1, Ordering::SeqCst) < self.conflicts {
                return Err(RepositoryError::Conflict("injected collision".into()));
            }
            self.inner.create_mcp_credential(credential).await
        }

        async fn update_mcp_credential(
            &self,
            credential: McpCredential,
            expected_aggregate_version: u64,
        ) -> Result<McpCredential, RepositoryError> {
            self.inner
                .update_mcp_credential(credential, expected_aggregate_version)
                .await
        }

        async fn find_mcp_credential(
            &self,
            organization_id: OrganizationId,
            credential_id: McpCredentialId,
        ) -> Result<Option<McpCredential>, RepositoryError> {
            self.inner
                .find_mcp_credential(organization_id, credential_id)
                .await
        }

        async fn list_mcp_credentials(
            &self,
            organization_id: OrganizationId,
            project_id: ProjectId,
            environment_id: EnvironmentId,
        ) -> Result<Vec<McpCredential>, RepositoryError> {
            self.inner
                .list_mcp_credentials(organization_id, project_id, environment_id)
                .await
        }

        async fn resolve_mcp_credentials(
            &self,
            organization_id: OrganizationId,
            project_id: ProjectId,
            environment_id: EnvironmentId,
            credential_ids: &[McpCredentialId],
        ) -> Result<Vec<McpCredential>, RepositoryError> {
            self.inner
                .resolve_mcp_credentials(
                    organization_id,
                    project_id,
                    environment_id,
                    credential_ids,
                )
                .await
        }
    }
}
