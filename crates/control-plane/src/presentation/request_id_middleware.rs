use a3s_boot::{BootRequest, BoxFuture, Middleware, MiddlewareOutcome, Result};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Default)]
pub struct RequestIdMiddleware;

impl Middleware for RequestIdMiddleware {
    fn handle(&self, mut request: BootRequest) -> BoxFuture<'static, Result<MiddlewareOutcome>> {
        Box::pin(async move {
            let request_id = request
                .header("x-request-id")
                .and_then(|value| Uuid::parse_str(value).ok())
                .unwrap_or_else(Uuid::new_v4);
            request = request.with_header("x-request-id", request_id.to_string());
            Ok(MiddlewareOutcome::next(request))
        })
    }
}
