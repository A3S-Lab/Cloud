use crate::modules::artifacts::application::CancelBuildRunResult;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelBuildRunResponse {
    pub build_run_id: Uuid,
    pub operation_id: Uuid,
    pub status: String,
    pub cancellation_requested_at: Option<chrono::DateTime<chrono::Utc>>,
    pub replayed: bool,
}

impl From<CancelBuildRunResult> for CancelBuildRunResponse {
    fn from(result: CancelBuildRunResult) -> Self {
        Self {
            build_run_id: result.build_run.id.as_uuid(),
            operation_id: result.build_run.operation_id.as_uuid(),
            status: result.build_run.status.as_str().into(),
            cancellation_requested_at: result.build_run.cancellation_requested_at,
            replayed: result.replayed,
        }
    }
}
