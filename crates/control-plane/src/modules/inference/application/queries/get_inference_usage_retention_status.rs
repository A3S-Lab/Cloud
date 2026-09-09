use crate::modules::inference::domain::{
    IInferenceUsageRepository, InferenceUsageRetentionPolicy, InferenceUsageRetentionStatus,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetInferenceUsageRetentionStatus {
    pub organization_id: OrganizationId,
}

impl Query for GetInferenceUsageRetentionStatus {
    type Output = ApplicationResult<InferenceUsageRetentionStatus>;
}

pub struct GetInferenceUsageRetentionStatusHandler {
    usage: Arc<dyn IInferenceUsageRepository>,
    policy: InferenceUsageRetentionPolicy,
}

impl GetInferenceUsageRetentionStatusHandler {
    pub fn new(
        usage: Arc<dyn IInferenceUsageRepository>,
        policy: InferenceUsageRetentionPolicy,
    ) -> Self {
        Self { usage, policy }
    }
}

impl QueryHandler<GetInferenceUsageRetentionStatus> for GetInferenceUsageRetentionStatusHandler {
    fn execute(
        &self,
        query: GetInferenceUsageRetentionStatus,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<InferenceUsageRetentionStatus>>,
    > {
        let usage = Arc::clone(&self.usage);
        let policy = self.policy.clone();
        Box::pin(async move {
            let state = match usage.retention_state(query.organization_id).await {
                Ok(state) => state,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(InferenceUsageRetentionStatus::from_state(&policy, state)
                .map_err(ApplicationError::Internal))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::inference::InMemoryInferenceUsageRepository;
    use std::time::Duration;
    use uuid::Uuid;

    #[tokio::test]
    async fn returns_configured_policy_against_initial_organization_state() {
        let usage = Arc::new(InMemoryInferenceUsageRepository::new());
        let policy =
            InferenceUsageRetentionPolicy::new(Duration::from_millis(86_400_000)).expect("policy");
        let handler = GetInferenceUsageRetentionStatusHandler::new(usage, policy.clone());
        let status = handler
            .execute(
                GetInferenceUsageRetentionStatus {
                    organization_id: OrganizationId::from_uuid(Uuid::from_u128(9)),
                },
                CqrsContext::new(a3s_boot::ModuleRef::new()),
            )
            .await
            .expect("query")
            .expect("status");
        assert_eq!(status.retention_ms, 86_400_000);
        assert_eq!(&status.policy_digest, policy.digest());
        assert!(!status.current_policy_applied);
        assert_eq!(status.version, 0);
        assert!(status.records_available_from.is_none());
    }
}
