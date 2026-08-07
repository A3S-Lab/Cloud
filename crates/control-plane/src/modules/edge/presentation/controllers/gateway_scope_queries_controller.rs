use crate::modules::edge::application::ListGatewayScopes;
use crate::modules::edge::presentation::dto::GatewayScopeResponse;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use crate::presentation::application_error_response;
use a3s_boot::{BootRequest, BootResponse, ControllerDefinition, QueryBus, Result};
use std::sync::Arc;
use uuid::Uuid;

pub fn gateway_scope_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/gateway-scopes",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListGatewayScopes {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                        })
                        .await?
                    {
                        Ok(scopes) => BootResponse::json(
                            &scopes
                                .into_iter()
                                .map(GatewayScopeResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

use super::request::request_id;
