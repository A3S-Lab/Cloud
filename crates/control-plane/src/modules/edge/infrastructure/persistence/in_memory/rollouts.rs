mod queries;
mod recovery;
mod staging;

pub(super) use queries::{
    certificate_binding, find, find_rollback, next_generation, pending_dispatches,
    pending_rollbacks, replay,
};
pub(super) use recovery::{
    mark_unavailable, pending_recoveries, project_terminal_rollback, record_recovery_failure,
    record_recovery_observation, stage_recovery_observation,
};
pub(super) use staging::{stage, stage_rollback};

use crate::modules::shared_kernel::domain::RepositoryError;

fn validate_batch_limit(limit: usize) -> Result<(), RepositoryError> {
    if limit == 0 || limit > 10_000 {
        return Err(RepositoryError::Conflict(
            "Gateway rollout dispatch batch limit is invalid".into(),
        ));
    }
    Ok(())
}
