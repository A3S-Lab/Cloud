use super::resource_access::project;
use crate::modules::applications::ApplicationAccess;
use crate::modules::applications::domain::{
    ApplicationMessage, ApplicationMessageFileReference, ApplicationSession,
    IApplicationMessageFileReferenceRepository, IApplicationSessionRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationMessageFileReferenceId, ApplicationMessageId, ApplicationSessionId,
    OrganizationId, PrincipalId, ProjectId, RepositoryError, Sha256Digest, UserFileId,
};
use a3s_boot::{Command, CommandHandler, CqrsContext, Query, QueryHandler};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;

/// Create one immutable Applications-owned Input message file reference.
#[derive(Debug, Clone)]
pub struct CreateApplicationMessageFileReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub message_id: ApplicationMessageId,
    pub user_file_id: UserFileId,
    pub content_digest: Sha256Digest,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
    pub created_at: DateTime<Utc>,
}

impl Command for CreateApplicationMessageFileReference {
    type Output = ApplicationResult<ApplicationMessageFileReferenceMutationResult>;
}

#[derive(Debug, Clone)]
pub struct GetApplicationMessageFileReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub reference_id: ApplicationMessageFileReferenceId,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
}

impl Query for GetApplicationMessageFileReference {
    type Output = ApplicationResult<ApplicationMessageFileReference>;
}

#[derive(Debug, Clone)]
pub struct ListApplicationMessageFileReferencesBySession {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
}

impl Query for ListApplicationMessageFileReferencesBySession {
    type Output = ApplicationResult<Vec<ApplicationMessageFileReference>>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationMessageFileReferenceMutationResult {
    pub reference: ApplicationMessageFileReference,
    pub replayed: bool,
}

pub struct CreateApplicationMessageFileReferenceHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
    references: Arc<dyn IApplicationMessageFileReferenceRepository>,
}

impl CreateApplicationMessageFileReferenceHandler {
    pub fn new(
        sessions: Arc<dyn IApplicationSessionRepository>,
        references: Arc<dyn IApplicationMessageFileReferenceRepository>,
    ) -> Self {
        Self {
            sessions,
            references,
        }
    }
}

impl CommandHandler<CreateApplicationMessageFileReference>
    for CreateApplicationMessageFileReferenceHandler
{
    fn execute(
        &self,
        command: CreateApplicationMessageFileReference,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationMessageFileReferenceMutationResult>>,
    > {
        let sessions = Arc::clone(&self.sessions);
        let references = Arc::clone(&self.references);
        Box::pin(async move {
            if let Err(error) = authorize_actor(
                &command.access,
                command.project_id,
                command.actor_principal_id,
            ) {
                return Ok(Err(error));
            }
            let session = match load_session(
                sessions.as_ref(),
                command.organization_id,
                command.project_id,
                command.application_id,
                command.session_id,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let source_message =
                match load_source_message(sessions.as_ref(), &session, command.message_id).await {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(error)),
                };
            let reference = match ApplicationMessageFileReference::create(
                &session,
                &source_message,
                command.user_file_id,
                command.content_digest,
                command.created_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match references.create_message_file_reference(reference).await {
                Ok(value) => Ok(Ok(ApplicationMessageFileReferenceMutationResult {
                    reference: value.value,
                    replayed: value.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct GetApplicationMessageFileReferenceHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
    references: Arc<dyn IApplicationMessageFileReferenceRepository>,
}

impl GetApplicationMessageFileReferenceHandler {
    pub fn new(
        sessions: Arc<dyn IApplicationSessionRepository>,
        references: Arc<dyn IApplicationMessageFileReferenceRepository>,
    ) -> Self {
        Self {
            sessions,
            references,
        }
    }
}

impl QueryHandler<GetApplicationMessageFileReference>
    for GetApplicationMessageFileReferenceHandler
{
    fn execute(
        &self,
        query: GetApplicationMessageFileReference,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationMessageFileReference>>,
    > {
        let sessions = Arc::clone(&self.sessions);
        let references = Arc::clone(&self.references);
        Box::pin(async move {
            if let Err(error) =
                authorize_actor(&query.access, query.project_id, query.actor_principal_id)
            {
                return Ok(Err(error));
            }
            if let Err(error) = load_session(
                sessions.as_ref(),
                query.organization_id,
                query.project_id,
                query.application_id,
                query.session_id,
            )
            .await
            {
                return Ok(Err(error));
            }
            match references
                .find_message_file_reference(
                    query.organization_id,
                    query.project_id,
                    query.application_id,
                    query.reference_id,
                )
                .await
            {
                Ok(Some(reference)) if reference.session_id == query.session_id => {
                    Ok(Ok(reference))
                }
                Ok(Some(_)) | Ok(None) | Err(RepositoryError::NotFound) => {
                    Ok(Err(reference_not_found()))
                }
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct ListApplicationMessageFileReferencesBySessionHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
    references: Arc<dyn IApplicationMessageFileReferenceRepository>,
}

impl ListApplicationMessageFileReferencesBySessionHandler {
    pub fn new(
        sessions: Arc<dyn IApplicationSessionRepository>,
        references: Arc<dyn IApplicationMessageFileReferenceRepository>,
    ) -> Self {
        Self {
            sessions,
            references,
        }
    }
}

impl QueryHandler<ListApplicationMessageFileReferencesBySession>
    for ListApplicationMessageFileReferencesBySessionHandler
{
    fn execute(
        &self,
        query: ListApplicationMessageFileReferencesBySession,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<ApplicationMessageFileReference>>>,
    > {
        let sessions = Arc::clone(&self.sessions);
        let references = Arc::clone(&self.references);
        Box::pin(async move {
            if let Err(error) =
                authorize_actor(&query.access, query.project_id, query.actor_principal_id)
            {
                return Ok(Err(error));
            }
            if let Err(error) = load_session(
                sessions.as_ref(),
                query.organization_id,
                query.project_id,
                query.application_id,
                query.session_id,
            )
            .await
            {
                return Ok(Err(error));
            }
            match references
                .list_message_file_references_by_session(
                    query.organization_id,
                    query.project_id,
                    query.application_id,
                    query.session_id,
                )
                .await
            {
                Ok(values) => Ok(Ok(values)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

fn authorize_actor(
    access: &ApplicationAccess,
    project_id: ProjectId,
    actor_principal_id: PrincipalId,
) -> ApplicationResult<()> {
    if actor_principal_id.as_uuid().is_nil() || project_id.as_uuid().is_nil() {
        return Err(ApplicationError::Invalid(
            "Application message file reference request identity is invalid".into(),
        ));
    }
    project(project_id, access)
}

async fn load_session(
    sessions: &dyn IApplicationSessionRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
    session_id: ApplicationSessionId,
) -> ApplicationResult<ApplicationSession> {
    match sessions
        .find_session(organization_id, project_id, application_id, session_id)
        .await
    {
        Ok(Some(session))
            if session.organization_id == organization_id
                && session.project_id == project_id
                && session.application_id == application_id =>
        {
            Ok(session)
        }
        Ok(Some(_)) | Ok(None) | Err(RepositoryError::NotFound) => Err(session_not_found()),
        Err(error) => Err(error.into()),
    }
}

async fn load_source_message(
    sessions: &dyn IApplicationSessionRepository,
    session: &ApplicationSession,
    message_id: ApplicationMessageId,
) -> ApplicationResult<ApplicationMessage> {
    match sessions
        .find_message(
            session.organization_id,
            session.project_id,
            session.application_id,
            message_id,
        )
        .await
    {
        Ok(Some(message)) => Ok(message),
        Ok(None) | Err(RepositoryError::NotFound) => Err(ApplicationError::Invalid(
            "Application message file reference source message was not found".into(),
        )),
        Err(error) => Err(error.into()),
    }
}

fn session_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application session not found".into())
}

fn reference_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application message file reference not found".into())
}
