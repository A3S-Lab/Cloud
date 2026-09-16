use crate::modules::fleet::application::EnrollNode;
use crate::presentation::application_error_response;
use a3s_boot::{
    controller, metadata, post, AUTH_PUBLIC_METADATA, BootError, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, Result,
};
use a3s_cloud_contracts::NodeEnrollmentRequest;
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn enrollment_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    Arc::new(EnrollmentController { bus }).controller()
}

#[derive(Debug, Clone)]
struct EnrollmentController {
    bus: Arc<CommandBus>,
}

#[controller("/node-control")]
#[metadata("auth.public", true)]
impl EnrollmentController {
    #[post("/enroll", raw)]
    async fn enroll(&self, request: BootRequest) -> Result<BootResponse> {
        let body: NodeEnrollmentRequest = request.json_with_content_type()?;
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(EnrollNode {
                request: body,
                request_id,
                received_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                Ok(BootResponse::json_with_status(status, &result.response)?
                    .with_header("x-a3s-api-envelope", "1"))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

fn request_id(request: &BootRequest) -> Result<Uuid> {
    request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })
}

#[cfg(test)]
mod nest_macro_enrollment_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn enrollment_controller_registers_public_post_via_nest_macros() {
        let controller = enrollment_controller(Arc::new(CommandBus::new()))
            .expect("enrollment nest command controller");
        assert_eq!(controller.prefix(), "/node-control");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Post);
        assert_eq!(routes[0].path(), "/node-control/enroll");
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
