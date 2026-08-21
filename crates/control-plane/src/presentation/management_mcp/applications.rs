use super::tool_result;
use crate::modules::applications::presentation::{
    ApplicationInvocationMutationResponse, ApplicationInvocationResponse,
    ApplicationMessageResponse, ApplicationMutationResponse, ApplicationReleaseResponse,
    ApplicationResponse, ApplicationSessionMutationResponse, ApplicationSessionResponse,
};
use crate::modules::applications::{
    AdmitApplicationInvocation, AdmitApplicationSession, ApplicationResponseMode,
    CreateApplication, GetApplication, GetApplicationInvocation, GetApplicationRelease,
    GetApplicationSession, ListApplicationReleases, ListApplications, PublishApplicationRelease,
    ReplayApplicationSession,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationInvocationId, ApplicationReleaseId, ApplicationSessionId,
    EnvironmentId, OntologyId, OntologyRevisionId, OrganizationId, PrincipalId, ProjectId,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenApplicationSessionArguments {
    project_id: Uuid,
    application_id: Uuid,
    release_id: Uuid,
    #[serde(default = "empty_object")]
    initial_variables: Value,
    #[serde(deserialize_with = "super::arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationSessionArguments {
    project_id: Uuid,
    application_id: Uuid,
    session_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestApplicationInvocationArguments {
    project_id: Uuid,
    application_id: Uuid,
    session_id: Uuid,
    ontology_id: Uuid,
    ontology_revision_id: Uuid,
    environment_id: Option<Uuid>,
    response_mode: String,
    input: Value,
    timeout_seconds: Option<u64>,
    #[serde(deserialize_with = "super::arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationInvocationArguments {
    project_id: Uuid,
    application_id: Uuid,
    session_id: Uuid,
    invocation_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListApplicationMessagesArguments {
    project_id: Uuid,
    application_id: Uuid,
    session_id: Uuid,
    #[serde(default)]
    after_sequence: u64,
    #[serde(
        default = "super::arguments::default_list_limit",
        deserialize_with = "super::arguments::deserialize_list_limit"
    )]
    limit: usize,
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

pub async fn open_session(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: OpenApplicationSessionArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(AdmitApplicationSession {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            release_id: ApplicationReleaseId::from_uuid(arguments.release_id),
            initial_variables: arguments.initial_variables,
            actor_principal_id,
            resource_access,
            idempotency_key: arguments.idempotency_key,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            ApplicationSessionMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_session(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ApplicationSessionArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetApplicationSession {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            actor_principal_id,
            resource_access,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            200,
            ApplicationSessionResponse::from(result.session),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn request_invocation(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: RequestApplicationInvocationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    let response_mode = match ApplicationResponseMode::parse(&arguments.response_mode) {
        Ok(value) => value,
        Err(error) => {
            return tool_result::application_error(ApplicationError::Invalid(error), request_id)
        }
    };
    match bus
        .execute(AdmitApplicationInvocation {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            ontology_id: OntologyId::from_uuid(arguments.ontology_id),
            ontology_revision_id: OntologyRevisionId::from_uuid(arguments.ontology_revision_id),
            environment_id: arguments.environment_id.map(EnvironmentId::from_uuid),
            response_mode,
            input: arguments.input,
            timeout_seconds: arguments.timeout_seconds,
            actor_principal_id,
            resource_access,
            idempotency_key: arguments.idempotency_key,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            ApplicationInvocationMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_invocation(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ApplicationInvocationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetApplicationInvocation {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            invocation_id: ApplicationInvocationId::from_uuid(arguments.invocation_id),
            actor_principal_id,
            resource_access,
        })
        .await?
    {
        Ok(invocation) => tool_result::success(
            200,
            ApplicationInvocationResponse::from(invocation),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_messages(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ListApplicationMessagesArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ReplayApplicationSession {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            after_sequence: arguments.after_sequence,
            limit: Some(arguments.limit),
            actor_principal_id,
            resource_access,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            200,
            result
                .messages
                .into_iter()
                .map(ApplicationMessageResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}
