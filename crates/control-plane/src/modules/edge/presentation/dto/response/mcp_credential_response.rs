use crate::modules::edge::application::{McpCredentialMutationResult, McpCredentialSecret};
use crate::modules::edge::domain::McpCredential;
use chrono::{DateTime, Utc};
use serde::ser::Serializer;
use serde::Serialize;
use std::fmt;
use uuid::Uuid;

/// Public metadata for a Cloud-owned MCP service credential.
///
/// This type deliberately has no verifier, encrypted delivery, or encryption
/// key fields, so those values cannot accidentally enter the REST contract.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpCredentialResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub prefix: String,
    pub generation: u64,
    pub aggregate_version: u64,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<McpCredential> for McpCredentialResponse {
    fn from(credential: McpCredential) -> Self {
        Self {
            id: credential.id.as_uuid(),
            organization_id: credential.organization_id.as_uuid(),
            project_id: credential.project_id.as_uuid(),
            environment_id: credential.environment_id.as_uuid(),
            prefix: credential.prefix().to_owned(),
            generation: credential.generation(),
            aggregate_version: credential.aggregate_version(),
            expires_at: credential.expires_at(),
            created_at: credential.created_at(),
            updated_at: credential.updated_at(),
            revoked_at: credential.revoked_at(),
        }
    }
}

/// Mutation response that owns the zeroizing one-time secret until JSON
/// serialization completes. The secret is omitted entirely for revocation.
pub struct McpCredentialMutationResponse {
    credential: McpCredentialResponse,
    secret: Option<McpCredentialSecret>,
    replayed: bool,
}

impl From<McpCredentialMutationResult> for McpCredentialMutationResponse {
    fn from(result: McpCredentialMutationResult) -> Self {
        let (credential, secret, replayed) = result.into_parts();
        Self {
            credential: credential.into(),
            secret,
            replayed,
        }
    }
}

impl Serialize for McpCredentialMutationResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Response<'a> {
            #[serde(flatten)]
            credential: &'a McpCredentialResponse,
            #[serde(skip_serializing_if = "Option::is_none")]
            secret: Option<&'a str>,
            replayed: bool,
        }

        Response {
            credential: &self.credential,
            secret: self.secret.as_ref().map(McpCredentialSecret::expose),
            replayed: self.replayed,
        }
        .serialize(serializer)
    }
}

impl fmt::Debug for McpCredentialMutationResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("McpCredentialMutationResponse")
            .field("credential", &self.credential)
            .field(
                "secret",
                &self.secret.as_ref().map(|_| "<redacted-mcp-secret>"),
            )
            .field("replayed", &self.replayed)
            .finish()
    }
}
