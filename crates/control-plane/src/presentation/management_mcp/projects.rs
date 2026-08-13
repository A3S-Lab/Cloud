use super::arguments::EmptyArguments;
use super::tool_result;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::projects::presentation::{
    EnvironmentListItemResponse, EnvironmentResponse, ProjectAttributionMutationResponse,
    ProjectAttributionProfileResponse, ProjectListItemResponse, ProjectResponse,
};
use crate::modules::projects::{
    CreateEnvironment, CreateProject, GetProjectAttribution, ListEnvironments, ListProjects,
    UpdateProjectAttribution,
};
use crate::modules::shared_kernel::domain::{
    OrganizationId, PrincipalId, ProjectAttributionProfileId, ProjectId,
};
use a3s_boot::{CommandBus, QueryBus, Result};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateProjectArguments {
    name: String,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateEnvironmentArguments {
    project_id: Uuid,
    name: String,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectArguments {
    project_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetProjectAttributionArguments {
    project_id: Uuid,
    #[serde(default)]
    attribution_profile_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateProjectAttributionArguments {
    project_id: Uuid,
    expected_version: u64,
    business_owner_reference: String,
    #[serde(default)]
    cost_attribution_code: Option<String>,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    idempotency_key: String,
}

pub async fn create_project(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    arguments: CreateProjectArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateProject {
            organization_id,
            name: arguments.name,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => {
            let status = if result.replayed { 200 } else { 201 };
            tool_result::success(status, ProjectResponse::from(result), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn create_environment(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    arguments: CreateEnvironmentArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateEnvironment {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            name: arguments.name,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => {
            let status = if result.replayed { 200 } else { 201 };
            tool_result::success(status, EnvironmentResponse::from(result), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_projects(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    _arguments: EmptyArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListProjects {
            organization_id,
            resource_access,
        })
        .await?
    {
        Ok(projects) => tool_result::success(
            200,
            projects
                .into_iter()
                .map(ProjectListItemResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_environments(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ProjectArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListEnvironments {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            resource_access,
        })
        .await?
    {
        Ok(environments) => tool_result::success(
            200,
            environments
                .into_iter()
                .map(EnvironmentListItemResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_project_attribution(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: GetProjectAttributionArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetProjectAttribution {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            attribution_profile_id: arguments
                .attribution_profile_id
                .map(ProjectAttributionProfileId::from_uuid),
            resource_access,
        })
        .await?
    {
        Ok(profile) => tool_result::success(
            200,
            ProjectAttributionProfileResponse::from(profile),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn update_project_attribution(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: UpdateProjectAttributionArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(UpdateProjectAttribution {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            actor_principal_id,
            resource_access,
            expected_project_version: arguments.expected_version,
            business_owner_reference: arguments.business_owner_reference,
            cost_attribution_code: arguments.cost_attribution_code,
            labels: arguments.labels,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => {
            let status = if result.replayed { 200 } else { 201 };
            tool_result::success(
                status,
                ProjectAttributionMutationResponse::from(result),
                request_id,
            )
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}
