use crate::modules::agents::domain::{
    AgentExecutionCheckpoint, AgentExecutionCheckpointObjectError,
    AgentExecutionCheckpointSnapshot, AgentReleaseBinding, IAgentExecutionCheckpointObjectStore,
};
use crate::modules::assets::DeployableAgentRelease;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, Sha256Digest};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

pub(super) fn validate_request_id(request_id: Uuid) -> ApplicationResult<()> {
    if request_id.is_nil() {
        return Err(ApplicationError::Invalid(
            "request identity must be a UUID".into(),
        ));
    }
    Ok(())
}

pub(super) fn idempotency<T: Serialize>(
    scope: String,
    key: String,
    input: &T,
) -> ApplicationResult<IdempotencyRequest> {
    let canonical =
        serde_json::to_vec(input).map_err(|error| ApplicationError::Internal(error.to_string()))?;
    IdempotencyRequest::new(scope, key, &canonical).map_err(ApplicationError::Invalid)
}

pub(super) fn bind_deployable_agent_release(
    deployable: &DeployableAgentRelease,
) -> ApplicationResult<AgentReleaseBinding> {
    AgentReleaseBinding::new(
        deployable.organization_id(),
        deployable.asset_id(),
        deployable.asset_release_id(),
        deployable.build_run_id(),
        deployable.artifact_uri(),
        Sha256Digest::parse(deployable.artifact_digest()).map_err(ApplicationError::Internal)?,
        deployable.artifact_media_type(),
        deployable.artifact_size_bytes(),
    )
    .map_err(ApplicationError::Internal)
}

pub(super) async fn load_checkpoint_snapshot(
    objects: Arc<dyn IAgentExecutionCheckpointObjectStore>,
    checkpoint: &AgentExecutionCheckpoint,
) -> ApplicationResult<AgentExecutionCheckpointSnapshot> {
    let bytes = objects
        .get(&checkpoint.object)
        .await
        .map_err(checkpoint_object_error)?;
    let snapshot =
        serde_json::from_slice::<AgentExecutionCheckpointSnapshot>(&bytes).map_err(|error| {
            ApplicationError::Internal(format!(
                "Agent checkpoint object is not a valid snapshot: {error}"
            ))
        })?;
    checkpoint.validate_snapshot(&snapshot).map_err(|error| {
        ApplicationError::Internal(format!(
            "Agent checkpoint object changed its committed projection: {error}"
        ))
    })?;
    Ok(snapshot)
}

pub(super) fn checkpoint_object_error(
    error: AgentExecutionCheckpointObjectError,
) -> ApplicationError {
    match error {
        AgentExecutionCheckpointObjectError::Conflict(message) => ApplicationError::Conflict(
            format!("Agent checkpoint object conflicts with existing content: {message}"),
        ),
        AgentExecutionCheckpointObjectError::Unavailable(message) => ApplicationError::Unavailable(
            format!("Agent checkpoint object storage is unavailable: {message}"),
        ),
        AgentExecutionCheckpointObjectError::Invalid(message) => ApplicationError::Internal(
            format!("Agent checkpoint object reference is invalid: {message}"),
        ),
        AgentExecutionCheckpointObjectError::NotFound => ApplicationError::Internal(
            "Agent checkpoint projection refers to a missing immutable object".into(),
        ),
        AgentExecutionCheckpointObjectError::Integrity(message) => ApplicationError::Internal(
            format!("Agent checkpoint object failed integrity validation: {message}"),
        ),
    }
}
