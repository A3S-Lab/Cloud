use crate::modules::identity::application::commands::bootstrap_identity::BootstrapIdentity;
use crate::modules::identity::presentation::dto::{
    BootstrapIdentityRequest, BootstrapIdentityResponse,
};
use crate::modules::identity::presentation::BootstrapGuard;
use crate::presentation::application_error_response;
use a3s_boot::{
    controller, metadata, post, AUTH_PUBLIC_METADATA, BootError, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, Result,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn bootstrap_controller(
    bus: Arc<CommandBus>,
    guard: BootstrapGuard,
) -> Result<ControllerDefinition> {
    // Nest macros own the public POST; BootstrapGuard stays injected at wiring.
    Ok(Arc::new(BootstrapController { bus })
        .controller()?
        .with_guard(guard))
}

#[derive(Debug, Clone)]
struct BootstrapController {
    bus: Arc<CommandBus>,
}

#[controller("/bootstrap")]
#[metadata("auth.public", true)]
impl BootstrapController {
    #[post("/", raw)]
    async fn bootstrap(&self, request: BootRequest) -> Result<BootResponse> {
        let body: BootstrapIdentityRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(BootstrapIdentity {
                organization_name: body.organization_name,
                token_name: body.token_name,
                token_secret: body.token,
                expires_at: body.expires_at,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(status, &BootstrapIdentityResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

fn request_identity(request: &BootRequest) -> Result<(String, Uuid)> {
    let idempotency_key = request
        .header("idempotency-key")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BootError::BadRequest("idempotency-key header is required".into()))?
        .to_owned();
    let request_id = request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })?;
    Ok((idempotency_key, request_id))
}

#[cfg(test)]
mod nest_macro_bootstrap_controller_tests {
    use super::*;
    use crate::modules::identity::domain::value_objects::BootstrapCredential;
    use a3s_boot::HttpMethod;

    #[test]
    fn bootstrap_controller_registers_public_post_via_nest_macros() {
        let guard = BootstrapGuard::new(
            BootstrapCredential::new(&"b".repeat(32)).expect("bootstrap credential"),
        );
        let controller = bootstrap_controller(Arc::new(CommandBus::new()), guard)
            .expect("bootstrap nest controller");

        assert_eq!(controller.prefix(), "/bootstrap");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Post);
        assert_eq!(routes[0].path(), "/bootstrap");
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_PUBLIC_METADATA)
                .cloned()
                .expect("auth.public"),
            serde_json::json!(true)
        );
    }
}
