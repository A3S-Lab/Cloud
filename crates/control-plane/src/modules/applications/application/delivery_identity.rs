use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationInvocationId, ApplicationSessionId, IdempotencyRequest,
};
use uuid::Uuid;

const SESSION_IDENTITY: &[u8] = b"cloud.application.session.v1";
const INVOCATION_IDENTITY: &[u8] = b"cloud.application.invocation.v1";

pub(super) fn idempotency(
    scope: String,
    key: String,
    canonical_request: &[u8],
) -> ApplicationResult<IdempotencyRequest> {
    IdempotencyRequest::new(scope, key, canonical_request).map_err(ApplicationError::Invalid)
}

pub(super) fn session_id(
    end_user_id: ApplicationEndUserId,
    request: &IdempotencyRequest,
) -> ApplicationSessionId {
    ApplicationSessionId::from_uuid(admission_uuid(
        end_user_id.as_uuid(),
        SESSION_IDENTITY,
        request,
    ))
}

pub(super) fn invocation_id(
    session_id: ApplicationSessionId,
    request: &IdempotencyRequest,
) -> ApplicationInvocationId {
    ApplicationInvocationId::from_uuid(admission_uuid(
        session_id.as_uuid(),
        INVOCATION_IDENTITY,
        request,
    ))
}

fn admission_uuid(namespace: Uuid, domain: &[u8], request: &IdempotencyRequest) -> Uuid {
    let mut identity =
        Vec::with_capacity(domain.len() + request.scope.len() + request.key.len() + 2);
    identity.extend_from_slice(domain);
    identity.push(0);
    identity.extend_from_slice(request.scope.as_bytes());
    identity.push(0);
    identity.extend_from_slice(request.key.as_bytes());
    Uuid::new_v5(&namespace, &identity)
}
