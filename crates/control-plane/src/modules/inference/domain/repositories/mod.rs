use crate::modules::shared_kernel::domain::{NodeId, OrganizationId, RepositoryError};
use a3s_cloud_contracts::{InferenceUsageBatchV1, InferenceUsageReceiptV1};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Authenticated write of one Gateway usage batch into the Inference ledger.
#[derive(Debug, Clone)]
pub struct AcceptInferenceUsageBatchWrite {
    pub organization_id: OrganizationId,
    pub authenticated_node_id: NodeId,
    pub batch: InferenceUsageBatchV1,
    pub accepted_at: DateTime<Utc>,
}

impl AcceptInferenceUsageBatchWrite {
    pub fn new(
        organization_id: OrganizationId,
        authenticated_node_id: NodeId,
        batch: InferenceUsageBatchV1,
        accepted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let write = Self {
            organization_id,
            authenticated_node_id,
            batch,
            accepted_at,
        };
        write.validate()?;
        Ok(write)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.batch.validate()?;
        if self.organization_id.as_uuid().is_nil() || self.authenticated_node_id.as_uuid().is_nil()
        {
            return Err("inference usage batch write identity is invalid".into());
        }
        Ok(())
    }
}

#[async_trait]
pub trait IInferenceUsageRepository: Send + Sync {
    async fn accept_usage_batch(
        &self,
        write: AcceptInferenceUsageBatchWrite,
    ) -> Result<InferenceUsageReceiptV1, RepositoryError>;
}

pub const INFERENCE_USAGE_REPOSITORY: &str = "INFERENCE_USAGE_REPOSITORY";
