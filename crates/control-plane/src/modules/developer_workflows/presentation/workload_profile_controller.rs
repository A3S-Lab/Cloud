use super::request::{environment_id, organization_id, project_id, workflow_access};
use super::routes::{
    DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX, WORKLOAD_PROFILE_COLLECTION_ROUTE,
    WORKLOAD_PROFILE_ITEM_ROUTE, WORKLOAD_PROFILE_REVISION_COLLECTION_ROUTE,
    WORKLOAD_PROFILE_REVISION_ITEM_ROUTE,
};
use super::workload_profile_dto::{
    AcceptWorkloadProfileRequest, AcceptedWorkloadProfileRevisionResponse,
    WorkloadProfileMutationResponse,
};
use crate::modules::developer_workflows::{
    AcceptWorkloadProfile, GetAcceptedWorkloadProfileRevision,
    GetCurrentAcceptedWorkloadProfileRevision, ListAcceptedWorkloadProfileRevisions,
    DEFAULT_WORKLOAD_PROFILE_REVISION_LIST_LIMIT, MAXIMUM_WORKLOAD_PROFILE_REVISION_LIST_LIMIT,
};
use crate::modules::shared_kernel::domain::{
    BuildPlanId, WorkloadProfileId, WorkloadProfileRevisionId,
};
use crate::presentation::{
    actor_principal_id, application_error_response, organization_tenant_build_write_controller,
    organization_tenant_cloud_read_controller, request_id, request_identity,
};
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn workload_profile_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let controller = ControllerDefinition::new(DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX)?.post(
        WORKLOAD_PROFILE_COLLECTION_ROUTE,
        move |request: BootRequest| {
            let bus = Arc::clone(&bus);
            async move {
                let body: AcceptWorkloadProfileRequest = request.json_with_content_type()?;
                let (idempotency_key, request_id) = request_identity(&request)?;
                match bus
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
        },
    )?;
    organization_tenant_build_write_controller(controller)
}

pub fn workload_profile_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let current_bus = Arc::clone(&bus);
    let list_bus = Arc::clone(&bus);
    let controller = ControllerDefinition::new(DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX)?
        .get(WORKLOAD_PROFILE_ITEM_ROUTE, move |request: BootRequest| {
            let bus = Arc::clone(&current_bus);
            async move {
                let request_id = request_id(&request)?;
                match bus
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
        })?
        .get(
            WORKLOAD_PROFILE_REVISION_COLLECTION_ROUTE,
            move |request: BootRequest| {
                let bus = Arc::clone(&list_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
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
            },
        )?
        .get(
            WORKLOAD_PROFILE_REVISION_ITEM_ROUTE,
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
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
                        Ok(revision) => BootResponse::json(
                            &AcceptedWorkloadProfileRevisionResponse::from(revision),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?;
    organization_tenant_cloud_read_controller(controller)
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
