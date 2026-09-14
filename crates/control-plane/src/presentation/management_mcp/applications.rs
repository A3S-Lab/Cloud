use super::tool_result;
use crate::access_projection::application_access;
use crate::modules::applications::presentation::{
    ApplicationAnnotationMutationResponse, ApplicationAnnotationResponse,
    ApplicationBlockingObservationResponse,
    ApplicationStreamingObservationResponse,
    ApplicationAsynchronousObservationResponse,
    ApplicationFeedbackMutationResponse, ApplicationFeedbackResponse,
    ApplicationInvocationCancellationResponse, ApplicationInvocationMutationResponse,
    ApplicationInvocationResponse, ApplicationMessageResponse,
    ApplicationMessageCitationMutationResponse, ApplicationMessageCitationResponse,
    ApplicationMessageFileReferenceMutationResponse, ApplicationMessageFileReferenceResponse,
    ApplicationMessageVariantMutationResponse, ApplicationMessageVariantResponse,
    ApplicationMutationResponse, ApplicationReleaseResponse, ApplicationResponse,
    ApplicationSessionMutationResponse, ApplicationSessionReplayResponse,
    ApplicationSessionResponse,
};
use crate::modules::applications::{
    AdmitApplicationInvocation, AdmitApplicationSession, ApplicationFeedbackRating,
    ApplicationResponseMode, CancelApplicationInvocation, CloseApplicationSession,
    CreateApplication, CreateApplicationAnnotation, CreateApplicationFeedback,
    CreateApplicationMessageCitation, CreateApplicationMessageFileReference,
    CreateApplicationMessageVariant, GetApplication,
    GetApplicationAnnotation, GetApplicationFeedback, GetApplicationInvocation,
    ObserveApplicationBlockingInvocation,
    ObserveApplicationStreamingInvocation,
    ObserveApplicationAsynchronousInvocation,
    GetApplicationMessageCitation, GetApplicationMessageFileReference,
    GetApplicationMessageVariant,
    GetApplicationRelease, GetApplicationSession, ListApplicationAnnotationsBySession,
    ListApplicationFeedbackBySession, ListApplicationMessageCitationsBySession,
    ListApplicationMessageFileReferencesBySession,
    ListApplicationMessageVariantsBySession,
    ListApplicationReleases, ListApplications, PublishApplicationRelease, ReplayApplicationSession,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    ApplicationAnnotationId, ApplicationFeedbackId, ApplicationId, ApplicationInvocationId,
    ApplicationMessageCitationId, ApplicationMessageFileReferenceId, ApplicationMessageId,
    ApplicationMessageVariantId,
    ApplicationReleaseId, ApplicationSessionId, KnowledgeBaseId, KnowledgeBaseRevisionId,
    KnowledgeChunkId, KnowledgeDocumentId, Sha256Digest, UserFileId,
    EnvironmentId, OntologyId, OntologyRevisionId, OrganizationId, PrincipalId, ProjectId,
};
use a3s_boot::{CommandBus, QueryBus, Result};
use chrono::Utc;
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
pub struct CloseApplicationSessionArguments {
    project_id: Uuid,
    application_id: Uuid,
    session_id: Uuid,
    #[serde(deserialize_with = "super::arguments::deserialize_expected_version")]
    expected_version: u64,
    #[serde(
        rename = "idempotencyKey",
        deserialize_with = "super::arguments::deserialize_idempotency_key"
    )]
    _idempotency_key: String,
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
pub struct ApplicationStreamingObservationArguments {
    project_id: Uuid,
    application_id: Uuid,
    session_id: Uuid,
    invocation_id: Uuid,
    #[serde(default)]
    after_sequence: u64,
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
pub struct CancelApplicationInvocationArguments {
    project_id: Uuid,
    application_id: Uuid,
    session_id: Uuid,
    invocation_id: Uuid,
    #[serde(deserialize_with = "super::arguments::deserialize_expected_version")]
    expected_version: u64,
    #[serde(
        rename = "idempotencyKey",
        deserialize_with = "super::arguments::deserialize_idempotency_key"
    )]
    _idempotency_key: String,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateApplicationFeedbackArguments {
    project_id: Uuid,
    application_id: Uuid,
    session_id: Uuid,
    rating: String,
    #[serde(default)]
    comment: Option<String>,
    #[serde(default)]
    source_message_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationFeedbackArguments {
    project_id: Uuid,
    application_id: Uuid,
    session_id: Uuid,
    feedback_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateApplicationAnnotationArguments {
    project_id: Uuid,
    application_id: Uuid,
    session_id: Uuid,
    content: Value,
    #[serde(default)]
    source_message_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationAnnotationArguments {
    project_id: Uuid,
    application_id: Uuid,
    session_id: Uuid,
    annotation_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateApplicationMessageVariantArguments {
    project_id: Uuid,
    application_id: Uuid,
    session_id: Uuid,
    source_message_id: Uuid,
    #[serde(default)]
    instruction: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateApplicationMessageFileReferenceArguments {
    project_id: Uuid,
    application_id: Uuid,
    session_id: Uuid,
    message_id: Uuid,
    user_file_id: Uuid,
    content_digest: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationMessageFileReferenceArguments {
    project_id: Uuid,
    application_id: Uuid,
    session_id: Uuid,
    reference_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateApplicationMessageCitationArguments {
    project_id: Uuid,
    application_id: Uuid,
    session_id: Uuid,
    message_id: Uuid,
    knowledge_base_id: Uuid,
    knowledge_base_revision_id: Uuid,
    knowledge_document_id: Uuid,
    knowledge_chunk_id: Uuid,
    #[serde(default)]
    excerpt: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationMessageCitationArguments {
    project_id: Uuid,
    application_id: Uuid,
    session_id: Uuid,
    citation_id: Uuid,
}


#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationMessageVariantArguments {
    project_id: Uuid,
    application_id: Uuid,
    session_id: Uuid,
    variant_id: Uuid,
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
            access: application_access(&resource_access),
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
            access: application_access(&resource_access),
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
            access: application_access(&resource_access),
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
            access: application_access(&resource_access),
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
            access: application_access(&resource_access),
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
            access: application_access(&resource_access),
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
            access: application_access(&resource_access),
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
            access: application_access(&resource_access),
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

pub async fn close_session(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CloseApplicationSessionArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    let CloseApplicationSessionArguments {
        project_id,
        application_id,
        session_id,
        expected_version,
        _idempotency_key: _,
    } = arguments;
    match bus
        .execute(CloseApplicationSession {
            organization_id,
            project_id: ProjectId::from_uuid(project_id),
            application_id: ApplicationId::from_uuid(application_id),
            session_id: ApplicationSessionId::from_uuid(session_id),
            expected_version,
            actor_principal_id,
            access: application_access(&resource_access),
            closed_at: Utc::now(),
        })
        .await?
    {
        Ok(result) => tool_result::success(
            200,
            ApplicationSessionMutationResponse::from(result),
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
            return tool_result::application_error(ApplicationError::Invalid(error), request_id);
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
            access: application_access(&resource_access),
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
            access: application_access(&resource_access),
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

pub async fn cancel_invocation(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CancelApplicationInvocationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    let CancelApplicationInvocationArguments {
        project_id,
        application_id,
        session_id,
        invocation_id,
        expected_version,
        _idempotency_key: _,
    } = arguments;
    match bus
        .execute(CancelApplicationInvocation {
            organization_id,
            project_id: ProjectId::from_uuid(project_id),
            application_id: ApplicationId::from_uuid(application_id),
            session_id: ApplicationSessionId::from_uuid(session_id),
            invocation_id: ApplicationInvocationId::from_uuid(invocation_id),
            expected_version,
            actor_principal_id,
            access: application_access(&resource_access),
            requested_at: Utc::now(),
        })
        .await?
    {
        Ok(result) => tool_result::success(
            200,
            ApplicationInvocationCancellationResponse::from(result),
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
            access: application_access(&resource_access),
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

pub async fn replay_session(
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
            access: application_access(&resource_access),
        })
        .await?
    {
        Ok(result) => tool_result::success(
            200,
            ApplicationSessionReplayResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn create_feedback(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateApplicationFeedbackArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    let rating = match ApplicationFeedbackRating::parse(&arguments.rating) {
        Ok(rating) => rating,
        Err(error) => {
            return tool_result::application_error(ApplicationError::Invalid(error), request_id);
        }
    };
    match bus
        .execute(CreateApplicationFeedback {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            source_message_id: arguments
                .source_message_id
                .map(ApplicationMessageId::from_uuid),
            rating,
            comment: arguments.comment,
            actor_principal_id,
            access: application_access(&resource_access),
            created_at: Utc::now(),
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            ApplicationFeedbackMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_feedback(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ApplicationSessionArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListApplicationFeedbackBySession {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            actor_principal_id,
            access: application_access(&resource_access),
        })
        .await?
    {
        Ok(items) => tool_result::success(
            200,
            items
                .into_iter()
                .map(ApplicationFeedbackResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_feedback(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ApplicationFeedbackArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetApplicationFeedback {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            feedback_id: ApplicationFeedbackId::from_uuid(arguments.feedback_id),
            actor_principal_id,
            access: application_access(&resource_access),
        })
        .await?
    {
        Ok(feedback) => {
            tool_result::success(200, ApplicationFeedbackResponse::from(feedback), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn create_annotation(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateApplicationAnnotationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateApplicationAnnotation {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            source_message_id: arguments
                .source_message_id
                .map(ApplicationMessageId::from_uuid),
            content: arguments.content,
            actor_principal_id,
            access: application_access(&resource_access),
            created_at: Utc::now(),
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            ApplicationAnnotationMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_annotations(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ApplicationSessionArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListApplicationAnnotationsBySession {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            actor_principal_id,
            access: application_access(&resource_access),
        })
        .await?
    {
        Ok(items) => tool_result::success(
            200,
            items
                .into_iter()
                .map(ApplicationAnnotationResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_annotation(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ApplicationAnnotationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetApplicationAnnotation {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            annotation_id: ApplicationAnnotationId::from_uuid(arguments.annotation_id),
            actor_principal_id,
            access: application_access(&resource_access),
        })
        .await?
    {
        Ok(annotation) => tool_result::success(
            200,
            ApplicationAnnotationResponse::from(annotation),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn create_message_file_reference(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateApplicationMessageFileReferenceArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    let content_digest = match Sha256Digest::parse(arguments.content_digest) {
        Ok(value) => value,
        Err(error) => {
            return tool_result::application_error(
                crate::modules::shared_kernel::application::ApplicationError::Invalid(error),
                request_id,
            );
        }
    };
    match bus
        .execute(CreateApplicationMessageFileReference {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            message_id: ApplicationMessageId::from_uuid(arguments.message_id),
            user_file_id: UserFileId::from_uuid(arguments.user_file_id),
            content_digest,
            actor_principal_id,
            access: application_access(&resource_access),
            created_at: Utc::now(),
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            ApplicationMessageFileReferenceMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_message_file_references(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ApplicationSessionArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListApplicationMessageFileReferencesBySession {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            actor_principal_id,
            access: application_access(&resource_access),
        })
        .await?
    {
        Ok(items) => tool_result::success(
            200,
            items
                .into_iter()
                .map(ApplicationMessageFileReferenceResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_message_file_reference(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ApplicationMessageFileReferenceArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetApplicationMessageFileReference {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            reference_id: ApplicationMessageFileReferenceId::from_uuid(arguments.reference_id),
            actor_principal_id,
            access: application_access(&resource_access),
        })
        .await?
    {
        Ok(reference) => tool_result::success(
            200,
            ApplicationMessageFileReferenceResponse::from(reference),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn create_message_citation(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateApplicationMessageCitationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateApplicationMessageCitation {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            message_id: ApplicationMessageId::from_uuid(arguments.message_id),
            knowledge_base_id: KnowledgeBaseId::from_uuid(arguments.knowledge_base_id),
            knowledge_base_revision_id: KnowledgeBaseRevisionId::from_uuid(
                arguments.knowledge_base_revision_id,
            ),
            knowledge_document_id: KnowledgeDocumentId::from_uuid(arguments.knowledge_document_id),
            knowledge_chunk_id: KnowledgeChunkId::from_uuid(arguments.knowledge_chunk_id),
            excerpt: arguments.excerpt,
            actor_principal_id,
            access: application_access(&resource_access),
            created_at: Utc::now(),
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            ApplicationMessageCitationMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_message_citations(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ApplicationSessionArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListApplicationMessageCitationsBySession {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            actor_principal_id,
            access: application_access(&resource_access),
        })
        .await?
    {
        Ok(items) => tool_result::success(
            200,
            items
                .into_iter()
                .map(ApplicationMessageCitationResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_message_citation(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ApplicationMessageCitationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetApplicationMessageCitation {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            citation_id: ApplicationMessageCitationId::from_uuid(arguments.citation_id),
            actor_principal_id,
            access: application_access(&resource_access),
        })
        .await?
    {
        Ok(citation) => tool_result::success(
            200,
            ApplicationMessageCitationResponse::from(citation),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn observe_blocking_invocation(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ApplicationInvocationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ObserveApplicationBlockingInvocation {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            invocation_id: ApplicationInvocationId::from_uuid(arguments.invocation_id),
            actor_principal_id,
            access: application_access(&resource_access),
            observed_at: Utc::now(),
        })
        .await?
    {
        Ok(observation) => tool_result::success(
            200,
            ApplicationBlockingObservationResponse::from(observation),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn observe_streaming_invocation(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ApplicationStreamingObservationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ObserveApplicationStreamingInvocation {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            invocation_id: ApplicationInvocationId::from_uuid(arguments.invocation_id),
            actor_principal_id,
            access: application_access(&resource_access),
            after_sequence: arguments.after_sequence,
            observed_at: Utc::now(),
        })
        .await?
    {
        Ok(observation) => tool_result::success(
            200,
            ApplicationStreamingObservationResponse::from(observation),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn observe_asynchronous_invocation(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ApplicationInvocationArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ObserveApplicationAsynchronousInvocation {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            invocation_id: ApplicationInvocationId::from_uuid(arguments.invocation_id),
            actor_principal_id,
            access: application_access(&resource_access),
            observed_at: Utc::now(),
        })
        .await?
    {
        Ok(observation) => tool_result::success(
            200,
            ApplicationAsynchronousObservationResponse::from(observation),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn create_message_variant(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateApplicationMessageVariantArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateApplicationMessageVariant {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            source_message_id: ApplicationMessageId::from_uuid(arguments.source_message_id),
            instruction: arguments.instruction,
            actor_principal_id,
            access: application_access(&resource_access),
            created_at: Utc::now(),
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            ApplicationMessageVariantMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_message_variants(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ApplicationSessionArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListApplicationMessageVariantsBySession {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            actor_principal_id,
            access: application_access(&resource_access),
        })
        .await?
    {
        Ok(items) => tool_result::success(
            200,
            items
                .into_iter()
                .map(ApplicationMessageVariantResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_message_variant(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ApplicationMessageVariantArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetApplicationMessageVariant {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            application_id: ApplicationId::from_uuid(arguments.application_id),
            session_id: ApplicationSessionId::from_uuid(arguments.session_id),
            variant_id: ApplicationMessageVariantId::from_uuid(arguments.variant_id),
            actor_principal_id,
            access: application_access(&resource_access),
        })
        .await?
    {
        Ok(variant) => tool_result::success(
            200,
            ApplicationMessageVariantResponse::from(variant),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}
