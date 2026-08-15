use super::tool_result;
use crate::modules::connectors::presentation::{
    ConnectorProfileMutationResponse, ConnectorProfileRecordResponse, ConnectorProfileResponse,
    ConnectorRevisionResponse,
};
use crate::modules::connectors::{
    CreateConnectorProfile, GetConnectorProfile, GetConnectorRevision, ListConnectorProfiles,
    ListConnectorRevisions, ReviseConnectorProfile,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, OrganizationId, PrincipalId, ProjectId,
};
use a3s_boot::{CommandBus, QueryBus, Result};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateConnectorProfileArguments {
    project_id: Uuid,
    environment_id: Uuid,
    name: String,
    definition_acl: String,
    #[serde(deserialize_with = "super::arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviseConnectorProfileArguments {
    project_id: Uuid,
    environment_id: Uuid,
    profile_id: Uuid,
    expected_version: u64,
    definition_acl: String,
    #[serde(deserialize_with = "super::arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListConnectorProfilesArguments {
    project_id: Uuid,
    environment_id: Uuid,
    #[serde(
        default = "super::arguments::default_list_limit",
        deserialize_with = "super::arguments::deserialize_list_limit"
    )]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectorProfileArguments {
    project_id: Uuid,
    environment_id: Uuid,
    profile_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListConnectorRevisionsArguments {
    project_id: Uuid,
    environment_id: Uuid,
    profile_id: Uuid,
    #[serde(
        default = "super::arguments::default_list_limit",
        deserialize_with = "super::arguments::deserialize_list_limit"
    )]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectorRevisionArguments {
    project_id: Uuid,
    environment_id: Uuid,
    profile_id: Uuid,
    revision_id: Uuid,
}

pub async fn create_profile(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateConnectorProfileArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateConnectorProfile {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            name: arguments.name,
            definition_acl: arguments.definition_acl,
            actor_principal_id,
            resource_access,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            ConnectorProfileMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn revise_profile(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ReviseConnectorProfileArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ReviseConnectorProfile {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            profile_id: ConnectorProfileId::from_uuid(arguments.profile_id),
            expected_version: arguments.expected_version,
            definition_acl: arguments.definition_acl,
            actor_principal_id,
            resource_access,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            ConnectorProfileMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_profiles(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListConnectorProfilesArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListConnectorProfiles {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            limit: arguments.limit,
            resource_access,
        })
        .await?
    {
        Ok(profiles) => tool_result::success(
            200,
            profiles
                .into_iter()
                .map(ConnectorProfileResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_profile(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ConnectorProfileArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetConnectorProfile {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            profile_id: ConnectorProfileId::from_uuid(arguments.profile_id),
            resource_access,
        })
        .await?
    {
        Ok(record) => tool_result::success(
            200,
            ConnectorProfileRecordResponse::from(record),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_revisions(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListConnectorRevisionsArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListConnectorRevisions {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            profile_id: ConnectorProfileId::from_uuid(arguments.profile_id),
            limit: arguments.limit,
            resource_access,
        })
        .await?
    {
        Ok(revisions) => tool_result::success(
            200,
            revisions
                .into_iter()
                .map(ConnectorRevisionResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_revision(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ConnectorRevisionArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetConnectorRevision {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            profile_id: ConnectorProfileId::from_uuid(arguments.profile_id),
            revision_id: ConnectorRevisionId::from_uuid(arguments.revision_id),
            resource_access,
        })
        .await?
    {
        Ok(revision) => {
            tool_result::success(200, ConnectorRevisionResponse::from(revision), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}
