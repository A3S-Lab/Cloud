use crate::modules::fleet::application::NodeLogRecord;
use crate::modules::shared_kernel::domain::{BuildRunId, OperationId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildRunLogPage {
    pub build_run_id: BuildRunId,
    pub operation_id: OperationId,
    pub generation: u64,
    pub records: Vec<NodeLogRecord>,
    pub next_after_sequence: Option<u64>,
}
