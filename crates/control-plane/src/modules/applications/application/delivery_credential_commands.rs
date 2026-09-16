use super::delivery_commands::load_release;
use super::resource_access::project;
use crate::modules::applications::domain::{
    ApplicationAudience, ApplicationDeliveryCredential, ApplicationDeliveryCredentialStatus,
    IApplicationDeliveryCredentialRepository, IApplicationRepository,
};
use crate::modules::applications::ApplicationAccess;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationDeliveryCredentialId, ApplicationId, ApplicationReleaseId, OrganizationId,
    PrincipalId, ProjectId, RepositoryError, SecretVersionReference,
};
use a3s_boot::{Command, CommandHandler, CqrsContext, Query, QueryHandler};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;

/// Register one Applications-owned anonymous delivery credential binding.
///
/// Callers supply the opaque lookup key and an exact Secrets version reference.
/// Plaintext never enters Applications. Identity material issuance stays
/// `APP0.3`.
#[derive(Debug, Clone)]
pub struct RegisterApplicationDeliveryCredential {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub credential_id: ApplicationDeliveryCredentialId,
    pub lookup_key: String,
    pub secret: SecretVersionReference,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
    pub created_at: DateTime<Utc>,
}

impl Command for RegisterApplicationDeliveryCredential {
    type Output = ApplicationResult<ApplicationDeliveryCredentialMutationResult>;
}

/// Generation-fenced disable of one delivery credential binding.
#[derive(Debug, Clone)]
pub struct DisableApplicationDeliveryCredential {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub credential_id: ApplicationDeliveryCredentialId,
    pub expected_generation: u64,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
    pub updated_at: DateTime<Utc>,
}

impl Command for DisableApplicationDeliveryCredential {
    type Output = ApplicationResult<ApplicationDeliveryCredentialMutationResult>;
}

/// Generation-fenced enable of one delivery credential binding.
#[derive(Debug, Clone)]
pub struct EnableApplicationDeliveryCredential {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub credential_id: ApplicationDeliveryCredentialId,
    pub expected_generation: u64,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
    pub updated_at: DateTime<Utc>,
}

impl Command for EnableApplicationDeliveryCredential {
    type Output = ApplicationResult<ApplicationDeliveryCredentialMutationResult>;
}

/// Generation-fenced revoke of one delivery credential binding.
#[derive(Debug, Clone)]
pub struct RevokeApplicationDeliveryCredential {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub credential_id: ApplicationDeliveryCredentialId,
    pub expected_generation: u64,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
    pub revoked_at: DateTime<Utc>,
}

impl Command for RevokeApplicationDeliveryCredential {
    type Output = ApplicationResult<ApplicationDeliveryCredentialMutationResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDeliveryCredentialMutationResult {
    pub credential: ApplicationDeliveryCredential,
    pub replayed: bool,
}

pub struct RegisterApplicationDeliveryCredentialHandler {
    applications: Arc<dyn IApplicationRepository>,
    credentials: Arc<dyn IApplicationDeliveryCredentialRepository>,
}

impl RegisterApplicationDeliveryCredentialHandler {
    pub fn new(
        applications: Arc<dyn IApplicationRepository>,
        credentials: Arc<dyn IApplicationDeliveryCredentialRepository>,
    ) -> Self {
        Self {
            applications,
            credentials,
        }
    }
}

impl CommandHandler<RegisterApplicationDeliveryCredential>
    for RegisterApplicationDeliveryCredentialHandler
{
    fn execute(
        &self,
        command: RegisterApplicationDeliveryCredential,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationDeliveryCredentialMutationResult>>,
    > {
        let applications = Arc::clone(&self.applications);
        let credentials = Arc::clone(&self.credentials);
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
                || command.credential_id.as_uuid().is_nil()
            {
                return Ok(Err(ApplicationError::Invalid(
                    "Application delivery credential request identity is invalid".into(),
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
            if release.contract.spec().audience != ApplicationAudience::Anonymous {
                return Ok(Err(ApplicationError::Conflict(
                    "Application delivery credentials require an anonymous-audience release".into(),
                )));
            }
            match credentials
                .find_delivery_credential(
                    command.organization_id,
                    command.application_id,
                    command.credential_id,
                )
                .await
            {
                Ok(Some(existing)) => {
                    return Ok(replay_register(&existing, &command));
                }
                Ok(None) | Err(RepositoryError::NotFound) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let credential = match ApplicationDeliveryCredential::issue(
                command.credential_id,
                &release,
                command.lookup_key,
                command.secret,
                command.actor_principal_id,
                command.created_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match credentials
                .create_delivery_credential(credential.clone())
                .await
            {
                Ok(value) => Ok(Ok(ApplicationDeliveryCredentialMutationResult {
                    credential: value,
                    replayed: false,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct DisableApplicationDeliveryCredentialHandler {
    credentials: Arc<dyn IApplicationDeliveryCredentialRepository>,
}

impl DisableApplicationDeliveryCredentialHandler {
    pub fn new(credentials: Arc<dyn IApplicationDeliveryCredentialRepository>) -> Self {
        Self { credentials }
    }
}

impl CommandHandler<DisableApplicationDeliveryCredential>
    for DisableApplicationDeliveryCredentialHandler
{
    fn execute(
        &self,
        command: DisableApplicationDeliveryCredential,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationDeliveryCredentialMutationResult>>,
    > {
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            transition_credential(
                credentials.as_ref(),
                LifecycleCommand::Disable {
                    organization_id: command.organization_id,
                    project_id: command.project_id,
                    application_id: command.application_id,
                    credential_id: command.credential_id,
                    expected_generation: command.expected_generation,
                    actor_principal_id: command.actor_principal_id,
                    access: command.access,
                    at: command.updated_at,
                },
            )
            .await
        })
    }
}

pub struct EnableApplicationDeliveryCredentialHandler {
    credentials: Arc<dyn IApplicationDeliveryCredentialRepository>,
}

impl EnableApplicationDeliveryCredentialHandler {
    pub fn new(credentials: Arc<dyn IApplicationDeliveryCredentialRepository>) -> Self {
        Self { credentials }
    }
}

impl CommandHandler<EnableApplicationDeliveryCredential>
    for EnableApplicationDeliveryCredentialHandler
{
    fn execute(
        &self,
        command: EnableApplicationDeliveryCredential,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationDeliveryCredentialMutationResult>>,
    > {
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            transition_credential(
                credentials.as_ref(),
                LifecycleCommand::Enable {
                    organization_id: command.organization_id,
                    project_id: command.project_id,
                    application_id: command.application_id,
                    credential_id: command.credential_id,
                    expected_generation: command.expected_generation,
                    actor_principal_id: command.actor_principal_id,
                    access: command.access,
                    at: command.updated_at,
                },
            )
            .await
        })
    }
}

pub struct RevokeApplicationDeliveryCredentialHandler {
    credentials: Arc<dyn IApplicationDeliveryCredentialRepository>,
}

impl RevokeApplicationDeliveryCredentialHandler {
    pub fn new(credentials: Arc<dyn IApplicationDeliveryCredentialRepository>) -> Self {
        Self { credentials }
    }
}

impl CommandHandler<RevokeApplicationDeliveryCredential>
    for RevokeApplicationDeliveryCredentialHandler
{
    fn execute(
        &self,
        command: RevokeApplicationDeliveryCredential,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationDeliveryCredentialMutationResult>>,
    > {
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            transition_credential(
                credentials.as_ref(),
                LifecycleCommand::Revoke {
                    organization_id: command.organization_id,
                    project_id: command.project_id,
                    application_id: command.application_id,
                    credential_id: command.credential_id,
                    expected_generation: command.expected_generation,
                    actor_principal_id: command.actor_principal_id,
                    access: command.access,
                    at: command.revoked_at,
                },
            )
            .await
        })
    }
}

enum LifecycleCommand {
    Disable {
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        credential_id: ApplicationDeliveryCredentialId,
        expected_generation: u64,
        actor_principal_id: PrincipalId,
        access: ApplicationAccess,
        at: DateTime<Utc>,
    },
    Enable {
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        credential_id: ApplicationDeliveryCredentialId,
        expected_generation: u64,
        actor_principal_id: PrincipalId,
        access: ApplicationAccess,
        at: DateTime<Utc>,
    },
    Revoke {
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        credential_id: ApplicationDeliveryCredentialId,
        expected_generation: u64,
        actor_principal_id: PrincipalId,
        access: ApplicationAccess,
        at: DateTime<Utc>,
    },
}

async fn transition_credential(
    credentials: &dyn IApplicationDeliveryCredentialRepository,
    command: LifecycleCommand,
) -> a3s_boot::Result<ApplicationResult<ApplicationDeliveryCredentialMutationResult>> {
    let (
        organization_id,
        project_id,
        application_id,
        credential_id,
        expected_generation,
        actor_principal_id,
        access,
        at,
    ) = match &command {
        LifecycleCommand::Disable {
            organization_id,
            project_id,
            application_id,
            credential_id,
            expected_generation,
            actor_principal_id,
            access,
            at,
        }
        | LifecycleCommand::Enable {
            organization_id,
            project_id,
            application_id,
            credential_id,
            expected_generation,
            actor_principal_id,
            access,
            at,
        }
        | LifecycleCommand::Revoke {
            organization_id,
            project_id,
            application_id,
            credential_id,
            expected_generation,
            actor_principal_id,
            access,
            at,
        } => (
            *organization_id,
            *project_id,
            *application_id,
            *credential_id,
            *expected_generation,
            *actor_principal_id,
            access.clone(),
            *at,
        ),
    };
    if let Err(error) = authorize_actor(&access, project_id, actor_principal_id) {
        return Ok(Err(error));
    }
    if organization_id.as_uuid().is_nil()
        || application_id.as_uuid().is_nil()
        || credential_id.as_uuid().is_nil()
    {
        return Ok(Err(ApplicationError::Invalid(
            "Application delivery credential request identity is invalid".into(),
        )));
    }
    let mut credential = match load_credential(
        credentials,
        organization_id,
        project_id,
        application_id,
        credential_id,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => return Ok(Err(error)),
    };
    let prior_generation = credential.generation;
    let outcome = match command {
        LifecycleCommand::Disable { .. } => credential.disable(expected_generation, at),
        LifecycleCommand::Enable { .. } => credential.enable(expected_generation, at),
        LifecycleCommand::Revoke { .. } => credential.revoke(expected_generation, at),
    };
    if let Err(error) = outcome {
        return Ok(Err(ApplicationError::Conflict(error)));
    }
    match credentials
        .update_delivery_credential(credential.clone(), prior_generation)
        .await
    {
        Ok(value) => Ok(Ok(ApplicationDeliveryCredentialMutationResult {
            credential: value,
            replayed: false,
        })),
        Err(error) => Ok(Err(error.into())),
    }
}

/// Get one Applications-owned anonymous delivery credential binding.
#[derive(Debug, Clone)]
pub struct GetApplicationDeliveryCredential {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub credential_id: ApplicationDeliveryCredentialId,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
}

impl Query for GetApplicationDeliveryCredential {
    type Output = ApplicationResult<ApplicationDeliveryCredential>;
}

/// List Applications-owned anonymous delivery credential bindings for one application.
#[derive(Debug, Clone)]
pub struct ListApplicationDeliveryCredentials {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
}

impl Query for ListApplicationDeliveryCredentials {
    type Output = ApplicationResult<Vec<ApplicationDeliveryCredential>>;
}

pub struct GetApplicationDeliveryCredentialHandler {
    credentials: Arc<dyn IApplicationDeliveryCredentialRepository>,
}

impl GetApplicationDeliveryCredentialHandler {
    pub fn new(credentials: Arc<dyn IApplicationDeliveryCredentialRepository>) -> Self {
        Self { credentials }
    }
}

impl QueryHandler<GetApplicationDeliveryCredential> for GetApplicationDeliveryCredentialHandler {
    fn execute(
        &self,
        query: GetApplicationDeliveryCredential,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationDeliveryCredential>>,
    > {
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            if let Err(error) =
                authorize_actor(&query.access, query.project_id, query.actor_principal_id)
            {
                return Ok(Err(error));
            }
            Ok(load_credential(
                credentials.as_ref(),
                query.organization_id,
                query.project_id,
                query.application_id,
                query.credential_id,
            )
            .await)
        })
    }
}

pub struct ListApplicationDeliveryCredentialsHandler {
    applications: Arc<dyn IApplicationRepository>,
    credentials: Arc<dyn IApplicationDeliveryCredentialRepository>,
}

impl ListApplicationDeliveryCredentialsHandler {
    pub fn new(
        applications: Arc<dyn IApplicationRepository>,
        credentials: Arc<dyn IApplicationDeliveryCredentialRepository>,
    ) -> Self {
        Self {
            applications,
            credentials,
        }
    }
}

impl QueryHandler<ListApplicationDeliveryCredentials>
    for ListApplicationDeliveryCredentialsHandler
{
    fn execute(
        &self,
        query: ListApplicationDeliveryCredentials,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<ApplicationDeliveryCredential>>>,
    > {
        let applications = Arc::clone(&self.applications);
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            if let Err(error) =
                authorize_actor(&query.access, query.project_id, query.actor_principal_id)
            {
                return Ok(Err(error));
            }
            if query.organization_id.as_uuid().is_nil() || query.application_id.as_uuid().is_nil() {
                return Ok(Err(ApplicationError::Invalid(
                    "Application delivery credential request identity is invalid".into(),
                )));
            }
            match applications
                .find(
                    query.organization_id,
                    query.project_id,
                    query.application_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "Application not found".into(),
                    )));
                }
                Err(error) => return Ok(Err(error.into())),
            }
            match credentials
                .list_delivery_credentials_by_application(
                    query.organization_id,
                    query.project_id,
                    query.application_id,
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
            "Application delivery credential request identity is invalid".into(),
        ));
    }
    project(project_id, access)
}

fn replay_register(
    existing: &ApplicationDeliveryCredential,
    command: &RegisterApplicationDeliveryCredential,
) -> ApplicationResult<ApplicationDeliveryCredentialMutationResult> {
    if existing.organization_id != command.organization_id
        || existing.project_id != command.project_id
        || existing.application_id != command.application_id
        || existing.id != command.credential_id
        || existing.lookup_key != command.lookup_key
        || existing.secret != command.secret
        || existing.created_by != command.actor_principal_id
        || existing.status != ApplicationDeliveryCredentialStatus::Active
        || existing.generation != 1
    {
        return Err(ApplicationError::Conflict(
            "Application delivery credential identity or lookup key is already in use".into(),
        ));
    }
    Ok(ApplicationDeliveryCredentialMutationResult {
        credential: existing.clone(),
        replayed: true,
    })
}

async fn load_credential(
    credentials: &dyn IApplicationDeliveryCredentialRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
    credential_id: ApplicationDeliveryCredentialId,
) -> ApplicationResult<ApplicationDeliveryCredential> {
    match credentials
        .find_delivery_credential(organization_id, application_id, credential_id)
        .await
    {
        Ok(Some(credential))
            if credential.organization_id == organization_id
                && credential.project_id == project_id
                && credential.application_id == application_id =>
        {
            Ok(credential)
        }
        Ok(Some(_)) | Ok(None) | Err(RepositoryError::NotFound) => Err(credential_not_found()),
        Err(error) => Err(error.into()),
    }
}

fn credential_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application delivery credential not found".into())
}
