use crate::modules::artifacts::application::RetryBuildRunResult;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryBuildRunResponse {
    pub build_run_id: Uuid,
    pub operation_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_revision_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_release_id: Option<Uuid>,
    pub attempt: u32,
    pub retry_of_build_run_id: Uuid,
    pub status: String,
    pub replayed: bool,
}

impl From<RetryBuildRunResult> for RetryBuildRunResponse {
    fn from(result: RetryBuildRunResult) -> Self {
        Self {
            build_run_id: result.build_run.id.as_uuid(),
            operation_id: result.build_run.operation_id.as_uuid(),
            source_revision_id: result.build_run.source_revision_id().map(|id| id.as_uuid()),
            asset_release_id: result.build_run.asset_release_id().map(|id| id.as_uuid()),
            attempt: result.build_run.attempt,
            retry_of_build_run_id: result.retry_of_build_run_id.as_uuid(),
            status: result.build_run.status.as_str().into(),
            replayed: result.replayed,
        }
    }
}
