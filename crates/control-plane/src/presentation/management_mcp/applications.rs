use super::tool_result;
use crate::modules::applications::presentation::{
    ApplicationMutationResponse, ApplicationReleaseResponse, ApplicationResponse,
};
use crate::modules::applications::{
    CreateApplication, GetApplication, GetApplicationRelease, ListApplicationReleases,
    ListApplications, PublishApplicationRelease,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationReleaseId, OrganizationId, PrincipalId, ProjectId,
};
use a3s_boot::{CommandBus, QueryBus, Result};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateApplicationArguments {
    project_id: Uuid,
    name: String,
    #[serde(default)]
    description: String,
    release_acl: String,
    #[serde(deserialize_with = "super::arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishApplicationReleaseArguments {
    project_id: Uuid,
    application_id: Uuid,
    #[serde(deserialize_with = "super::arguments::deserialize_expected_version")]
    expected_version: u64,
    release_acl: String,
    #[serde(deserialize_with = "super::arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListApplicationsArguments {
    project_id: Uuid,
    #[serde(
        default = "super::arguments::default_list_limit",
        deserialize_with = "super::arguments::deserialize_list_limit"
    )]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationArguments {
    project_id: Uuid,
    application_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListApplicationReleasesArguments {
    project_id: Uuid,
    application_id: Uuid,
    #[serde(
        default = "super::arguments::default_list_limit",
        deserialize_with = "super::arguments::deserialize_list_limit"
    )]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationReleaseArguments {
    project_id: Uuid,
    application_id: Uuid,
    release_id: Uuid,
}

pub async fn create(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateApplicationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateApplication {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            name: arguments.name,
            description: arguments.description,
            release_acl: arguments.release_acl,
            actor_principal_id,
            resource_access,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            ApplicationMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn publish_release(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: PublishApplicationReleaseArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(PublishApplicationRelease {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            expected_version: arguments.expected_version,
            release_acl: arguments.release_acl,
            actor_principal_id,
            resource_access,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            ApplicationMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListApplicationsArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListApplications {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            limit: Some(arguments.limit),
            resource_access,
        })
        .await?
    {
        Ok(applications) => tool_result::success(
            200,
            applications
                .into_iter()
                .map(ApplicationResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ApplicationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetApplication {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            resource_access,
        })
        .await?
    {
        Ok(application) => {
            tool_result::success(200, ApplicationResponse::from(application), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_releases(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListApplicationReleasesArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListApplicationReleases {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            limit: Some(arguments.limit),
            resource_access,
        })
        .await?
    {
        Ok(releases) => tool_result::success(
            200,
            releases
                .into_iter()
                .map(ApplicationReleaseResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_release(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ApplicationReleaseArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetApplicationRelease {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            release_id: ApplicationReleaseId::from_uuid(arguments.release_id),
            resource_access,
        })
        .await?
    {
        Ok(release) => {
            tool_result::success(200, ApplicationReleaseResponse::from(release), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}
