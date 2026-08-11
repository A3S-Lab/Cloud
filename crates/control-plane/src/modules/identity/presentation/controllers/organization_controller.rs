use crate::modules::identity::application::commands::create_organization::CreateOrganization;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::dto::{
    CreateOrganizationRequest, OrganizationResponse,
};
use crate::modules::identity::presentation::request_context::{actor, mutation_identity};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, Result, AUTH_SCOPES_METADATA,
};
use std::sync::Arc;

pub fn organization_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/organizations")?
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::PLATFORM_WRITE])?
        .post("/", move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let body: CreateOrganizationRequest = request.json_with_content_type()?;
                let actor = actor(&request)?;
                let (idempotency_key, request_id) = mutation_identity(&request)?;
                match bus
                    .execute(CreateOrganization {
                        name: body.name,
                        actor_principal_id: actor.principal_id,
                        idempotency_key,
                        request_id,
                    })
                    .await?
                {
                    Ok(result) => {
                        let status = if result.replayed { 200 } else { 201 };
                        BootResponse::json_with_status(status, &OrganizationResponse::from(result))
                    }
                    Err(error) => application_error_response(error, request_id),
                }
            }
        })
}
