use super::request::workload_access;
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, DeploymentId, EnvironmentId, NodePoolId, OrganizationId, ProjectId,
    SourceRevisionId, WorkloadId,
};
use crate::modules::workloads::application::{
    BindSkillWorkloadDeployment, CancelDeployment, CreateAgentWorkloadDeployment,
    CreateSourceWorkloadDeployment, CreateWorkloadDeployment, RollbackWorkloadDeployment,
    StopWorkload, UnbindSkillWorkloadDeployment, UpdateAgentWorkloadDeployment,
    UpdateWorkloadDeployment,
};
use crate::modules::workloads::presentation::dto::{
    parse_source_workload_manifest, parse_workload_manifest, CancelDeploymentResponse,
    CreateSourceWorkloadRequest, CreateWorkloadRequest, RollbackWorkloadRequest,
    UpdateAgentWorkloadRequest, UpdateWorkloadRequest, WorkloadDeploymentResponse,
    WorkloadStopResponse,
};
use crate::presentation::{
    application_error_response, organization_tenant_workload_write_controller, request_identity,
    with_deferred_project_scope, A3S_ACL_MEDIA_TYPE,
};
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, Result, RouteDefinition,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn workloads_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let source_bus = Arc::clone(&bus);
    let agent_create_bus = Arc::clone(&bus);
    let agent_update_bus = Arc::clone(&bus);
    let cancel_bus = Arc::clone(&bus);
    let stop_bus = Arc::clone(&bus);
    let update_bus = Arc::clone(&bus);
    let rollback_bus = Arc::clone(&bus);
    let bind_skill_bus = Arc::clone(&bus);
    let unbind_skill_bus = Arc::clone(&bus);
    let controller = ControllerDefinition::new("/organizations")?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/workloads",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let body = create_workload_request(&request)?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
                    let environment_id =
                        EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?);
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateWorkloadDeployment {
                            organization_id,
                            project_id,
                            environment_id,
                            name: body.name,
                            node_pool_id: body.node_pool_id.map(NodePoolId::from_uuid),
                            template: body.template.into(),
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.bundle.replayed { 200 } else { 202 };
                            BootResponse::json_with_status(
                                status,
                                &WorkloadDeploymentResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/source-revisions/{source_revision_id}/workloads",
            move |request: BootRequest| {
                let bus = Arc::clone(&source_bus);
                async move {
                    let body = create_source_workload_request(&request)?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
                    let environment_id =
                        EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?);
                    let source_revision_id = SourceRevisionId::from_uuid(
                        request.param_as::<Uuid>("source_revision_id")?,
                    );
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateSourceWorkloadDeployment {
                            organization_id,
                            project_id,
                            environment_id,
                            source_revision_id,
                            name: body.name,
                            node_pool_id: body.node_pool_id.map(NodePoolId::from_uuid),
                            template: body.template.into(),
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.bundle.replayed { 200 } else { 202 };
                            BootResponse::json_with_status(
                                status,
                                &WorkloadDeploymentResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/assets/{asset_id}/releases/{asset_release_id}/workloads",
            move |request: BootRequest| {
                let bus = Arc::clone(&agent_create_bus);
                async move {
                    let body = create_source_workload_request(&request)?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
                    let environment_id =
                        EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?);
                    let asset_id = AssetId::from_uuid(request.param_as::<Uuid>("asset_id")?);
                    let asset_release_id = AssetReleaseId::from_uuid(
                        request.param_as::<Uuid>("asset_release_id")?,
                    );
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateAgentWorkloadDeployment {
                            organization_id,
                            project_id,
                            environment_id,
                            asset_id,
                            asset_release_id,
                            name: body.name,
                            node_pool_id: body.node_pool_id.map(NodePoolId::from_uuid),
                            template: body.template.into(),
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.bundle.replayed { 200 } else { 202 };
                            BootResponse::json_with_status(
                                status,
                                &WorkloadDeploymentResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .route(with_deferred_project_scope(
            RouteDefinition::post(
                "/{organization_id}/workloads/{workload_id}/deployments",
                move |request: BootRequest| {
                let bus = Arc::clone(&update_bus);
                async move {
                    let (body, expected_name, expected_node_pool_id) =
                        update_workload_request(&request)?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let workload_id =
                        WorkloadId::from_uuid(request.param_as::<Uuid>("workload_id")?);
                    let access = workload_access(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(UpdateWorkloadDeployment {
                            organization_id,
                            workload_id,
                            access,
                            expected_name,
                            expected_node_pool_id,
                            template: body.template.into(),
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.bundle.replayed { 200 } else { 202 };
                            BootResponse::json_with_status(
                                status,
                                &WorkloadDeploymentResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
                },
            )?,
        )?)?
        .route(with_deferred_project_scope(
            RouteDefinition::post(
                "/{organization_id}/workloads/{workload_id}/assets/{asset_id}/releases/{asset_release_id}/deployments",
                move |request: BootRequest| {
                let bus = Arc::clone(&agent_update_bus);
                async move {
                    let (body, expected_name, expected_node_pool_id) =
                        update_agent_workload_request(&request)?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let workload_id =
                        WorkloadId::from_uuid(request.param_as::<Uuid>("workload_id")?);
                    let access = workload_access(&request)?;
                    let asset_id = AssetId::from_uuid(request.param_as::<Uuid>("asset_id")?);
                    let asset_release_id = AssetReleaseId::from_uuid(
                        request.param_as::<Uuid>("asset_release_id")?,
                    );
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(UpdateAgentWorkloadDeployment {
                            organization_id,
                            workload_id,
                            access,
                            asset_id,
                            asset_release_id,
                            expected_name,
                            expected_node_pool_id,
                            template: body.template.into(),
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.bundle.replayed { 200 } else { 202 };
                            BootResponse::json_with_status(
                                status,
                                &WorkloadDeploymentResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
                },
            )?,
        )?)?
        .route(with_deferred_project_scope(
            RouteDefinition::post(
                "/{organization_id}/workloads/{workload_id}/rollback",
                move |request: BootRequest| {
                let bus = Arc::clone(&rollback_bus);
                async move {
                    let body: RollbackWorkloadRequest = request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let workload_id =
                        WorkloadId::from_uuid(request.param_as::<Uuid>("workload_id")?);
                    let access = workload_access(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(RollbackWorkloadDeployment {
                            organization_id,
                            workload_id,
                            access,
                            source_revision_id: body.source_revision_id(),
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.bundle.replayed { 200 } else { 202 };
                            BootResponse::json_with_status(
                                status,
                                &WorkloadDeploymentResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
                },
            )?,
        )?)?
        .route(with_deferred_project_scope(
            RouteDefinition::post(
                "/{organization_id}/workloads/{workload_id}/skills/{skill_asset_id}/releases/{skill_asset_release_id}/bindings",
                move |request: BootRequest| {
                let bus = Arc::clone(&bind_skill_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let workload_id =
                        WorkloadId::from_uuid(request.param_as::<Uuid>("workload_id")?);
                    let access = workload_access(&request)?;
                    let skill_asset_id =
                        AssetId::from_uuid(request.param_as::<Uuid>("skill_asset_id")?);
                    let skill_asset_release_id = AssetReleaseId::from_uuid(
                        request.param_as::<Uuid>("skill_asset_release_id")?,
                    );
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(BindSkillWorkloadDeployment {
                            organization_id,
                            workload_id,
                            access,
                            skill_asset_id,
                            skill_asset_release_id,
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.bundle.replayed { 200 } else { 202 };
                            BootResponse::json_with_status(
                                status,
                                &WorkloadDeploymentResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
                },
            )?,
        )?)?
        .route(with_deferred_project_scope(
            RouteDefinition::delete(
                "/{organization_id}/workloads/{workload_id}/skills/{skill_asset_id}/bindings",
                move |request: BootRequest| {
                let bus = Arc::clone(&unbind_skill_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let workload_id =
                        WorkloadId::from_uuid(request.param_as::<Uuid>("workload_id")?);
                    let access = workload_access(&request)?;
                    let skill_asset_id =
                        AssetId::from_uuid(request.param_as::<Uuid>("skill_asset_id")?);
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(UnbindSkillWorkloadDeployment {
                            organization_id,
                            workload_id,
                            access,
                            skill_asset_id,
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.bundle.replayed { 200 } else { 202 };
                            BootResponse::json_with_status(
                                status,
                                &WorkloadDeploymentResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
                },
            )?,
        )?)?
        .route(with_deferred_project_scope(
            RouteDefinition::post(
                "/{organization_id}/workloads/{workload_id}/stop",
                move |request: BootRequest| {
                let bus = Arc::clone(&stop_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let workload_id =
                        WorkloadId::from_uuid(request.param_as::<Uuid>("workload_id")?);
                    let access = workload_access(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(StopWorkload {
                            organization_id,
                            workload_id,
                            access,
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.bundle.replayed { 200 } else { 202 };
                            BootResponse::json_with_status(
                                status,
                                &WorkloadStopResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
                },
            )?,
        )?)?
        .route(with_deferred_project_scope(
            RouteDefinition::delete(
                "/{organization_id}/deployments/{deployment_id}",
                move |request: BootRequest| {
                let bus = Arc::clone(&cancel_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let deployment_id =
                        DeploymentId::from_uuid(request.param_as::<Uuid>("deployment_id")?);
                    let access = workload_access(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CancelDeployment {
                            organization_id,
                            deployment_id,
                            access,
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.replayed { 200 } else { 202 };
                            BootResponse::json_with_status(
                                status,
                                &CancelDeploymentResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
                },
            )?,
        )?)?;
    organization_tenant_workload_write_controller(controller)
}

fn create_workload_request(request: &BootRequest) -> Result<CreateWorkloadRequest> {
    if is_acl_request(request) {
        let manifest = parse_workload_manifest(request.body())?;
        Ok(CreateWorkloadRequest {
            name: manifest.name,
            node_pool_id: manifest.node_pool_id,
            template: manifest.template,
        })
    } else {
        request.json_with_content_type()
    }
}

fn create_source_workload_request(request: &BootRequest) -> Result<CreateSourceWorkloadRequest> {
    if is_acl_request(request) {
        let manifest = parse_source_workload_manifest(request.body())?;
        Ok(CreateSourceWorkloadRequest {
            name: manifest.name,
            node_pool_id: manifest.node_pool_id,
            template: manifest.template,
        })
    } else {
        request.json_with_content_type()
    }
}

fn update_workload_request(
    request: &BootRequest,
) -> Result<(
    UpdateWorkloadRequest,
    Option<String>,
    Option<Option<NodePoolId>>,
)> {
    if is_acl_request(request) {
        let manifest = parse_workload_manifest(request.body())?;
        Ok((
            UpdateWorkloadRequest {
                template: manifest.template,
            },
            Some(manifest.name),
            Some(manifest.node_pool_id.map(NodePoolId::from_uuid)),
        ))
    } else {
        Ok((request.json_with_content_type()?, None, None))
    }
}

fn update_agent_workload_request(
    request: &BootRequest,
) -> Result<(
    UpdateAgentWorkloadRequest,
    Option<String>,
    Option<Option<NodePoolId>>,
)> {
    if is_acl_request(request) {
        let manifest = parse_source_workload_manifest(request.body())?;
        Ok((
            UpdateAgentWorkloadRequest {
                template: manifest.template,
            },
            Some(manifest.name),
            Some(manifest.node_pool_id.map(NodePoolId::from_uuid)),
        ))
    } else {
        Ok((request.json_with_content_type()?, None, None))
    }
}

fn is_acl_request(request: &BootRequest) -> bool {
    request
        .header("content-type")
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case(A3S_ACL_MEDIA_TYPE))
}
