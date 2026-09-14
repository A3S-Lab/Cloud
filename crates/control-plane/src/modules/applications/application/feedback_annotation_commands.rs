use super::resource_access::project;
use crate::modules::applications::ApplicationAccess;
use crate::modules::applications::domain::{
    ApplicationAnnotation, ApplicationFeedback, ApplicationFeedbackRating, ApplicationMessage,
    ApplicationSession, IApplicationAnnotationRepository, IApplicationFeedbackRepository,
    IApplicationSessionRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationMessageId, ApplicationSessionId, OrganizationId, PrincipalId,
    ProjectId, RepositoryError,
};
use a3s_boot::{Command, CommandHandler, CqrsContext, Query, QueryHandler};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

/// Create one immutable Applications-owned session feedback record.
#[derive(Debug, Clone)]
pub struct CreateApplicationFeedback {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub source_message_id: Option<ApplicationMessageId>,
    pub rating: ApplicationFeedbackRating,
    pub comment: Option<String>,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
    pub created_at: DateTime<Utc>,
}

impl Command for CreateApplicationFeedback {
    type Output = ApplicationResult<ApplicationFeedbackMutationResult>;
}

/// Create one immutable Applications-owned session annotation record.
#[derive(Debug, Clone)]
pub struct CreateApplicationAnnotation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub source_message_id: Option<ApplicationMessageId>,
    pub content: Value,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
    pub created_at: DateTime<Utc>,
}

impl Command for CreateApplicationAnnotation {
    type Output = ApplicationResult<ApplicationAnnotationMutationResult>;
}

#[derive(Debug, Clone)]
pub struct GetApplicationFeedback {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub feedback_id: crate::modules::shared_kernel::domain::ApplicationFeedbackId,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
}

impl Query for GetApplicationFeedback {
    type Output = ApplicationResult<ApplicationFeedback>;
}

#[derive(Debug, Clone)]
pub struct ListApplicationFeedbackBySession {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
}

impl Query for ListApplicationFeedbackBySession {
    type Output = ApplicationResult<Vec<ApplicationFeedback>>;
}

#[derive(Debug, Clone)]
pub struct GetApplicationAnnotation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub annotation_id: crate::modules::shared_kernel::domain::ApplicationAnnotationId,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
}

impl Query for GetApplicationAnnotation {
    type Output = ApplicationResult<ApplicationAnnotation>;
}

#[derive(Debug, Clone)]
pub struct ListApplicationAnnotationsBySession {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
}

impl Query for ListApplicationAnnotationsBySession {
    type Output = ApplicationResult<Vec<ApplicationAnnotation>>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationFeedbackMutationResult {
    pub feedback: ApplicationFeedback,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationAnnotationMutationResult {
    pub annotation: ApplicationAnnotation,
    pub replayed: bool,
}

pub struct CreateApplicationFeedbackHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
    feedbacks: Arc<dyn IApplicationFeedbackRepository>,
}

impl CreateApplicationFeedbackHandler {
    pub fn new(
        sessions: Arc<dyn IApplicationSessionRepository>,
        feedbacks: Arc<dyn IApplicationFeedbackRepository>,
    ) -> Self {
        Self {
            sessions,
            feedbacks,
        }
    }
}

impl CommandHandler<CreateApplicationFeedback> for CreateApplicationFeedbackHandler {
    fn execute(
        &self,
        command: CreateApplicationFeedback,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationFeedbackMutationResult>>,
    > {
        let sessions = Arc::clone(&self.sessions);
        let feedbacks = Arc::clone(&self.feedbacks);
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
                match load_optional_message(sessions.as_ref(), &session, command.source_message_id)
                    .await
                {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(error)),
                };
            let feedback = match ApplicationFeedback::create(
                &session,
                source_message.as_ref(),
                command.rating,
                command.comment,
                command.created_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match feedbacks.create_feedback(feedback).await {
                Ok(value) => Ok(Ok(ApplicationFeedbackMutationResult {
                    feedback: value.value,
                    replayed: value.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct CreateApplicationAnnotationHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
    annotations: Arc<dyn IApplicationAnnotationRepository>,
}

impl CreateApplicationAnnotationHandler {
    pub fn new(
        sessions: Arc<dyn IApplicationSessionRepository>,
        annotations: Arc<dyn IApplicationAnnotationRepository>,
    ) -> Self {
        Self {
            sessions,
            annotations,
        }
    }
}

impl CommandHandler<CreateApplicationAnnotation> for CreateApplicationAnnotationHandler {
    fn execute(
        &self,
        command: CreateApplicationAnnotation,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationAnnotationMutationResult>>,
    > {
        let sessions = Arc::clone(&self.sessions);
        let annotations = Arc::clone(&self.annotations);
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
                match load_optional_message(sessions.as_ref(), &session, command.source_message_id)
                    .await
                {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(error)),
                };
            let annotation = match ApplicationAnnotation::create(
                &session,
                source_message.as_ref(),
                command.content,
                command.created_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match annotations.create_annotation(annotation).await {
                Ok(value) => Ok(Ok(ApplicationAnnotationMutationResult {
                    annotation: value.value,
                    replayed: value.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct GetApplicationFeedbackHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
    feedbacks: Arc<dyn IApplicationFeedbackRepository>,
}

impl GetApplicationFeedbackHandler {
    pub fn new(
        sessions: Arc<dyn IApplicationSessionRepository>,
        feedbacks: Arc<dyn IApplicationFeedbackRepository>,
    ) -> Self {
        Self {
            sessions,
            feedbacks,
        }
    }
}

impl QueryHandler<GetApplicationFeedback> for GetApplicationFeedbackHandler {
    fn execute(
        &self,
        query: GetApplicationFeedback,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<ApplicationFeedback>>>
    {
        let sessions = Arc::clone(&self.sessions);
        let feedbacks = Arc::clone(&self.feedbacks);
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
            match feedbacks
                .find_feedback(
                    query.organization_id,
                    query.project_id,
                    query.application_id,
                    query.feedback_id,
                )
                .await
            {
                Ok(Some(feedback)) if feedback.session_id == query.session_id => Ok(Ok(feedback)),
                Ok(Some(_)) | Ok(None) | Err(RepositoryError::NotFound) => {
                    Ok(Err(feedback_not_found()))
                }
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct ListApplicationFeedbackBySessionHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
    feedbacks: Arc<dyn IApplicationFeedbackRepository>,
}

impl ListApplicationFeedbackBySessionHandler {
    pub fn new(
        sessions: Arc<dyn IApplicationSessionRepository>,
        feedbacks: Arc<dyn IApplicationFeedbackRepository>,
    ) -> Self {
        Self {
            sessions,
            feedbacks,
        }
    }
}

impl QueryHandler<ListApplicationFeedbackBySession> for ListApplicationFeedbackBySessionHandler {
    fn execute(
        &self,
        query: ListApplicationFeedbackBySession,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<ApplicationFeedback>>>>
    {
        let sessions = Arc::clone(&self.sessions);
        let feedbacks = Arc::clone(&self.feedbacks);
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
            match feedbacks
                .list_feedback_by_session(
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

pub struct GetApplicationAnnotationHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
    annotations: Arc<dyn IApplicationAnnotationRepository>,
}

impl GetApplicationAnnotationHandler {
    pub fn new(
        sessions: Arc<dyn IApplicationSessionRepository>,
        annotations: Arc<dyn IApplicationAnnotationRepository>,
    ) -> Self {
        Self {
            sessions,
            annotations,
        }
    }
}

impl QueryHandler<GetApplicationAnnotation> for GetApplicationAnnotationHandler {
    fn execute(
        &self,
        query: GetApplicationAnnotation,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<ApplicationAnnotation>>>
    {
        let sessions = Arc::clone(&self.sessions);
        let annotations = Arc::clone(&self.annotations);
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
            match annotations
                .find_annotation(
                    query.organization_id,
                    query.project_id,
                    query.application_id,
                    query.annotation_id,
                )
                .await
            {
                Ok(Some(annotation)) if annotation.session_id == query.session_id => {
                    Ok(Ok(annotation))
                }
                Ok(Some(_)) | Ok(None) | Err(RepositoryError::NotFound) => {
                    Ok(Err(annotation_not_found()))
                }
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct ListApplicationAnnotationsBySessionHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
    annotations: Arc<dyn IApplicationAnnotationRepository>,
}

impl ListApplicationAnnotationsBySessionHandler {
    pub fn new(
        sessions: Arc<dyn IApplicationSessionRepository>,
        annotations: Arc<dyn IApplicationAnnotationRepository>,
    ) -> Self {
        Self {
            sessions,
            annotations,
        }
    }
}

impl QueryHandler<ListApplicationAnnotationsBySession>
    for ListApplicationAnnotationsBySessionHandler
{
    fn execute(
        &self,
        query: ListApplicationAnnotationsBySession,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<ApplicationAnnotation>>>>
    {
        let sessions = Arc::clone(&self.sessions);
        let annotations = Arc::clone(&self.annotations);
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
            match annotations
                .list_annotations_by_session(
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
            "Application feedback request identity is invalid".into(),
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

async fn load_optional_message(
    sessions: &dyn IApplicationSessionRepository,
    session: &ApplicationSession,
    source_message_id: Option<ApplicationMessageId>,
) -> ApplicationResult<Option<ApplicationMessage>> {
    let Some(message_id) = source_message_id else {
        return Ok(None);
    };
    match sessions
        .find_message(
            session.organization_id,
            session.project_id,
            session.application_id,
            message_id,
        )
        .await
    {
        Ok(Some(message)) => Ok(Some(message)),
        Ok(None) | Err(RepositoryError::NotFound) => Err(ApplicationError::Invalid(
            "Application feedback source message was not found".into(),
        )),
        Err(error) => Err(error.into()),
    }
}

fn session_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application session not found".into())
}

fn feedback_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application feedback not found".into())
}

fn annotation_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application annotation not found".into())
}
