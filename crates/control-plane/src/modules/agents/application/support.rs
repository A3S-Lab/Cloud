use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use serde::Serialize;
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
