use crate::modules::edge::application::{McpCredentialDeliveryResult, McpCredentialMutationResult};
use crate::modules::edge::domain::McpCredential;
use chrono::{DateTime, Utc};
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpCredentialResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub prefix: String,
    pub state: String,
    pub generation: u64,
    pub aggregate_version: u64,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<McpCredential> for McpCredentialResponse {
    fn from(credential: McpCredential) -> Self {
        let state = if credential.revoked_at().is_some() {
            "revoked"
        } else if credential.is_active_at(Utc::now()) {
            "active"
        } else {
            "expired"
        };
        Self {
            id: credential.id.as_uuid(),
            organization_id: credential.organization_id.as_uuid(),
            project_id: credential.project_id.as_uuid(),
            environment_id: credential.environment_id.as_uuid(),
            prefix: credential.prefix().into(),
            state: state.into(),
            generation: credential.generation(),
            aggregate_version: credential.aggregate_version(),
            expires_at: credential.expires_at(),
            created_at: credential.created_at(),
            updated_at: credential.updated_at(),
            revoked_at: credential.revoked_at(),
        }
    }
}

pub struct McpCredentialDeliveryResponse {
    credential: McpCredentialResponse,
    bearer_credential: Zeroizing<String>,
    delivery_expires_at: DateTime<Utc>,
    replayed: bool,
}

impl From<McpCredentialDeliveryResult> for McpCredentialDeliveryResponse {
    fn from(result: McpCredentialDeliveryResult) -> Self {
        let (credential, bearer_credential, delivery_expires_at, replayed) = result.into_parts();
        Self {
            credential: credential.into(),
            bearer_credential,
            delivery_expires_at,
            replayed,
        }
    }
}

impl Serialize for McpCredentialDeliveryResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("McpCredentialDeliveryResponse", 4)?;
        state.serialize_field("credential", &self.credential)?;
        state.serialize_field("bearerCredential", self.bearer_credential.as_str())?;
        state.serialize_field("deliveryExpiresAt", &self.delivery_expires_at)?;
        state.serialize_field("replayed", &self.replayed)?;
        state.end()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpCredentialMutationResponse {
    pub credential: McpCredentialResponse,
    pub replayed: bool,
}

impl From<McpCredentialMutationResult> for McpCredentialMutationResponse {
    fn from(result: McpCredentialMutationResult) -> Self {
        Self {
            credential: result.credential.into(),
            replayed: result.replayed,
        }
    }
}
