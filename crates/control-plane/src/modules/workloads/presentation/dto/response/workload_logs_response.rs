use crate::modules::fleet::NodeLogRecordResponse;
use crate::modules::workloads::application::WorkloadLogPage;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadLogsResponse {
    pub workload_id: Uuid,
    pub revision_id: Uuid,
    pub node_id: Option<Uuid>,
    pub unit_id: String,
    pub generation: u64,
    pub records: Vec<NodeLogRecordResponse>,
    pub next_cursor: Option<String>,
}

impl From<WorkloadLogPage> for WorkloadLogsResponse {
    fn from(page: WorkloadLogPage) -> Self {
        Self {
            workload_id: page.workload_id.as_uuid(),
            revision_id: page.revision_id.as_uuid(),
            node_id: page.node_id.map(|node_id| node_id.as_uuid()),
            unit_id: page.unit_id,
            generation: page.generation,
            records: page.records.into_iter().map(Into::into).collect(),
            next_cursor: page
                .next_after_sequence
                .map(|sequence| format!("v1:{sequence}")),
        }
    }
}
