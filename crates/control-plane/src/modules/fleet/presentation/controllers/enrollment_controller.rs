use crate::modules::fleet::application::EnrollNode;
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, Result,
    AUTH_PUBLIC_METADATA,
};
use a3s_cloud_contracts::NodeEnrollmentRequest;
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn enrollment_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/node-control")?
        .with_metadata(AUTH_PUBLIC_METADATA, true)?
        .post("/enroll", move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let body: NodeEnrollmentRequest = request.json_with_content_type()?;
                let request_id = request_id(&request)?;
                match bus
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
        })
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
