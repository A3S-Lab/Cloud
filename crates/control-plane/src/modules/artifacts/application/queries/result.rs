use crate::modules::artifacts::application::BuildLogRecord;
use crate::modules::shared_kernel::domain::{BuildRunId, OperationId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildRunLogPage {
    pub build_run_id: BuildRunId,
    pub operation_id: OperationId,
    pub generation: u64,
    pub records: Vec<BuildLogRecord>,
    pub next_after_sequence: Option<u64>,
}
