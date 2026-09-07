use super::webhook_transport::AutomationWebhookTransportRequest;
use crate::modules::automations::application::AutomationWebhookEndpointScope;
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, Result,
    AUTH_PUBLIC_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

/// Build the public transport adapter for signed Automation webhooks.
///
/// This adapter owns no endpoint lookup, signature verification, schema
/// selection, authorization, or persistence. It only converts a bounded Boot
/// request into the application command consumed by the Automations owner.
/// The Cloud application decides whether and where to register this adapter;
/// Gateway remains the live public ingress authority.
pub fn automation_webhooks_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/webhooks")?
        .with_metadata(AUTH_PUBLIC_METADATA, true)?
        .post(
            "/automations/{organization_id}/{project_id}/{environment_id}/{endpoint_key}",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let scope = AutomationWebhookEndpointScope {
                        organization_id: request.param_as::<Uuid>("organization_id")?,
                        project_id: request.param_as::<Uuid>("project_id")?,
                        environment_id: request.param_as::<Uuid>("environment_id")?,
                    };
                    let endpoint_key = request
                        .param("endpoint_key")
                        .ok_or_else(|| {
                            BootError::BadRequest("webhook endpoint key is required".into())
                        })?
                        .to_owned();
                    let command = AutomationWebhookTransportRequest {
                        scope,
                        endpoint_key,
                        headers: request
                            .headers
                            .iter()
                            .map(|(name, value)| (name.clone(), value.clone()))
                            .collect(),
                        body: request.body().to_vec(),
                        received_at: Utc::now(),
                    }
                    .into_receive_command()
                    .map_err(|_| {
                        BootError::BadRequest(
                            "signed Automation webhook transport is invalid".into(),
                        )
                    })?;
                    let request_id = request_id(&request)?;
                    match bus.execute(command).await? {
                        Ok(_) => Ok(BootResponse::empty(202)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
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
mod tests {
    use super::*;
    use a3s_boot::CommandBus;

    #[test]
    fn exposes_only_the_scoped_signed_webhook_transport_route() {
        let controller = automation_webhooks_controller(Arc::new(CommandBus::new()))
            .expect("automation webhook controller");
        assert_eq!(controller.prefix(), "/webhooks");
        assert_eq!(controller.routes().len(), 1);
        assert_eq!(
            controller.routes()[0].path(),
            "/webhooks/automations/{organization_id}/{project_id}/{environment_id}/{endpoint_key}"
        );
    }
}
