use crate::modules::workloads::application::StopWorkloadResult;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadStopResponse {
    pub organization_id: Uuid,
    pub workload_id: Uuid,
    pub operation_id: Uuid,
    pub desired_state: String,
    pub requested_at: DateTime<Utc>,
    pub replayed: bool,
}

impl From<StopWorkloadResult> for WorkloadStopResponse {
    fn from(result: StopWorkloadResult) -> Self {
        Self {
            organization_id: result.bundle.workload.organization_id.as_uuid(),
            workload_id: result.bundle.workload.id.as_uuid(),
            operation_id: result.bundle.operation.id.as_uuid(),
            desired_state: result.bundle.workload.desired_state.as_str().into(),
            requested_at: result.bundle.operation.requested_at,
            replayed: result.bundle.replayed,
        }
    }
}
