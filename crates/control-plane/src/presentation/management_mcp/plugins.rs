use super::arguments::EmptyArguments;
use super::tool_result;
use crate::access_projection::plugin_access;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::plugins::domain::value_objects::PluginCatalogSelection;
use crate::modules::plugins::{
    ConfirmPluginPlanProjection, GetPluginAssignment, GetPluginPlanProjection, GetPluginRegistry,
    InspectCachedPluginCatalog, InspectPluginCatalog, ListPluginAssignments, ListPluginRegistries,
    PluginAssignmentMutationResponse, PluginAssignmentResponse, PluginCatalogInspectRequest,
    PluginCatalogSearchRequest, PluginPlanProjectionResponse, PluginRegistryResponse,
    SearchCachedPluginCatalog, SearchPluginCatalog, SetPluginAssignment,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeId, OrganizationId, PluginAssignmentId, PluginPlanProjectionId,
    PluginRegistryId, PrincipalId, ProjectId, Sha256Digest,
};
use a3s_boot::{BootError, CommandBus, QueryBus, Result};
use a3s_use_core::{PluginDesiredState, PluginManagedScope, PluginOperationConfirmation};
use chrono::Utc;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginRegistryArguments {
    registry_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginCatalogSearchArguments {
    registry_id: Uuid,
    #[serde(flatten)]
    request: PluginCatalogSearchRequest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginCatalogInspectArguments {
    registry_id: Uuid,
    #[serde(flatten)]
    request: PluginCatalogInspectRequest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListPluginAssignmentsArguments {
    project_id: Uuid,
    environment_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginAssignmentArguments {
    project_id: Uuid,
    environment_id: Uuid,
    assignment_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetPluginAssignmentArguments {
    project_id: Uuid,
    environment_id: Uuid,
    registry_id: Uuid,
    target_host_id: Uuid,
    workspace_scope: PluginManagedScope,
    selection: PluginCatalogSelection,
    policy_digest: String,
    desired_state: PluginDesiredState,
    #[serde(deserialize_with = "super::arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginPlanProjectionArguments {
    projection_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfirmPluginPlanProjectionArguments {
    projection_id: Uuid,
    confirmation: PluginOperationConfirmation,
    #[serde(deserialize_with = "super::arguments::deserialize_idempotency_key")]
    #[allow(dead_code)]
    idempotency_key: String,
}

pub async fn list_registries(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    _arguments: EmptyArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListPluginRegistries { organization_id })
        .await?
    {
        Ok(registries) => tool_result::success(
            200,
            registries
                .into_iter()
                .map(PluginRegistryResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_registry(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: PluginRegistryArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetPluginRegistry {
            organization_id,
            registry_id: PluginRegistryId::from_uuid(arguments.registry_id),
        })
        .await?
    {
        Ok(registry) => {
            tool_result::success(200, PluginRegistryResponse::from(registry), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_assignments(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListPluginAssignmentsArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListPluginAssignments {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            access: plugin_access(&resource_access),
        })
        .await?
    {
        Ok(assignments) => tool_result::success(
            200,
            assignments
                .into_iter()
                .map(PluginAssignmentResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_assignment(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: PluginAssignmentArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetPluginAssignment {
            organization_id,
            assignment_id: PluginAssignmentId::from_uuid(arguments.assignment_id),
        })
        .await?
    {
        Ok(assignment)
            if assignment.project_id.as_uuid() == arguments.project_id
                && assignment.environment_id.as_uuid() == arguments.environment_id =>
        {
            tool_result::success(200, PluginAssignmentResponse::from(assignment), request_id)
        }
        Ok(_) => tool_result::application_error(
            crate::modules::shared_kernel::application::ApplicationError::NotFound(
                "plugin assignment was not found".into(),
            ),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn set_assignment(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: SetPluginAssignmentArguments,
    request_id: Uuid,
) -> Result<Value> {
    let policy_digest =
        Sha256Digest::parse(arguments.policy_digest).map_err(BootError::BadRequest)?;
    match bus
        .execute(SetPluginAssignment {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            registry_id: PluginRegistryId::from_uuid(arguments.registry_id),
            target_host_id: NodeId::from_uuid(arguments.target_host_id),
            workspace_scope: arguments.workspace_scope,
            selection: arguments.selection,
            policy_digest,
            desired_state: arguments.desired_state,
            actor_id: actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
            requested_at: Utc::now(),
        })
        .await?
    {
        Ok(result) => {
            let status = if result.replayed { 200 } else { 201 };
            tool_result::success(
                status,
                PluginAssignmentMutationResponse {
                    assignment: PluginAssignmentResponse::from(result.assignment),
                    replayed: result.replayed,
                },
                request_id,
            )
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_plan_projection(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: PluginPlanProjectionArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetPluginPlanProjection {
            organization_id,
            projection_id: PluginPlanProjectionId::from_uuid(arguments.projection_id),
        })
        .await?
    {
        Ok(projection) => tool_result::success(
            200,
            PluginPlanProjectionResponse::from(projection),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn confirm_plan_projection(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    arguments: ConfirmPluginPlanProjectionArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ConfirmPluginPlanProjection {
            organization_id,
            projection_id: PluginPlanProjectionId::from_uuid(arguments.projection_id),
            confirmation: arguments.confirmation,
            confirmed_at: Utc::now(),
        })
        .await?
    {
        Ok(projection) => tool_result::success(
            200,
            PluginPlanProjectionResponse::from(projection),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn search_catalog(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: PluginCatalogSearchArguments,
    request_id: Uuid,
    cached: bool,
) -> Result<Value> {
    let result = if cached {
        bus.execute(SearchCachedPluginCatalog {
            organization_id,
            registry_id: PluginRegistryId::from_uuid(arguments.registry_id),
            host: arguments.request.host,
            search: arguments.request.search,
        })
        .await?
    } else {
        bus.execute(SearchPluginCatalog {
            organization_id,
            registry_id: PluginRegistryId::from_uuid(arguments.registry_id),
            host: arguments.request.host,
            search: arguments.request.search,
        })
        .await?
    };
    match result {
        Ok(page) => tool_result::success(200, page, request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn inspect_catalog(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: PluginCatalogInspectArguments,
    request_id: Uuid,
    cached: bool,
) -> Result<Value> {
    let request = arguments.request;
    let result = if cached {
        bus.execute(InspectCachedPluginCatalog {
            organization_id,
            registry_id: PluginRegistryId::from_uuid(arguments.registry_id),
            host: request.host,
            package_id: request.package_id,
            version: request.version,
            channel: request.channel,
        })
        .await?
    } else {
        bus.execute(InspectPluginCatalog {
            organization_id,
            registry_id: PluginRegistryId::from_uuid(arguments.registry_id),
            host: request.host,
            package_id: request.package_id,
            version: request.version,
            channel: request.channel,
        })
        .await?
    };
    match result {
        Ok(inspection) => tool_result::success(200, inspection, request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}
