use super::request::{environment_id, organization_id, project_id, workflow_access};
use super::workload_profile_dto::{
    AcceptWorkloadProfileRequest, AcceptedWorkloadProfileRevisionResponse,
    WorkloadProfileMutationResponse,
};
use crate::modules::developer_workflows::{
    AcceptWorkloadProfile, GetAcceptedWorkloadProfileRevision,
    GetCurrentAcceptedWorkloadProfileRevision, ListAcceptedWorkloadProfileRevisions,
    DEFAULT_WORKLOAD_PROFILE_REVISION_LIST_LIMIT, MAXIMUM_WORKLOAD_PROFILE_REVISION_LIST_LIMIT,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{
    BuildPlanId, WorkloadProfileId, WorkloadProfileRevisionId,
};
use crate::presentation::{
    actor_principal_id, application_error_response, request_id, request_identity,
};
use a3s_boot::{
    controller, get, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootError, BootRequest,
    BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn workload_profile_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    Arc::new(WorkloadProfileCommandsController { bus }).controller()
}

pub fn workload_profile_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(WorkloadProfileQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct WorkloadProfileCommandsController {
    bus: Arc<CommandBus>,
}

#[derive(Debug, Clone)]
struct WorkloadProfileQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::BUILD_WRITE])]
impl WorkloadProfileCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/workload-profiles",
        raw
    )]
    async fn accept_profile(&self, request: BootRequest) -> Result<BootResponse> {
        let body: AcceptWorkloadProfileRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(AcceptWorkloadProfile {
                organization_id: organization_id(&request)?,
                project_id: project_id(&request)?,
                environment_id: environment_id(&request)?,
                build_plan_id: BuildPlanId::from_uuid(body.build_plan_id),
                profile_acl: body.profile_acl,
                access: workflow_access(&request)?,
                actor_principal_id: actor_principal_id(&request)?,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(
                    status,
                    &WorkloadProfileMutationResponse::from(result),
                )
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::CLOUD_READ])]
impl WorkloadProfileQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/workload-profiles/{workload_profile_id}",
        raw
    )]
    async fn get_current(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetCurrentAcceptedWorkloadProfileRevision {
                organization_id: organization_id(&request)?,
                project_id: project_id(&request)?,
                environment_id: environment_id(&request)?,
                workload_profile_id: workload_profile_id(&request)?,
                access: workflow_access(&request)?,
            })
            .await?
        {
            Ok(revision) => {
                BootResponse::json(&AcceptedWorkloadProfileRevisionResponse::from(revision))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/workload-profiles/{workload_profile_id}/revisions",
        raw
    )]
    async fn list_revisions(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(ListAcceptedWorkloadProfileRevisions {
                organization_id: organization_id(&request)?,
                project_id: project_id(&request)?,
                environment_id: environment_id(&request)?,
                workload_profile_id: workload_profile_id(&request)?,
                limit: revision_list_limit(&request)?,
                access: workflow_access(&request)?,
            })
            .await?
        {
            Ok(revisions) => BootResponse::json(
                &revisions
                    .into_iter()
                    .map(AcceptedWorkloadProfileRevisionResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/workload-profiles/{workload_profile_id}/revisions/{workload_profile_revision_id}",
        raw
    )]
    async fn get_revision(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetAcceptedWorkloadProfileRevision {
                organization_id: organization_id(&request)?,
                project_id: project_id(&request)?,
                environment_id: environment_id(&request)?,
                workload_profile_id: workload_profile_id(&request)?,
                workload_profile_revision_id: WorkloadProfileRevisionId::from_uuid(
                    request.param_as::<Uuid>("workload_profile_revision_id")?,
                ),
                access: workflow_access(&request)?,
            })
            .await?
        {
            Ok(revision) => {
                BootResponse::json(&AcceptedWorkloadProfileRevisionResponse::from(revision))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

fn workload_profile_id(request: &BootRequest) -> Result<WorkloadProfileId> {
    Ok(WorkloadProfileId::from_uuid(
        request.param_as::<Uuid>("workload_profile_id")?,
    ))
}

fn revision_list_limit(request: &BootRequest) -> Result<usize> {
    let limit = request
        .optional_query_value_as::<usize>("limit")?
        .unwrap_or(DEFAULT_WORKLOAD_PROFILE_REVISION_LIST_LIMIT);
    if limit == 0 || limit > MAXIMUM_WORKLOAD_PROFILE_REVISION_LIST_LIMIT {
        return Err(BootError::BadRequest(format!(
            "limit must be between 1 and {MAXIMUM_WORKLOAD_PROFILE_REVISION_LIST_LIMIT}"
        )));
    }
    Ok(limit)
}

#[cfg(test)]
mod nest_macro_workload_profile_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;
    use std::collections::BTreeSet;

    #[test]
    fn workload_profile_queries_register_scoped_guarded_gets_via_nest_macros() {
        let controller = workload_profile_queries_controller(Arc::new(QueryBus::new()))
            .expect("workload profile nest query controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 3);
        let paths: BTreeSet<_> = routes
            .iter()
            .map(|route| (route.method(), route.path().to_string()))
            .collect();
        assert!(paths.contains(&(
            HttpMethod::Get,
            "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/workload-profiles/{workload_profile_id}"
                .into()
        )));
        assert!(paths.contains(&(
            HttpMethod::Get,
            "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/workload-profiles/{workload_profile_id}/revisions"
                .into()
        )));
        assert!(paths.contains(&(
            HttpMethod::Get,
            "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/workload-profiles/{workload_profile_id}/revisions/{workload_profile_revision_id}"
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
    fn workload_profile_commands_register_scoped_guarded_post_via_nest_macros() {
        let controller = workload_profile_commands_controller(Arc::new(CommandBus::new()))
            .expect("workload profile nest command controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Post);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/workload-profiles"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::BUILD_WRITE])
        );
    }
}
