use crate::modules::identity::domain::value_objects::{OidcIssuer, OidcProviderKey};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OidcFlowId, OrganizationId, PrincipalId, Sha256Digest,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

pub const MIN_OIDC_FLOW_LIFETIME: Duration = Duration::minutes(1);
pub const MAX_OIDC_FLOW_LIFETIME: Duration = Duration::minutes(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OidcFlowPurpose {
    Login,
    Link,
}

impl OidcFlowPurpose {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Link => "link",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "login" => Ok(Self::Login),
            "link" => Ok(Self::Link),
            _ => Err("OIDC flow purpose must be login or link".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OidcFlow {
    pub id: OidcFlowId,
    pub organization_id: OrganizationId,
    pub provider_key: OidcProviderKey,
    pub issuer: OidcIssuer,
    pub provider_config_digest: Sha256Digest,
    pub purpose: OidcFlowPurpose,
    pub principal_id: Option<PrincipalId>,
    pub state_digest: Sha256Digest,
    pub nonce_digest: Sha256Digest,
    pub pkce_verifier_digest: Sha256Digest,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub consumed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OidcFlowError {
    #[error("OIDC flow identity is invalid")]
    InvalidIdentity,
    #[error("OIDC flow has expired")]
    Expired,
    #[error("OIDC flow has already been used")]
    Replayed,
}

impl OidcFlow {
    #[allow(clippy::too_many_arguments)]
    pub fn begin(
        id: OidcFlowId,
        organization_id: OrganizationId,
        provider_key: OidcProviderKey,
        issuer: OidcIssuer,
        provider_config_digest: Sha256Digest,
        purpose: OidcFlowPurpose,
        principal_id: Option<PrincipalId>,
        state_digest: Sha256Digest,
        nonce_digest: Sha256Digest,
        pkce_verifier_digest: Sha256Digest,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let created_at = canonical_timestamp(created_at);
        let expires_at = canonical_timestamp(expires_at);
        let lifetime = expires_at - created_at;
        if !(MIN_OIDC_FLOW_LIFETIME..=MAX_OIDC_FLOW_LIFETIME).contains(&lifetime) {
            return Err("OIDC flow lifetime must be between 1 and 15 minutes".into());
        }
        if matches!(purpose, OidcFlowPurpose::Link) != principal_id.is_some() {
            return Err("OIDC link flow must bind exactly one authenticated principal".into());
        }
        Ok(Self {
            id,
            organization_id,
            provider_key,
            issuer,
            provider_config_digest,
            purpose,
            principal_id,
            state_digest,
            nonce_digest,
            pkce_verifier_digest,
            created_at,
            expires_at,
            consumed_at: None,
        })
    }

    pub fn consume(
        &mut self,
        state_digest: &Sha256Digest,
        nonce_digest: &Sha256Digest,
        pkce_verifier_digest: &Sha256Digest,
        now: DateTime<Utc>,
    ) -> Result<(), OidcFlowError> {
        if self.consumed_at.is_some() {
            return Err(OidcFlowError::Replayed);
        }
        let now = canonical_timestamp(now);
        if now >= self.expires_at {
            return Err(OidcFlowError::Expired);
        }
        if &self.state_digest != state_digest
            || &self.nonce_digest != nonce_digest
            || &self.pkce_verifier_digest != pkce_verifier_digest
        {
            return Err(OidcFlowError::InvalidIdentity);
        }
        self.consumed_at = Some(now.max(self.created_at));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(byte: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
    }

    #[test]
    fn oidc_flow_binds_purpose_and_consumes_once() {
        let now = Utc::now();
        assert!(OidcFlow::begin(
            OidcFlowId::new(),
            OrganizationId::new(),
            OidcProviderKey::parse("workforce").expect("provider"),
            OidcIssuer::parse("https://identity.example.test").expect("issuer"),
            digest('d'),
            OidcFlowPurpose::Link,
            None,
            digest('a'),
            digest('b'),
            digest('c'),
            now,
            now + Duration::minutes(5),
        )
        .is_err());

        let mut flow = OidcFlow::begin(
            OidcFlowId::new(),
            OrganizationId::new(),
            OidcProviderKey::parse("workforce").expect("provider"),
            OidcIssuer::parse("https://identity.example.test").expect("issuer"),
            digest('d'),
            OidcFlowPurpose::Login,
            None,
            digest('a'),
            digest('b'),
            digest('c'),
            now,
            now + Duration::minutes(5),
        )
        .expect("flow");
        assert_eq!(
            flow.consume(&digest('a'), &digest('b'), &digest('c'), now),
            Ok(())
        );
        assert_eq!(
            flow.consume(&digest('a'), &digest('b'), &digest('c'), now),
            Err(OidcFlowError::Replayed)
        );
    }
}
