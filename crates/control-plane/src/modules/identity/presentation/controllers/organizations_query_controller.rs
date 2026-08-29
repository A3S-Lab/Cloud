use crate::modules::identity::application::queries::list_organizations::ListOrganizations;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::dto::OrganizationListItemResponse;
use crate::modules::identity::presentation::request_context::{
    authenticated_credential_actor, request_id,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, ControllerDefinition, QueryBus, Result, AUTH_SCOPES_METADATA,
};
use std::sync::Arc;

pub fn organizations_query_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/organizations")?
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])?
        .get("/", move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let principal = request.require_auth_principal()?;
                let actor = authenticated_credential_actor(&principal)?;
                let request_id = request_id(&request)?;
                match bus
                    .execute(ListOrganizations {
                        actor_principal_id: actor.principal_id,
                        credential_id: actor.credential_id,
                        request_id,
                    })
                    .await?
                {
                    Ok(organizations) => BootResponse::json(
                        &organizations
                            .into_iter()
                            .map(OrganizationListItemResponse::from)
                            .collect::<Vec<_>>(),
                    ),
                    Err(error) => application_error_response(error, request_id),
                }
            }
        })
}
