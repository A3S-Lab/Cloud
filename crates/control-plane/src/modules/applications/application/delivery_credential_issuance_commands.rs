//! Issue anonymous delivery credentials by minting Secrets material then C20 register.
//!
//! `APP0.2-C30` closes the C16-C20 gap where callers had to invent an exact
//! `SecretVersionReference` without an Applications-owned issuance path.
//! Identity/Secrets mint stays behind `IApplicationDeliveryCredentialMaterialPort`.
//! Public routes, Principal-bound end-user credentials, and secret verification
//! at anonymous admission remain later APP0.3 work.

use super::delivery_credential_commands::{
    ApplicationDeliveryCredentialMutationResult, RegisterApplicationDeliveryCredential,
    RegisterApplicationDeliveryCredentialHandler,
};
use super::delivery_credential_material_port::IApplicationDeliveryCredentialMaterialPort;
use super::resource_access::project;
use crate::modules::applications::ApplicationAccess;
use crate::modules::applications::domain::{
    ApplicationDeliveryCredential, IApplicationDeliveryCredentialRepository,
    IApplicationRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationDeliveryCredentialId, ApplicationId, ApplicationReleaseId, OrganizationId,
    PrincipalId, ProjectId, RepositoryError,
};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use zeroize::Zeroizing;

/// Mint Secrets material and register one Applications-owned anonymous binding.
#[derive(Debug, Clone)]
pub struct IssueApplicationDeliveryCredential {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub credential_id: ApplicationDeliveryCredentialId,
    pub actor_principal_id: PrincipalId,
    pub access: ApplicationAccess,
    pub created_at: DateTime<Utc>,
}

impl Command for IssueApplicationDeliveryCredential {
    type Output = ApplicationResult<ApplicationDeliveryCredentialIssuance>;
}

/// One-time issuance evidence. Plaintext is present only on first successful mint.
#[derive(Clone)]
pub struct ApplicationDeliveryCredentialIssuance {
    pub credential: ApplicationDeliveryCredential,
    pub lookup_key: String,
    pub plaintext_secret: Option<Zeroizing<String>>,
    pub replayed: bool,
}

impl std::fmt::Debug for ApplicationDeliveryCredentialIssuance {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApplicationDeliveryCredentialIssuance")
            .field("credential", &self.credential)
            .field("lookup_key", &self.lookup_key)
            .field(
                "plaintext_secret",
                &self.plaintext_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("replayed", &self.replayed)
            .finish()
    }
}

impl PartialEq for ApplicationDeliveryCredentialIssuance {
    fn eq(&self, other: &Self) -> bool {
        self.credential == other.credential
            && self.lookup_key == other.lookup_key
            && self.replayed == other.replayed
            && self.plaintext_secret.as_deref() == other.plaintext_secret.as_deref()
    }
}

impl Eq for ApplicationDeliveryCredentialIssuance {}

pub struct IssueApplicationDeliveryCredentialHandler {
    applications: Arc<dyn IApplicationRepository>,
    credentials: Arc<dyn IApplicationDeliveryCredentialRepository>,
    material: Arc<dyn IApplicationDeliveryCredentialMaterialPort>,
}

impl IssueApplicationDeliveryCredentialHandler {
    pub fn new(
        applications: Arc<dyn IApplicationRepository>,
        credentials: Arc<dyn IApplicationDeliveryCredentialRepository>,
        material: Arc<dyn IApplicationDeliveryCredentialMaterialPort>,
    ) -> Self {
        Self {
            applications,
            credentials,
            material,
        }
    }
}

impl CommandHandler<IssueApplicationDeliveryCredential>
    for IssueApplicationDeliveryCredentialHandler
{
    fn execute(
        &self,
        command: IssueApplicationDeliveryCredential,
        context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ApplicationDeliveryCredentialIssuance>>,
    > {
        let applications = Arc::clone(&self.applications);
        let credentials = Arc::clone(&self.credentials);
        let material = Arc::clone(&self.material);
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
                    "Application delivery credential issuance identity is invalid".into(),
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
                Ok(Some(existing))
                    if existing.organization_id == command.organization_id
                        && existing.project_id == command.project_id
                        && existing.application_id == command.application_id =>
                {
                    return Ok(Ok(ApplicationDeliveryCredentialIssuance {
                        lookup_key: existing.lookup_key.clone(),
                        credential: existing,
                        plaintext_secret: None,
                        replayed: true,
                    }));
                }
                Ok(Some(_)) => {
                    return Ok(Err(ApplicationError::Conflict(
                        "Application delivery credential identity or lookup key is already in use"
                            .into(),
                    )));
                }
                Ok(None) | Err(RepositoryError::NotFound) => {}
                Err(error) => return Ok(Err(error.into())),
            }

            let minted = match material
                .mint(
                    command.organization_id,
                    command.project_id,
                    command.application_id,
                    command.credential_id,
                    command.actor_principal_id,
                )
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };

            let register = RegisterApplicationDeliveryCredentialHandler::new(
                Arc::clone(&applications),
                Arc::clone(&credentials),
            );
            let registered = match register
                .execute(
                    RegisterApplicationDeliveryCredential {
                        organization_id: command.organization_id,
                        project_id: command.project_id,
                        application_id: command.application_id,
                        application_release_id: command.application_release_id,
                        credential_id: command.credential_id,
                        lookup_key: minted.lookup_key.clone(),
                        secret: minted.secret,
                        actor_principal_id: command.actor_principal_id,
                        access: command.access.clone(),
                        created_at: command.created_at,
                    },
                    context,
                )
                .await?
            {
                Ok(ApplicationDeliveryCredentialMutationResult {
                    credential,
                    replayed,
                }) => (credential, replayed),
                Err(error) => return Ok(Err(error)),
            };

            Ok(Ok(ApplicationDeliveryCredentialIssuance {
                lookup_key: minted.lookup_key,
                credential: registered.0,
                plaintext_secret: if registered.1 {
                    None
                } else {
                    Some(minted.plaintext_secret)
                },
                replayed: registered.1,
            }))
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
            "Application delivery credential issuance identity is invalid".into(),
        ));
    }
    project(project_id, access)
}
