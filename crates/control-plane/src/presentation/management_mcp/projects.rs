use super::tool_result;
use crate::modules::projects::presentation::{
    EnvironmentListItemResponse, EnvironmentResponse, ProjectListItemResponse, ProjectResponse,
};
use crate::modules::projects::{CreateEnvironment, CreateProject, ListEnvironments, ListProjects};
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use a3s_boot::{CommandBus, QueryBus, Result};
use serde::Deserialize;
use serde_json::Value;
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
#[serde(deny_unknown_fields)]
pub struct EmptyArguments {}

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
    request_id: Uuid,
) -> Result<Value> {
    match bus.execute(ListProjects { organization_id }).await? {
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
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListEnvironments {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
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
