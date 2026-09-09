//! Generates Identity-owned inference bearer material without persisting it.

use crate::modules::identity::domain::entities::InferenceCredential;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, InferenceCredentialId, OrganizationId, ProjectId,
};
use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::Argon2;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::Semaphore;
use zeroize::Zeroizing;

const PREFIX_RANDOM_BYTES: usize = 8;
const SECRET_RANDOM_BYTES: usize = 32;
const SALT_RANDOM_BYTES: usize = 16;
const MAX_CONCURRENT_HASHES: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceCredentialIssueRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub expires_at: DateTime<Utc>,
    pub issued_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct IssuedInferenceCredential {
    pub credential: InferenceCredential,
    pub secret: Zeroizing<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceCredentialIssuanceError {
    InvalidRequest(String),
    Unavailable,
}

impl std::fmt::Display for InferenceCredentialIssuanceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(formatter, "{message}"),
            Self::Unavailable => write!(formatter, "inference credential issuer is unavailable"),
        }
    }
}

impl std::error::Error for InferenceCredentialIssuanceError {}

#[derive(Clone)]
pub struct InferenceCredentialIssuer {
    hashing_permits: Arc<Semaphore>,
}

impl InferenceCredentialIssuer {
    pub fn new() -> Self {
        Self {
            hashing_permits: Arc::new(Semaphore::new(MAX_CONCURRENT_HASHES)),
        }
    }

    pub async fn issue(
        &self,
        request: InferenceCredentialIssueRequest,
    ) -> Result<IssuedInferenceCredential, InferenceCredentialIssuanceError> {
        let issued_at = canonical_timestamp(request.issued_at);
        let expires_at = canonical_timestamp(request.expires_at);
        if expires_at <= issued_at {
            return Err(InferenceCredentialIssuanceError::InvalidRequest(
                "inference credential expiry must be after issuance".into(),
            ));
        }
        let (prefix, secret, verifier_hash) = self.generate_material().await?;
        let credential = InferenceCredential::issue(
            InferenceCredentialId::new(),
            request.organization_id,
            request.project_id,
            request.environment_id,
            prefix,
            verifier_hash,
            expires_at,
            issued_at,
        )
        .map_err(InferenceCredentialIssuanceError::InvalidRequest)?;
        Ok(IssuedInferenceCredential { credential, secret })
    }

    async fn generate_material(
        &self,
    ) -> Result<(String, Zeroizing<String>, String), InferenceCredentialIssuanceError> {
        let (prefix, secret, salt) = random_material()?;
        let permit = Arc::clone(&self.hashing_permits)
            .try_acquire_owned()
            .map_err(|_| InferenceCredentialIssuanceError::Unavailable)?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let verifier_hash = Argon2::default()
                .hash_password(secret.as_bytes(), &salt)
                .map_err(|_| InferenceCredentialIssuanceError::Unavailable)?
                .to_string();
            Ok((prefix, secret, verifier_hash))
        })
        .await
        .map_err(|_| InferenceCredentialIssuanceError::Unavailable)?
    }
}

impl Default for InferenceCredentialIssuer {
    fn default() -> Self {
        Self::new()
    }
}

fn random_material(
) -> Result<(String, Zeroizing<String>, SaltString), InferenceCredentialIssuanceError> {
    let mut prefix_random = Zeroizing::new([0_u8; PREFIX_RANDOM_BYTES]);
    let mut secret_random = Zeroizing::new([0_u8; SECRET_RANDOM_BYTES]);
    let mut salt_random = Zeroizing::new([0_u8; SALT_RANDOM_BYTES]);
    getrandom::fill(&mut *prefix_random).map_err(|_| InferenceCredentialIssuanceError::Unavailable)?;
    getrandom::fill(&mut *secret_random).map_err(|_| InferenceCredentialIssuanceError::Unavailable)?;
    getrandom::fill(&mut *salt_random).map_err(|_| InferenceCredentialIssuanceError::Unavailable)?;

    let mut prefix = String::with_capacity("a3s_inf_".len() + PREFIX_RANDOM_BYTES * 2);
    prefix.push_str("a3s_inf_");
    push_lower_hex(&mut prefix, &prefix_random[..]);
    let mut secret = Zeroizing::new(String::with_capacity(
        prefix.len() + SECRET_RANDOM_BYTES * 2,
    ));
    secret.push_str(&prefix);
    push_lower_hex(&mut secret, &secret_random[..]);
    let salt = SaltString::encode_b64(&salt_random[..])
        .map_err(|_| InferenceCredentialIssuanceError::Unavailable)?;
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
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    use chrono::Duration;

    #[tokio::test]
    async fn issues_a3s_inf_prefix_and_verifiable_argon2id_secret() {
        let issuer = InferenceCredentialIssuer::new();
        let issued_at = Utc::now();
        let issued = issuer
            .issue(InferenceCredentialIssueRequest {
                organization_id: OrganizationId::new(),
                project_id: ProjectId::new(),
                environment_id: EnvironmentId::new(),
                expires_at: issued_at + Duration::hours(1),
                issued_at,
            })
            .await
            .unwrap();
        assert!(issued.credential.prefix().starts_with("a3s_inf_"));
        assert_eq!(issued.credential.prefix().len(), "a3s_inf_".len() + 16);
        let projection = issued.credential.gateway_projection().unwrap();
        let hash = PasswordHash::new(projection.verifier_hash()).unwrap();
        Argon2::default()
            .verify_password(issued.secret.as_bytes(), &hash)
            .unwrap();
        assert!(issued.secret.starts_with(issued.credential.prefix()));
    }
}
