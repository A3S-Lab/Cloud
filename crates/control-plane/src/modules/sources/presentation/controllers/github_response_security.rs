use crate::presentation::boot_error_response;
use a3s_boot::{BootError, BootResponse, BoxFuture, ExceptionFilter, ExecutionContext, Result};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct GithubNoStoreErrorFilter;

impl ExceptionFilter for GithubNoStoreErrorFilter {
    fn catch(
        &self,
        context: ExecutionContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<BootResponse>>> {
        let request_id = context
            .request
            .header("x-request-id")
            .and_then(|value| Uuid::parse_str(value).ok())
            .unwrap_or_else(Uuid::new_v4);
        Box::pin(async move {
            boot_error_response(error, request_id)
                .map(no_store)
                .map(Some)
        })
    }
}

pub(super) fn no_store(response: BootResponse) -> BootResponse {
    response
        .with_header("cache-control", "no-store")
        .with_header("pragma", "no-cache")
        .with_header("referrer-policy", "no-referrer")
}
