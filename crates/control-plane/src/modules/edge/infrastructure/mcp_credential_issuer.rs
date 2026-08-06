use crate::modules::edge::domain::services::{
    validate_mcp_credential_lifetime, IMcpCredentialIssuer, IssuedMcpCredential,
    McpCredentialIssuanceError, McpCredentialIssueRequest,
};
use crate::modules::edge::domain::McpCredential;
use crate::modules::shared_kernel::domain::{canonical_timestamp, McpCredentialId};
use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::Argon2;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::Semaphore;
use zeroize::Zeroizing;

const PREFIX_RANDOM_BYTES: usize = 8;
const SECRET_RANDOM_BYTES: usize = 32;
const SALT_RANDOM_BYTES: usize = 16;
const MAX_CONCURRENT_HASHES: usize = 4;

/// Generates Cloud-owned hosted MCP bearer material without persisting it.
///
/// The application layer owns the atomic credential, encrypted delivery
/// receipt, idempotency, outbox, and audit transaction. Keeping this provider
/// generation-only prevents a second persistence path from bypassing that
/// transaction.
#[derive(Clone)]
pub struct McpCredentialIssuer {
    hashing_permits: Arc<Semaphore>,
}

impl McpCredentialIssuer {
    pub fn new() -> Self {
        Self {
            hashing_permits: Arc::new(Semaphore::new(MAX_CONCURRENT_HASHES)),
        }
    }

    async fn generate_material(
        &self,
    ) -> Result<(String, Zeroizing<String>, String), McpCredentialIssuanceError> {
        let (prefix, secret, salt) = random_material()?;
        let permit = Arc::clone(&self.hashing_permits)
            .try_acquire_owned()
            .map_err(|_| McpCredentialIssuanceError::Unavailable)?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let verifier_hash = Argon2::default()
                .hash_password(secret.as_bytes(), &salt)
                .map_err(|_| McpCredentialIssuanceError::Unavailable)?
                .to_string();
            Ok((prefix, secret, verifier_hash))
        })
        .await
        .map_err(|_| McpCredentialIssuanceError::Unavailable)?
    }
}

impl Default for McpCredentialIssuer {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IMcpCredentialIssuer for McpCredentialIssuer {
    async fn issue(
        &self,
        request: McpCredentialIssueRequest,
    ) -> Result<IssuedMcpCredential, McpCredentialIssuanceError> {
        let request = request.canonicalize()?;
        let (prefix, secret, verifier_hash) = self.generate_material().await?;
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
        Ok(IssuedMcpCredential::new(credential, secret))
    }

    async fn rotate(
        &self,
        mut credential: McpCredential,
        expires_at: DateTime<Utc>,
        rotated_at: DateTime<Utc>,
    ) -> Result<IssuedMcpCredential, McpCredentialIssuanceError> {
        let rotated_at = canonical_timestamp(rotated_at);
        let expires_at = canonical_timestamp(expires_at);
        validate_mcp_credential_lifetime(rotated_at, expires_at)?;
        let (prefix, secret, verifier_hash) = self.generate_material().await?;
        credential
            .rotate(prefix, verifier_hash, expires_at, rotated_at)
            .map_err(McpCredentialIssuanceError::InvalidRequest)?;
        Ok(IssuedMcpCredential::new(credential, secret))
    }
}

fn random_material() -> Result<(String, Zeroizing<String>, SaltString), McpCredentialIssuanceError>
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
    use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    use chrono::{Duration, TimeZone};

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
    async fn generates_one_redacted_gateway_compatible_secret_without_persistence() {
        let issued = McpCredentialIssuer::new()
            .issue(request())
            .await
            .expect("issue");
        let debug = format!("{issued:?}");
        let (credential, secret) = issued.into_parts();

        assert_eq!(credential.prefix().len(), 24);
        assert_eq!(secret.len(), 88);
        assert!(secret.starts_with(credential.prefix()));
        assert!(!debug.contains(secret.as_str()));
        assert!(debug.contains("<redacted>"));
        let verifier = credential.gateway_projection();
        let verifier = PasswordHash::new(verifier.verifier_hash()).expect("verifier");
        assert!(Argon2::default()
            .verify_password(secret.as_bytes(), &verifier)
            .is_ok());
    }

    #[tokio::test]
    async fn rotation_replaces_material_and_advances_the_existing_identity() {
        let issuer = McpCredentialIssuer::new();
        let issued = issuer.issue(request()).await.expect("issue");
        let (credential, first_secret) = issued.into_parts();
        let credential_id = credential.id;
        let rotated = issuer
            .rotate(
                credential,
                now() + Duration::days(60),
                now() + Duration::minutes(1),
            )
            .await
            .expect("rotate");
        let (rotated, second_secret) = rotated.into_parts();

        assert_eq!(rotated.id, credential_id);
        assert_eq!(rotated.generation(), 2);
        assert_eq!(rotated.aggregate_version(), 2);
        assert_ne!(first_secret.as_str(), second_secret.as_str());
        let verifier = rotated.gateway_projection();
        let verifier = PasswordHash::new(verifier.verifier_hash()).expect("verifier");
        assert!(Argon2::default()
            .verify_password(second_secret.as_bytes(), &verifier)
            .is_ok());
    }

    #[tokio::test]
    async fn rejects_invalid_or_unbounded_lifetimes_before_hashing() {
        let issuer = McpCredentialIssuer::new();
        let mut invalid = request();
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
    }
}
