use crate::modules::plugins::application::EnrollPluginRegistry;
use crate::modules::plugins::presentation::dto::{
    EnrollPluginRegistryRequest, PluginRegistryMutationResponse, PluginRegistryResponse,
};
use crate::modules::shared_kernel::domain::OrganizationId;
use crate::presentation::{
    actor_principal_id, application_error_response, organization_tenant_plugin_write_controller,
    request_identity,
};
use a3s_boot::{BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn plugin_registry_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let controller = ControllerDefinition::new("/organizations")?.post(
        "/{organization_id}/plugin-registries",
        move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let body: EnrollPluginRegistryRequest = request.json_with_content_type()?;
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let actor_id = actor_principal_id(&request)?;
                let (idempotency_key, request_id) = request_identity(&request)?;
                let bootstrap_root = STANDARD
                    .decode(body.bootstrap_root_base64.trim())
                    .map_err(|_| {
                        BootError::BadRequest(
                            "bootstrapRootBase64 must be standard base64 encoding of the TUF bootstrap root"
                                .into(),
                        )
                    })?;
                match bus
                    .execute(EnrollPluginRegistry {
                        organization_id,
                        actor_id,
                        name: body.name,
                        endpoint: body.endpoint,
                        bootstrap_root,
                        idempotency_key,
                        request_id,
                        requested_at: Utc::now(),
                    })
                    .await?
                {
                    Ok(result) => {
                        let status = if result.replayed { 200 } else { 201 };
                        BootResponse::json_with_status(
                            status,
                            &PluginRegistryMutationResponse {
                                registry: PluginRegistryResponse::from(result.registry),
                                replayed: result.replayed,
                            },
                        )
                    }
                    Err(error) => application_error_response(error, request_id),
                }
            }
        },
    )?;
    organization_tenant_plugin_write_controller(controller)
}
