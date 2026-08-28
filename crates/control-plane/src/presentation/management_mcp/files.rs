use super::{arguments, tool_result};
use crate::modules::files::presentation::{
    UserFileMutationResponse, UserFileQuotaResponse, UserFileResponse,
};
use crate::modules::files::{
    GetUserFile, GetUserFileQuota, ListUserFiles, ReserveUserFile, TombstoneUserFile,
    UserFileTransition,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, ProjectId, UserFileId};
use a3s_boot::{CommandBus, QueryBus, Result};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReserveUserFileArguments {
    project_id: Uuid,
    admission_acl: String,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListUserFilesArguments {
    project_id: Uuid,
    #[serde(
        default = "arguments::default_list_limit",
        deserialize_with = "arguments::deserialize_list_limit"
    )]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserFileArguments {
    project_id: Uuid,
    user_file_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TombstoneUserFileArguments {
    project_id: Uuid,
    user_file_id: Uuid,
    #[serde(deserialize_with = "arguments::deserialize_expected_version")]
    expected_version: u64,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

pub async fn reserve(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ReserveUserFileArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ReserveUserFile {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            admission_acl: arguments.admission_acl,
            actor_principal_id,
            resource_access,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            UserFileMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListUserFilesArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListUserFiles {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            limit: Some(arguments.limit),
            resource_access,
        })
        .await?
    {
        Ok(files) => tool_result::success(
            200,
            files
                .into_iter()
                .map(UserFileResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: UserFileArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetUserFile {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            user_file_id: UserFileId::from_uuid(arguments.user_file_id),
            resource_access,
        })
        .await?
    {
        Ok(file) => tool_result::success(200, UserFileResponse::from(file), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn tombstone(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: TombstoneUserFileArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(TombstoneUserFile(UserFileTransition {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            user_file_id: UserFileId::from_uuid(arguments.user_file_id),
            expected_version: arguments.expected_version,
            actor_principal_id,
            resource_access,
            idempotency_key: arguments.idempotency_key,
            request_id,
        }))
        .await?
    {
        Ok(result) => tool_result::success(200, UserFileMutationResponse::from(result), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn quota(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetUserFileQuota {
            organization_id,
            resource_access,
        })
        .await?
    {
        Ok(quota) => tool_result::success(200, UserFileQuotaResponse::from(quota), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}
