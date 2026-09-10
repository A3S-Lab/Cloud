use crate::modules::edge::application::McpCredentialDeliveryResult;
use crate::modules::edge::domain::repositories::McpCredentialWrite;
use crate::modules::edge::domain::{McpCredential, McpCredentialDeliveryReceipt};
use crate::modules::shared_kernel::application::ApplicationResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Edge-owned encryption boundary for MCP credential delivery receipts.
///
/// Secrets remains authoritative for key material and ciphertext encoding.
/// Edge Application handlers never import the Secrets encryption service trait.
#[async_trait]
pub trait IEdgeMcpCredentialEncryption: Send + Sync {
    async fn encrypt_delivery_receipt(
        &self,
        credential: &McpCredential,
        bearer_credential: &str,
    ) -> ApplicationResult<McpCredentialDeliveryReceipt>;

    async fn recover_delivery(
        &self,
        write: McpCredentialWrite,
        observed_at: DateTime<Utc>,
    ) -> ApplicationResult<McpCredentialDeliveryResult>;
}
