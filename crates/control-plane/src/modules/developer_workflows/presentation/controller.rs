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
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn build_plan_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let controller = ControllerDefinition::new(DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX)?.post(
        BUILD_PLAN_COLLECTION_ROUTE,
        move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let body: AcceptBuildPlanRequest = request.json_with_content_type()?;
                let (idempotency_key, request_id) = request_identity(&request)?;
                match bus
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
                        BootResponse::json_with_status(
                            status,
                            &BuildPlanMutationResponse::from(result),
                        )
                    }
                    Err(error) => application_error_response(error, request_id),
                }
            }
        },
    )?;
    organization_tenant_build_write_controller(controller)
}

pub fn build_plan_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let detect_bus = Arc::clone(&bus);
    let list_bus = Arc::clone(&bus);
    let controller = ControllerDefinition::new(DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX)?
        .post(BUILD_PLAN_DETECTION_ROUTE, move |request: BootRequest| {
            let bus = Arc::clone(&detect_bus);
            async move {
                let body: DetectBuildPlansRequest = request.json_with_content_type()?;
                let request_id = request_id(&request)?;
                match bus
                    .execute(DetectBuildPlanProposals {
                        organization_id: organization_id(&request)?,
                        project_id: project_id(&request)?,
                        environment_id: environment_id(&request)?,
                        source_revision_id: SourceRevisionId::from_uuid(body.source_revision_id),
                        access: workflow_access(&request)?,
                    })
                    .await?
                {
                    Ok(detection) => {
                        BootResponse::json(&BuildPlanDetectionResponse::from(detection))
                    }
                    Err(error) => application_error_response(error, request_id),
                }
            }
        })?
        .get(BUILD_PLAN_COLLECTION_ROUTE, move |request: BootRequest| {
            let bus = Arc::clone(&list_bus);
            async move {
                let request_id = request_id(&request)?;
                let source_revision_id = required_source_revision_id(&request)?;
                let limit = build_plan_list_limit(&request)?;
                match bus
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
        })?
        .get(BUILD_PLAN_ITEM_ROUTE, move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let request_id = request_id(&request)?;
                match bus
                    .execute(GetAcceptedBuildPlan {
                        organization_id: organization_id(&request)?,
                        project_id: project_id(&request)?,
                        environment_id: environment_id(&request)?,
                        build_plan_id: BuildPlanId::from_uuid(
                            request.param_as::<Uuid>("build_plan_id")?,
                        ),
                        access: workflow_access(&request)?,
                    })
                    .await?
                {
                    Ok(plan) => BootResponse::json(&AcceptedBuildPlanResponse::from(plan)),
                    Err(error) => application_error_response(error, request_id),
                }
            }
        })?;
    organization_tenant_cloud_read_controller(controller)
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
