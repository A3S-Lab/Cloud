use super::delivery_commands::load_release;
use super::resource_access::{project, release_not_found};
use crate::modules::applications::ApplicationAccess;
use crate::modules::applications::domain::{
    ApplicationPublicationChannel, ApplicationPublicationRateShapingPolicyRef,
    ApplicationPublicationRouteIntent, IApplicationPublicationRouteIntentRepository,
    IApplicationRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationPublicationRouteIntentId, ApplicationReleaseId, OrganizationId,
    PrincipalId, ProjectId, RepositoryError, Sha256Digest,
};
use a3s_boot::{Command, CommandHandler, CqrsContext, Query, QueryHandler};
use serde::Serialize;
use std::sync::Arc;

/// Create one immutable Applications-owned exact-release publication route intent.
///
/// Reconstructs the release from Applications storage, builds the domain
/// aggregate, and persists through `create_intent`. Exact release digest must
/// match the stored release. Gateway apply and Delivery rate middleware stay later.
#[derive(Debug, Clone)]
pub struct CreateApplicationPublicationRouteIntent {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_digest: Sha256Digest,
    pub channels: Vec<ApplicationPublicationChannel>,
    pub embed_origin_allowlist: Vec<String>,
    pub rate_shaping_policy: ApplicationPublicationRateShapingPolicyRef,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
}

impl Command for CreateApplicationPublicationRouteIntent {
    type Output = ApplicationResult<ApplicationPublicationRouteIntentMutationResult>;
}

#[derive(Debug, Clone)]
pub struct GetApplicationPublicationRouteIntent {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub intent_id: ApplicationPublicationRouteIntentId,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
}

impl Query for GetApplicationPublicationRouteIntent {
    type Output = ApplicationResult<ApplicationPublicationRouteIntent>;
}

#[derive(Debug, Clone)]
pub struct ListApplicationPublicationRouteIntentsByRelease {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_digest: Sha256Digest,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
}

impl Query for ListApplicationPublicationRouteIntentsByRelease {
    type Output = ApplicationResult<Vec<ApplicationPublicationRouteIntent>>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationPublicationRouteIntentMutationResult {
    pub intent: ApplicationPublicationRouteIntent,
    pub replayed: bool,
}

pub struct CreateApplicationPublicationRouteIntentHandler {
    applications: Arc<dyn IApplicationRepository>,
    intents: Arc<dyn IApplicationPublicationRouteIntentRepository>,
}

impl CreateApplicationPublicationRouteIntentHandler {
    pub fn new(
        applications: Arc<dyn IApplicationRepository>,
        intents: Arc<dyn IApplicationPublicationRouteIntentRepository>,
    ) -> Self {
        Self {
            applications,
            intents,
        }
    }
}

impl CommandHandler<CreateApplicationPublicationRouteIntent>
    for CreateApplicationPublicationRouteIntentHandler
{
    fn execute(
        &self,
        command: CreateApplicationPublicationRouteIntent,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationPublicationRouteIntentMutationResult>>,
    > {
        let applications = Arc::clone(&self.applications);
        let intents = Arc::clone(&self.intents);
        Box::pin(async move {
            if let Err(error) = authorize_actor(
                &command.access,
                command.project_id,
                command.actor_principal_id,
            ) {
                return Ok(Err(error));
            }
            if command.organization_id.as_uuid().is_nil()
                || command.application_id.as_uuid().is_nil()
                || command.application_release_id.as_uuid().is_nil()
            {
                return Ok(Err(ApplicationError::Invalid(
                    "Application publication route intent request identity is invalid".into(),
                )));
            }
            let release = match load_release(
                applications.as_ref(),
                command.organization_id,
                command.project_id,
                command.application_id,
                command.application_release_id,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            if release.contract.digest() != &command.application_release_digest {
                return Ok(Err(release_not_found()));
            }
            let intent = match ApplicationPublicationRouteIntent::create(
                &release,
                command.channels,
                command.embed_origin_allowlist,
                command.rate_shaping_policy,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match intents.create_intent(intent).await {
                Ok(value) => Ok(Ok(ApplicationPublicationRouteIntentMutationResult {
                    intent: value.value,
                    replayed: value.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct GetApplicationPublicationRouteIntentHandler {
    intents: Arc<dyn IApplicationPublicationRouteIntentRepository>,
}

impl GetApplicationPublicationRouteIntentHandler {
    pub fn new(intents: Arc<dyn IApplicationPublicationRouteIntentRepository>) -> Self {
        Self { intents }
    }
}

impl QueryHandler<GetApplicationPublicationRouteIntent>
    for GetApplicationPublicationRouteIntentHandler
{
    fn execute(
        &self,
        query: GetApplicationPublicationRouteIntent,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationPublicationRouteIntent>>,
    > {
        let intents = Arc::clone(&self.intents);
        Box::pin(async move {
            if let Err(error) =
                authorize_actor(&query.access, query.project_id, query.actor_principal_id)
            {
                return Ok(Err(error));
            }
            if query.organization_id.as_uuid().is_nil()
                || query.application_id.as_uuid().is_nil()
                || query.intent_id.as_uuid().is_nil()
            {
                return Ok(Err(ApplicationError::Invalid(
                    "Application publication route intent request identity is invalid".into(),
                )));
            }
            match intents
                .find_intent(
                    query.organization_id,
                    query.project_id,
                    query.application_id,
                    query.intent_id,
                )
                .await
            {
                Ok(Some(intent)) if intent.project_id == query.project_id => Ok(Ok(intent)),
                Ok(Some(_)) | Ok(None) | Err(RepositoryError::NotFound) => {
                    Ok(Err(intent_not_found()))
                }
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct ListApplicationPublicationRouteIntentsByReleaseHandler {
    applications: Arc<dyn IApplicationRepository>,
    intents: Arc<dyn IApplicationPublicationRouteIntentRepository>,
}

impl ListApplicationPublicationRouteIntentsByReleaseHandler {
    pub fn new(
        applications: Arc<dyn IApplicationRepository>,
        intents: Arc<dyn IApplicationPublicationRouteIntentRepository>,
    ) -> Self {
        Self {
            applications,
            intents,
        }
    }
}

impl QueryHandler<ListApplicationPublicationRouteIntentsByRelease>
    for ListApplicationPublicationRouteIntentsByReleaseHandler
{
    fn execute(
        &self,
        query: ListApplicationPublicationRouteIntentsByRelease,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<ApplicationPublicationRouteIntent>>>,
    > {
        let applications = Arc::clone(&self.applications);
        let intents = Arc::clone(&self.intents);
        Box::pin(async move {
            if let Err(error) =
                authorize_actor(&query.access, query.project_id, query.actor_principal_id)
            {
                return Ok(Err(error));
            }
            if query.organization_id.as_uuid().is_nil()
                || query.application_id.as_uuid().is_nil()
                || query.application_release_id.as_uuid().is_nil()
            {
                return Ok(Err(ApplicationError::Invalid(
                    "Application publication route intent request identity is invalid".into(),
                )));
            }
            let release = match load_release(
                applications.as_ref(),
                query.organization_id,
                query.project_id,
                query.application_id,
                query.application_release_id,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            if release.contract.digest() != &query.application_release_digest {
                return Ok(Err(release_not_found()));
            }
            match intents
                .list_intents_by_release(
                    query.organization_id,
                    query.project_id,
                    query.application_id,
                    query.application_release_id,
                    &query.application_release_digest,
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
            "Application publication route intent request identity is invalid".into(),
        ));
    }
    project(project_id, access)
}

fn intent_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application publication route intent not found".into())
}
