use super::resource_access::project;
use crate::modules::applications::ApplicationAccess;
use crate::modules::applications::domain::{
    ApplicationMessage, ApplicationMessageCitation, ApplicationSession,
    IApplicationMessageCitationRepository, IApplicationSessionRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationMessageCitationId, ApplicationMessageId, ApplicationSessionId,
    KnowledgeBaseId, KnowledgeBaseRevisionId, KnowledgeChunkId, KnowledgeDocumentId,
    OrganizationId, PrincipalId, ProjectId, RepositoryError,
};
use a3s_boot::{Command, CommandHandler, CqrsContext, Query, QueryHandler};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;

/// Create one immutable Applications-owned Answer/FinalOutput message citation.
#[derive(Debug, Clone)]
pub struct CreateApplicationMessageCitation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub message_id: ApplicationMessageId,
    pub knowledge_base_id: KnowledgeBaseId,
    pub knowledge_base_revision_id: KnowledgeBaseRevisionId,
    pub knowledge_document_id: KnowledgeDocumentId,
    pub knowledge_chunk_id: KnowledgeChunkId,
    pub excerpt: Option<String>,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
    pub created_at: DateTime<Utc>,
}

impl Command for CreateApplicationMessageCitation {
    type Output = ApplicationResult<ApplicationMessageCitationMutationResult>;
}

#[derive(Debug, Clone)]
pub struct GetApplicationMessageCitation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub citation_id: ApplicationMessageCitationId,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
}

impl Query for GetApplicationMessageCitation {
    type Output = ApplicationResult<ApplicationMessageCitation>;
}

#[derive(Debug, Clone)]
pub struct ListApplicationMessageCitationsBySession {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
}

impl Query for ListApplicationMessageCitationsBySession {
    type Output = ApplicationResult<Vec<ApplicationMessageCitation>>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationMessageCitationMutationResult {
    pub citation: ApplicationMessageCitation,
    pub replayed: bool,
}

pub struct CreateApplicationMessageCitationHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
    citations: Arc<dyn IApplicationMessageCitationRepository>,
}

impl CreateApplicationMessageCitationHandler {
    pub fn new(
        sessions: Arc<dyn IApplicationSessionRepository>,
        citations: Arc<dyn IApplicationMessageCitationRepository>,
    ) -> Self {
        Self {
            sessions,
            citations,
        }
    }
}

impl CommandHandler<CreateApplicationMessageCitation>
    for CreateApplicationMessageCitationHandler
{
    fn execute(
        &self,
        command: CreateApplicationMessageCitation,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationMessageCitationMutationResult>>,
    > {
        let sessions = Arc::clone(&self.sessions);
        let citations = Arc::clone(&self.citations);
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
            let citation = match ApplicationMessageCitation::create(
                &session,
                &source_message,
                command.knowledge_base_id,
                command.knowledge_base_revision_id,
                command.knowledge_document_id,
                command.knowledge_chunk_id,
                command.excerpt,
                command.created_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match citations.create_message_citation(citation).await {
                Ok(value) => Ok(Ok(ApplicationMessageCitationMutationResult {
                    citation: value.value,
                    replayed: value.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct GetApplicationMessageCitationHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
    citations: Arc<dyn IApplicationMessageCitationRepository>,
}

impl GetApplicationMessageCitationHandler {
    pub fn new(
        sessions: Arc<dyn IApplicationSessionRepository>,
        citations: Arc<dyn IApplicationMessageCitationRepository>,
    ) -> Self {
        Self {
            sessions,
            citations,
        }
    }
}

impl QueryHandler<GetApplicationMessageCitation> for GetApplicationMessageCitationHandler {
    fn execute(
        &self,
        query: GetApplicationMessageCitation,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationMessageCitation>>,
    > {
        let sessions = Arc::clone(&self.sessions);
        let citations = Arc::clone(&self.citations);
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
            match citations
                .find_message_citation(
                    query.organization_id,
                    query.project_id,
                    query.application_id,
                    query.citation_id,
                )
                .await
            {
                Ok(Some(citation)) if citation.session_id == query.session_id => Ok(Ok(citation)),
                Ok(Some(_)) | Ok(None) | Err(RepositoryError::NotFound) => {
                    Ok(Err(citation_not_found()))
                }
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct ListApplicationMessageCitationsBySessionHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
    citations: Arc<dyn IApplicationMessageCitationRepository>,
}

impl ListApplicationMessageCitationsBySessionHandler {
    pub fn new(
        sessions: Arc<dyn IApplicationSessionRepository>,
        citations: Arc<dyn IApplicationMessageCitationRepository>,
    ) -> Self {
        Self {
            sessions,
            citations,
        }
    }
}

impl QueryHandler<ListApplicationMessageCitationsBySession>
    for ListApplicationMessageCitationsBySessionHandler
{
    fn execute(
        &self,
        query: ListApplicationMessageCitationsBySession,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<ApplicationMessageCitation>>>,
    > {
        let sessions = Arc::clone(&self.sessions);
        let citations = Arc::clone(&self.citations);
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
            match citations
                .list_message_citations_by_session(
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
            "Application message citation request identity is invalid".into(),
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
            "Application message citation source message was not found".into(),
        )),
        Err(error) => Err(error.into()),
    }
}

fn session_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application session not found".into())
}

fn citation_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application message citation not found".into())
}
