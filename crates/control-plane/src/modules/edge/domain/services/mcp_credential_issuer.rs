use crate::modules::edge::domain::McpCredential;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, OrganizationId, ProjectId,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::fmt;
use zeroize::Zeroizing;

pub const MAX_MCP_CREDENTIAL_LIFETIME_DAYS: i64 = 365;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCredentialIssueRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub expires_at: DateTime<Utc>,
    pub issued_at: DateTime<Utc>,
}

impl McpCredentialIssueRequest {
    pub fn canonicalize(self) -> Result<Self, McpCredentialIssuanceError> {
        let issued_at = canonical_timestamp(self.issued_at);
        let expires_at = canonical_timestamp(self.expires_at);
        validate_lifetime(issued_at, expires_at)?;
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
        {
            return Err(McpCredentialIssuanceError::InvalidRequest(
                "scope identities must be non-nil".into(),
            ));
        }
        Ok(Self {
            expires_at,
            issued_at,
            ..self
        })
    }
}

pub struct IssuedMcpCredential {
    credential: McpCredential,
    secret: Zeroizing<String>,
}

impl IssuedMcpCredential {
    pub fn new(credential: McpCredential, secret: Zeroizing<String>) -> Self {
        Self { credential, secret }
    }

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
}

#[async_trait]
pub trait IMcpCredentialIssuer: Send + Sync {
    async fn issue(
        &self,
        request: McpCredentialIssueRequest,
    ) -> Result<IssuedMcpCredential, McpCredentialIssuanceError>;

    async fn rotate(
        &self,
        credential: McpCredential,
        expires_at: DateTime<Utc>,
        rotated_at: DateTime<Utc>,
    ) -> Result<IssuedMcpCredential, McpCredentialIssuanceError>;
}

pub fn validate_lifetime(
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
) -> Result<(), McpCredentialIssuanceError> {
    if expires_at <= issued_at
        || expires_at - issued_at > Duration::days(MAX_MCP_CREDENTIAL_LIFETIME_DAYS)
    {
        return Err(McpCredentialIssuanceError::InvalidRequest(
            "lifetime must be positive and at most 365 days".into(),
        ));
    }
    Ok(())
}
