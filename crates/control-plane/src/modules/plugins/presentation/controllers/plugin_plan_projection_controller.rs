use crate::modules::plugins::application::{
    ConfirmPluginPlanProjection, GetPluginPlanProjection,
};
use crate::modules::plugins::presentation::dto::{
    ConfirmPluginPlanProjectionRequest, PluginPlanProjectionResponse,
};
use crate::modules::shared_kernel::domain::{OrganizationId, PluginPlanProjectionId};
use crate::presentation::{
    application_error_response, organization_tenant_cloud_read_controller,
    organization_tenant_plugin_write_controller, request_identity, request_id,
};
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn plugin_plan_projection_commands_controller(
    bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    let controller = ControllerDefinition::new("/organizations")?.put(
        "/{organization_id}/plugin-plan-projections/{projection_id}/confirmation",
        move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let body: ConfirmPluginPlanProjectionRequest = request.json_with_content_type()?;
                let (_idempotency_key, request_id) = request_identity(&request)?;
                match bus
                    .execute(ConfirmPluginPlanProjection {
                        organization_id: OrganizationId::from_uuid(
                            request.param_as::<Uuid>("organization_id")?,
                        ),
                        projection_id: PluginPlanProjectionId::from_uuid(
                            request.param_as::<Uuid>("projection_id")?,
                        ),
                        confirmation: body.confirmation,
                        confirmed_at: Utc::now(),
                    })
                    .await?
                {
                    Ok(projection) => {
                        BootResponse::json(&PluginPlanProjectionResponse::from(projection))
                    }
                    Err(error) => application_error_response(error, request_id),
                }
            }
        },
    )?;
    organization_tenant_plugin_write_controller(controller)
}

pub fn plugin_plan_projection_queries_controller(
    bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    let controller = ControllerDefinition::new("/organizations")?.get(
        "/{organization_id}/plugin-plan-projections/{projection_id}",
        move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let request_id = request_id(&request)?;
                match bus
                    .execute(GetPluginPlanProjection {
                        organization_id: OrganizationId::from_uuid(
                            request.param_as::<Uuid>("organization_id")?,
                        ),
                        projection_id: PluginPlanProjectionId::from_uuid(
                            request.param_as::<Uuid>("projection_id")?,
                        ),
                    })
                    .await?
                {
                    Ok(projection) => {
                        BootResponse::json(&PluginPlanProjectionResponse::from(projection))
                    }
                    Err(error) => application_error_response(error, request_id),
                }
            }
        },
    )?;
    organization_tenant_cloud_read_controller(controller)
}
