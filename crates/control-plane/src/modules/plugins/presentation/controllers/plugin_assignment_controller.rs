use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::plugins::application::{
    GetPluginAssignment, ListPluginAssignments, SetPluginAssignment,
};
use crate::modules::plugins::presentation::dto::{
    PluginAssignmentMutationResponse, PluginAssignmentResponse, SetPluginAssignmentRequest,
};
use crate::modules::shared_kernel::domain::NodeId;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, PluginAssignmentId, PluginRegistryId, ProjectId, Sha256Digest,
};
use crate::presentation::{
    actor_principal_id, application_error_response, plugin_access, request_id, request_identity,
    resource_access_evaluator,
};
use a3s_boot::{
    controller, get, metadata, put, use_guard, AUTH_SCOPES_METADATA, BootError, BootRequest,
    BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn plugin_assignment_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    Arc::new(PluginAssignmentCommandsController { bus }).controller()
}

pub fn plugin_assignment_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(PluginAssignmentQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct PluginAssignmentCommandsController {
    bus: Arc<CommandBus>,
}

#[derive(Debug, Clone)]
struct PluginAssignmentQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::PLUGIN_WRITE])]
impl PluginAssignmentCommandsController {
    #[put(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/plugin-assignments",
        raw
    )]
    async fn set_assignment(&self, request: BootRequest) -> Result<BootResponse> {
        let body: SetPluginAssignmentRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
        let environment_id =
            EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?);
        let actor_id = actor_principal_id(&request)?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        let access = plugin_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        let policy_digest =
            Sha256Digest::parse(body.policy_digest).map_err(BootError::BadRequest)?;
        match self
            .bus
            .execute(SetPluginAssignment {
                organization_id,
                project_id,
                environment_id,
                access,
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
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::CLOUD_READ])]
impl PluginAssignmentQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/plugin-assignments",
        raw
    )]
    async fn list_assignments(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let access = plugin_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        match self
            .bus
            .execute(ListPluginAssignments {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                access,
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

    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/plugin-assignments/{assignment_id}",
        raw
    )]
    async fn get_assignment(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
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
            Ok(assignment) => BootResponse::json(&PluginAssignmentResponse::from(assignment)),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_plugin_assignment_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;
    use std::collections::BTreeSet;

    #[test]
    fn plugin_assignment_queries_register_scoped_guarded_gets_via_nest_macros() {
        let controller = plugin_assignment_queries_controller(Arc::new(QueryBus::new()))
            .expect("plugin assignment nest query controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 2);
        let paths: BTreeSet<_> = routes
            .iter()
            .map(|route| (route.method(), route.path().to_string()))
            .collect();
        assert!(paths.contains(&(
            HttpMethod::Get,
            "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/plugin-assignments"
                .into()
        )));
        assert!(paths.contains(&(
            HttpMethod::Get,
            "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/plugin-assignments/{assignment_id}"
                .into()
        )));
        for route in routes {
            assert_eq!(
                route
                    .metadata()
                    .get(AUTH_SCOPES_METADATA)
                    .cloned()
                    .expect("auth.scopes"),
                serde_json::json!([ApiTokenScope::CLOUD_READ])
            );
        }
    }

    #[test]
    fn plugin_assignment_commands_register_scoped_guarded_put_via_nest_macros() {
        let controller = plugin_assignment_commands_controller(Arc::new(CommandBus::new()))
            .expect("plugin assignment nest command controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Put);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/plugin-assignments"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::PLUGIN_WRITE])
        );
    }
}
