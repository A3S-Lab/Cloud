use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, WorkloadId};
use crate::modules::workloads::domain::repositories::WorkloadStopBundle;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct StopWorkload {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for StopWorkload {
    type Output = ApplicationResult<StopWorkloadResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StopWorkloadResult {
    pub bundle: WorkloadStopBundle,
}
