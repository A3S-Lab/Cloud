use crate::modules::edge::domain::McpCredential;
use chrono::{DateTime, Utc};
use std::fmt;
use zeroize::Zeroizing;

pub const MCP_CREDENTIAL_DELIVERY_RECEIPT_TTL_SECONDS: i64 = 600;

pub struct McpCredentialDeliveryResult {
    pub credential: McpCredential,
    pub(crate) bearer_credential: Zeroizing<String>,
    pub delivery_expires_at: DateTime<Utc>,
    pub replayed: bool,
}

impl McpCredentialDeliveryResult {
    pub fn bearer_credential(&self) -> &str {
        self.bearer_credential.as_str()
    }

    pub fn into_parts(self) -> (McpCredential, Zeroizing<String>, DateTime<Utc>, bool) {
        (
            self.credential,
            self.bearer_credential,
            self.delivery_expires_at,
            self.replayed,
        )
    }
}

impl fmt::Debug for McpCredentialDeliveryResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("McpCredentialDeliveryResult")
            .field("credential", &self.credential)
            .field("bearer_credential", &"<redacted>")
            .field("delivery_expires_at", &self.delivery_expires_at)
            .field("replayed", &self.replayed)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct McpCredentialMutationResult {
    pub credential: McpCredential,
    pub replayed: bool,
}
