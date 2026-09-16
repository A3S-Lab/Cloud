use super::dto::{
    AcceptBuildPlanRequest, AcceptedBuildPlanResponse, BuildPlanDetectionResponse,
    BuildPlanMutationResponse, DetectBuildPlansRequest,
};
use super::request::{environment_id, organization_id, project_id, workflow_access};
use super::routes::{
    BUILD_PLAN_COLLECTION_ROUTE, BUILD_PLAN_DETECTION_ROUTE, BUILD_PLAN_ITEM_ROUTE,
    DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX,
};
use crate::modules::developer_workflows::{
    AcceptBuildPlan, DetectBuildPlanProposals, GetAcceptedBuildPlan, ListAcceptedBuildPlans,
    DEFAULT_BUILD_PLAN_LIST_LIMIT, MAXIMUM_BUILD_PLAN_LIST_LIMIT,
};
use crate::modules::shared_kernel::domain::{BuildPlanId, SourceRevisionId};
use crate::presentation::{
    actor_principal_id, application_error_response, organization_tenant_build_write_controller,
    organization_tenant_cloud_read_controller, request_id, request_identity,
};
use a3s_boot::{
    controller, get, post, BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition,
    QueryBus, Result,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn build_plan_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let controller = Arc::new(BuildPlanCommandsController { bus }).controller()?;
    organization_tenant_build_write_controller(controller)
}

pub fn build_plan_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let controller = Arc::new(BuildPlanQueriesController { bus }).controller()?;
    organization_tenant_cloud_read_controller(controller)
}

#[derive(Debug, Clone)]
struct BuildPlanCommandsController {
    bus: Arc<CommandBus>,
}

#[derive(Debug, Clone)]
struct BuildPlanQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
impl BuildPlanCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/build-plans",
        raw
    )]
    async fn accept(&self, request: BootRequest) -> Result<BootResponse> {
        let body: AcceptBuildPlanRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(AcceptBuildPlan {
                organization_id: organization_id(&request)?,
                project_id: project_id(&request)?,
                environment_id: environment_id(&request)?,
                source_revision_id: SourceRevisionId::from_uuid(body.source_revision_id),
                proposal_acl: body.proposal_acl,
                access: workflow_access(&request)?,
                actor_principal_id: actor_principal_id(&request)?,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(status, &BuildPlanMutationResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[controller("/organizations")]
impl BuildPlanQueriesController {
    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/build-plan-detections",
        raw
    )]
    async fn detect(&self, request: BootRequest) -> Result<BootResponse> {
        let body: DetectBuildPlansRequest = request.json_with_content_type()?;
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(DetectBuildPlanProposals {
                organization_id: organization_id(&request)?,
                project_id: project_id(&request)?,
                environment_id: environment_id(&request)?,
                source_revision_id: SourceRevisionId::from_uuid(body.source_revision_id),
                access: workflow_access(&request)?,
            })
            .await?
        {
            Ok(detection) => BootResponse::json(&BuildPlanDetectionResponse::from(detection)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/build-plans",
        raw
    )]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let source_revision_id = required_source_revision_id(&request)?;
        let limit = build_plan_list_limit(&request)?;
        match self
            .bus
            .execute(ListAcceptedBuildPlans {
                organization_id: organization_id(&request)?,
                project_id: project_id(&request)?,
                environment_id: environment_id(&request)?,
                source_revision_id,
                limit,
                access: workflow_access(&request)?,
            })
            .await?
        {
            Ok(plans) => BootResponse::json(
                &plans
                    .into_iter()
                    .map(AcceptedBuildPlanResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/build-plans/{build_plan_id}",
        raw
    )]
    async fn get(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        match self
            .bus
            .execute(GetAcceptedBuildPlan {
                organization_id: organization_id(&request)?,
                project_id: project_id(&request)?,
                environment_id: environment_id(&request)?,
                build_plan_id: BuildPlanId::from_uuid(request.param_as::<Uuid>("build_plan_id")?),
                access: workflow_access(&request)?,
            })
            .await?
        {
            Ok(plan) => BootResponse::json(&AcceptedBuildPlanResponse::from(plan)),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

fn required_source_revision_id(request: &BootRequest) -> Result<SourceRevisionId> {
    request
        .optional_query_value_as::<Uuid>("sourceRevisionId")?
        .map(SourceRevisionId::from_uuid)
        .ok_or_else(|| BootError::BadRequest("sourceRevisionId query parameter is required".into()))
}

fn build_plan_list_limit(request: &BootRequest) -> Result<usize> {
    let limit = request
        .optional_query_value_as::<usize>("limit")?
        .unwrap_or(DEFAULT_BUILD_PLAN_LIST_LIMIT);
    if limit == 0 || limit > MAXIMUM_BUILD_PLAN_LIST_LIMIT {
        return Err(BootError::BadRequest(format!(
            "limit must be between 1 and {MAXIMUM_BUILD_PLAN_LIST_LIMIT}"
        )));
    }
    Ok(limit)
}

#[cfg(test)]
mod nest_macro_build_plan_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn build_plan_controllers_register_scoped_routes_via_nest_macros() {
        let commands = build_plan_commands_controller(Arc::new(CommandBus::new()))
            .expect("build plan nest command controller");
        assert_eq!(commands.prefix(), DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX);
        assert_eq!(commands.routes().len(), 1);
        assert_eq!(commands.routes()[0].method(), HttpMethod::Post);
        assert_eq!(
            commands.routes()[0].path(),
            format!("{DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX}{BUILD_PLAN_COLLECTION_ROUTE}")
        );
        assert_eq!(
            commands.routes()[0]
                .metadata()
                .get("auth.scopes")
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!(["build:write"])
        );

        let queries = build_plan_queries_controller(Arc::new(QueryBus::new()))
            .expect("build plan nest query controller");
        assert_eq!(queries.prefix(), DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX);
        assert_eq!(queries.routes().len(), 3);
        assert_eq!(queries.routes()[0].method(), HttpMethod::Post);
        assert_eq!(
            queries.routes()[0].path(),
            format!("{DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX}{BUILD_PLAN_DETECTION_ROUTE}")
        );
        assert_eq!(
            queries.routes()[2].path(),
            format!("{DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX}{BUILD_PLAN_ITEM_ROUTE}")
        );
        assert_eq!(
            queries.routes()[0]
                .metadata()
                .get("auth.scopes")
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!(["cloud:read"])
        );
    }
}
