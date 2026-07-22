use crate::modules::artifacts::application::BuildRunLogPage;
use crate::modules::fleet::NodeLogRecordResponse;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildRunLogsResponse {
    pub build_run_id: Uuid,
    pub operation_id: Uuid,
    pub generation: u64,
    pub records: Vec<NodeLogRecordResponse>,
    pub next_cursor: Option<String>,
}

impl From<BuildRunLogPage> for BuildRunLogsResponse {
    fn from(page: BuildRunLogPage) -> Self {
        Self {
            build_run_id: page.build_run_id.as_uuid(),
            operation_id: page.operation_id.as_uuid(),
            generation: page.generation,
            records: page.records.into_iter().map(Into::into).collect(),
            next_cursor: page
                .next_after_sequence
                .map(|sequence| format!("v1:{sequence}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{BuildRunId, OperationId};

    #[test]
    fn build_log_response_hides_runtime_placement_identity() {
        let response = BuildRunLogsResponse::from(BuildRunLogPage {
            build_run_id: BuildRunId::new(),
            operation_id: OperationId::new(),
            generation: 1,
            records: Vec::new(),
            next_after_sequence: None,
        });
        let encoded = serde_json::to_value(response).expect("build logs response");
        assert!(encoded.get("buildRunId").is_some());
        assert!(encoded.get("operationId").is_some());
        assert!(encoded.get("nodeId").is_none());
        assert!(encoded.get("unitId").is_none());
    }
}
