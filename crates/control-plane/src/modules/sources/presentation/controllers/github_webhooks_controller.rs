use crate::modules::sources::application::commands::accept_source_webhook_delivery::AcceptSourceWebhookDelivery;
use crate::modules::sources::application::commands::reconcile_github_connection_lifecycle::ReconcileGithubConnectionLifecycle;
use crate::modules::sources::domain::{
    ISourceWebhookVerifier, SourceWebhookVerificationError, SourceWebhookVerificationRequest,
    VerifiedSourceWebhook,
};
use crate::modules::sources::presentation::dto::SourceWebhookResponse;
use crate::presentation::application_error_response;
use a3s_boot::{
    controller, metadata, post, AUTH_PUBLIC_METADATA, BootError, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn github_webhooks_controller(
    bus: Arc<CommandBus>,
    verifier: Arc<dyn ISourceWebhookVerifier>,
) -> Result<ControllerDefinition> {
    Arc::new(GithubWebhooksController { bus, verifier }).controller()
}

#[derive(Clone)]
struct GithubWebhooksController {
    bus: Arc<CommandBus>,
    verifier: Arc<dyn ISourceWebhookVerifier>,
}

#[controller("/webhooks")]
#[metadata("auth.public", true)]
impl GithubWebhooksController {
    #[post("/github", raw)]
    async fn receive(&self, request: BootRequest) -> Result<BootResponse> {
        require_json(&request)?;
        let event = required_header(&request, "x-github-event")?;
        let delivery_id = required_header(&request, "x-github-delivery")?;
        let signature = required_header(&request, "x-hub-signature-256")?;
        let request_id = request_id(&request)?;
        let verified = self
            .verifier
            .verify(SourceWebhookVerificationRequest {
                event,
                delivery_id,
                signature,
                body: request.body(),
            })
            .map_err(verification_error)?;
        let received_at = Utc::now();
        match verified {
            VerifiedSourceWebhook::Ignored => {}
            VerifiedSourceWebhook::Repository(webhook) => {
                if let Err(error) = self
                    .bus
                    .execute(AcceptSourceWebhookDelivery {
                        webhook,
                        received_at,
                        request_id,
                    })
                    .await?
                {
                    return application_error_response(error, request_id);
                }
            }
            VerifiedSourceWebhook::GithubConnectionLifecycle(lifecycle) => {
                if let Err(error) = self
                    .bus
                    .execute(ReconcileGithubConnectionLifecycle {
                        lifecycle,
                        received_at,
                        request_id,
                    })
                    .await?
                {
                    return application_error_response(error, request_id);
                }
            }
        }
        BootResponse::json_with_status(202, &SourceWebhookResponse::received())
    }
}

fn required_header<'a>(request: &'a BootRequest, name: &str) -> Result<&'a str> {
    request
        .header(name)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BootError::Unauthorized("GitHub webhook authentication failed".into()))
}

fn require_json(request: &BootRequest) -> Result<()> {
    let media_type = request
        .header("content-type")
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if media_type != Some("application/json") {
        return Err(BootError::UnsupportedMediaType(
            "GitHub webhooks require application/json".into(),
        ));
    }
    Ok(())
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

fn verification_error(error: SourceWebhookVerificationError) -> BootError {
    match error {
        SourceWebhookVerificationError::Authentication => {
            BootError::Unauthorized("GitHub webhook authentication failed".into())
        }
        SourceWebhookVerificationError::PayloadTooLarge { maximum_bytes } => {
            BootError::PayloadTooLarge(format!(
                "GitHub webhook body exceeds the {maximum_bytes}-byte limit"
            ))
        }
        SourceWebhookVerificationError::Invalid(message) => {
            tracing::warn!(%message, "rejected invalid signed GitHub webhook");
            BootError::BadRequest("GitHub webhook payload is invalid".into())
        }
        SourceWebhookVerificationError::Unavailable(message) => {
            tracing::error!(%message, "GitHub webhook verification is unavailable");
            BootError::Adapter("GitHub webhook verification is unavailable".into())
        }
    }
}

#[cfg(test)]
mod nest_macro_github_webhooks_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;
    use crate::modules::sources::domain::SourceWebhookVerificationRequest;

    #[derive(Debug)]
    struct RejectingVerifier;

    impl ISourceWebhookVerifier for RejectingVerifier {
        fn verify(
            &self,
            _request: SourceWebhookVerificationRequest<'_>,
        ) -> std::result::Result<VerifiedSourceWebhook, SourceWebhookVerificationError> {
            Err(SourceWebhookVerificationError::Authentication)
        }
    }

    #[test]
    fn github_webhooks_controller_registers_public_post_via_nest_macros() {
        let controller = github_webhooks_controller(
            Arc::new(CommandBus::new()),
            Arc::new(RejectingVerifier),
        )
        .expect("github webhook nest controller");
        assert_eq!(controller.prefix(), "/webhooks");
        assert_eq!(controller.routes().len(), 1);
        assert_eq!(controller.routes()[0].method(), HttpMethod::Post);
        assert_eq!(controller.routes()[0].path(), "/webhooks/github");
        assert_eq!(
            controller.routes()[0]
                .metadata()
                .get(AUTH_PUBLIC_METADATA)
                .cloned()
                .expect("auth.public"),
            serde_json::json!(true)
        );
    }
}
