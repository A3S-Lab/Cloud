use crate::modules::plugins::application::{
    GetPluginAssignment, ListPluginAssignments, SetPluginAssignment,
};
use crate::modules::plugins::presentation::dto::{
    PluginAssignmentMutationResponse, PluginAssignmentResponse, SetPluginAssignmentRequest,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, PluginAssignmentId, PluginRegistryId, ProjectId, Sha256Digest,
};
use crate::modules::shared_kernel::domain::NodeId;
use crate::presentation::{
    actor_principal_id, application_error_response, organization_tenant_plugin_write_controller,
    request_identity, request_id,
};
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn plugin_assignment_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let controller = ControllerDefinition::new("/organizations")?.put(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/plugin-assignments",
        move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let body: SetPluginAssignmentRequest = request.json_with_content_type()?;
                let organization_id =
                    OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
                let environment_id =
                    EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?);
                let actor_id = actor_principal_id(&request)?;
                let (idempotency_key, request_id) = request_identity(&request)?;
                let policy_digest = Sha256Digest::parse(body.policy_digest)
                    .map_err(BootError::BadRequest)?;
                match bus
                    .execute(SetPluginAssignment {
                        organization_id,
                        project_id,
                        environment_id,
                        registry_id: PluginRegistryId::from_uuid(body.registry_id),
                        target_host_id: NodeId::from_uuid(body.target_host_id),
                        workspace_scope: body.workspace_scope,
                        selection: body.selection,
                        policy_digest,
                        desired_state: body.desired_state,
                        actor_id,
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
                            &PluginAssignmentMutationResponse {
                                assignment: PluginAssignmentResponse::from(result.assignment),
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

pub fn plugin_assignment_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let list_bus = Arc::clone(&bus);
    let get_bus = Arc::clone(&bus);
    let controller = ControllerDefinition::new("/organizations")?
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/plugin-assignments",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListPluginAssignments {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                        })
                        .await?
                    {
                        Ok(assignments) => BootResponse::json(
                            &assignments
                                .into_iter()
                                .map(PluginAssignmentResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/plugin-assignments/{assignment_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetPluginAssignment {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            assignment_id: PluginAssignmentId::from_uuid(
                                request.param_as::<Uuid>("assignment_id")?,
                            ),
                        })
                        .await?
                    {
                        Ok(assignment) => {
                            BootResponse::json(&PluginAssignmentResponse::from(assignment))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?;
    crate::presentation::organization_tenant_cloud_read_controller(controller)
}
