use crate::modules::audit::domain::{
    AuditRetentionPolicy, AuditRetentionStatus, IAuditRecordRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetAuditRetentionStatus {
    pub organization_id: OrganizationId,
}

impl Query for GetAuditRetentionStatus {
    type Output = ApplicationResult<AuditRetentionStatus>;
}

pub struct GetAuditRetentionStatusHandler {
    repository: Arc<dyn IAuditRecordRepository>,
    policy: AuditRetentionPolicy,
}

impl GetAuditRetentionStatusHandler {
    pub fn new(repository: Arc<dyn IAuditRecordRepository>, policy: AuditRetentionPolicy) -> Self {
        Self { repository, policy }
    }
}

impl QueryHandler<GetAuditRetentionStatus> for GetAuditRetentionStatusHandler {
    fn execute(
        &self,
        query: GetAuditRetentionStatus,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AuditRetentionStatus>>>
    {
        let repository = Arc::clone(&self.repository);
        let policy = self.policy.clone();
        Box::pin(async move {
            let state = match repository.retention_state(query.organization_id).await {
                Ok(state) => state,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(
                AuditRetentionStatus::from_state(&policy, state)
                    .map_err(ApplicationError::Internal),
            )
        })
    }
}
