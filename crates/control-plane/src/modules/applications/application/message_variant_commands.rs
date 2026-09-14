use super::resource_access::project;
use crate::modules::applications::ApplicationAccess;
use crate::modules::applications::domain::{
    ApplicationMessage, ApplicationMessageVariant, ApplicationSession,
    IApplicationMessageVariantRepository, IApplicationSessionRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationMessageId, ApplicationMessageVariantId, ApplicationSessionId,
    OrganizationId, PrincipalId, ProjectId, RepositoryError,
};
use a3s_boot::{Command, CommandHandler, CqrsContext, Query, QueryHandler};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

/// Create one immutable Applications-owned session message variant.
#[derive(Debug, Clone)]
pub struct CreateApplicationMessageVariant {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub source_message_id: ApplicationMessageId,
    pub instruction: Option<Value>,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
    pub created_at: DateTime<Utc>,
}

impl Command for CreateApplicationMessageVariant {
    type Output = ApplicationResult<ApplicationMessageVariantMutationResult>;
}

#[derive(Debug, Clone)]
pub struct GetApplicationMessageVariant {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub variant_id: ApplicationMessageVariantId,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
}

impl Query for GetApplicationMessageVariant {
    type Output = ApplicationResult<ApplicationMessageVariant>;
}

#[derive(Debug, Clone)]
pub struct ListApplicationMessageVariantsBySession {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub session_id: ApplicationSessionId,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
}

impl Query for ListApplicationMessageVariantsBySession {
    type Output = ApplicationResult<Vec<ApplicationMessageVariant>>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationMessageVariantMutationResult {
    pub variant: ApplicationMessageVariant,
    pub replayed: bool,
}

pub struct CreateApplicationMessageVariantHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
    variants: Arc<dyn IApplicationMessageVariantRepository>,
}

impl CreateApplicationMessageVariantHandler {
    pub fn new(
        sessions: Arc<dyn IApplicationSessionRepository>,
        variants: Arc<dyn IApplicationMessageVariantRepository>,
    ) -> Self {
        Self { sessions, variants }
    }
}

impl CommandHandler<CreateApplicationMessageVariant> for CreateApplicationMessageVariantHandler {
    fn execute(
        &self,
        command: CreateApplicationMessageVariant,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationMessageVariantMutationResult>>,
    > {
        let sessions = Arc::clone(&self.sessions);
        let variants = Arc::clone(&self.variants);
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
                match load_source_message(sessions.as_ref(), &session, command.source_message_id)
                    .await
                {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(error)),
                };
            let variant = match ApplicationMessageVariant::create(
                &session,
                &source_message,
                command.instruction,
                command.created_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match variants.create_message_variant(variant).await {
                Ok(value) => Ok(Ok(ApplicationMessageVariantMutationResult {
                    variant: value.value,
                    replayed: value.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct GetApplicationMessageVariantHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
    variants: Arc<dyn IApplicationMessageVariantRepository>,
}

impl GetApplicationMessageVariantHandler {
    pub fn new(
        sessions: Arc<dyn IApplicationSessionRepository>,
        variants: Arc<dyn IApplicationMessageVariantRepository>,
    ) -> Self {
        Self { sessions, variants }
    }
}

impl QueryHandler<GetApplicationMessageVariant> for GetApplicationMessageVariantHandler {
    fn execute(
        &self,
        query: GetApplicationMessageVariant,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<ApplicationMessageVariant>>>
    {
        let sessions = Arc::clone(&self.sessions);
        let variants = Arc::clone(&self.variants);
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
            match variants
                .find_message_variant(
                    query.organization_id,
                    query.project_id,
                    query.application_id,
                    query.variant_id,
                )
                .await
            {
                Ok(Some(variant)) if variant.session_id == query.session_id => Ok(Ok(variant)),
                Ok(Some(_)) | Ok(None) | Err(RepositoryError::NotFound) => {
                    Ok(Err(variant_not_found()))
                }
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct ListApplicationMessageVariantsBySessionHandler {
    sessions: Arc<dyn IApplicationSessionRepository>,
    variants: Arc<dyn IApplicationMessageVariantRepository>,
}

impl ListApplicationMessageVariantsBySessionHandler {
    pub fn new(
        sessions: Arc<dyn IApplicationSessionRepository>,
        variants: Arc<dyn IApplicationMessageVariantRepository>,
    ) -> Self {
        Self { sessions, variants }
    }
}

impl QueryHandler<ListApplicationMessageVariantsBySession>
    for ListApplicationMessageVariantsBySessionHandler
{
    fn execute(
        &self,
        query: ListApplicationMessageVariantsBySession,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<ApplicationMessageVariant>>>,
    > {
        let sessions = Arc::clone(&self.sessions);
        let variants = Arc::clone(&self.variants);
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
            match variants
                .list_message_variants_by_session(
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
            "Application message variant request identity is invalid".into(),
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
    source_message_id: ApplicationMessageId,
) -> ApplicationResult<ApplicationMessage> {
    match sessions
        .find_message(
            session.organization_id,
            session.project_id,
            session.application_id,
            source_message_id,
        )
        .await
    {
        Ok(Some(message)) => Ok(message),
        Ok(None) | Err(RepositoryError::NotFound) => Err(ApplicationError::Invalid(
            "Application message variant source message was not found".into(),
        )),
        Err(error) => Err(error.into()),
    }
}

fn session_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application session not found".into())
}

fn variant_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application message variant not found".into())
}
