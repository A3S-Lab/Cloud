use crate::modules::inference::domain::services::{
    InferenceUsageDailyRollup, InferenceUsageRequestFact, InferenceUsageRetentionReport,
    InferenceUsageRetentionState, InferenceUsageRetentionSweep,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeId, OrganizationId, RepositoryError,
};
use a3s_cloud_contracts::{InferenceUsageBatchV1, InferenceUsageReceiptV1};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};

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

    async fn list_daily_rollups(
        &self,
        organization_id: OrganizationId,
        environment_id: EnvironmentId,
        from_day: NaiveDate,
        to_day: NaiveDate,
    ) -> Result<Vec<InferenceUsageDailyRollup>, RepositoryError>;

    async fn get_request_fact(
        &self,
        organization_id: OrganizationId,
        request_id: uuid::Uuid,
    ) -> Result<Option<InferenceUsageRequestFact>, RepositoryError>;

    async fn retention_available_from(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<DateTime<Utc>>, RepositoryError>;

    async fn retention_state(
        &self,
        organization_id: OrganizationId,
    ) -> Result<InferenceUsageRetentionState, RepositoryError>;

    async fn sweep_retention(
        &self,
        sweep: InferenceUsageRetentionSweep,
    ) -> Result<InferenceUsageRetentionReport, RepositoryError>;
}

pub const INFERENCE_USAGE_REPOSITORY: &str = "INFERENCE_USAGE_REPOSITORY";
