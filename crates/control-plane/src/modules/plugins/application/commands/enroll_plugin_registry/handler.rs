use super::{EnrollPluginRegistry, EnrollPluginRegistryResult};
use crate::modules::plugins::domain::entities::{NewPluginRegistry, PluginRegistry};
use crate::modules::plugins::domain::events::PluginRegistryEnrolled;
use crate::modules::plugins::domain::repositories::{
    CreatePluginRegistryWrite, IPluginRegistryRepository,
};
use crate::modules::plugins::domain::services::{
    IPluginRegistryEnrollmentAuthorizer, IPluginTrustRootStore,
    PluginRegistryEnrollmentAuthorizationError, PluginTrustRootStoreError,
};
use crate::modules::plugins::domain::value_objects::{PluginRegistryEndpoint, PluginTrustRoot};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{PluginRegistryId, ResourceName, Sha256Digest};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use a3s_use_extension::inspect_bootstrap_root;
use std::sync::Arc;

pub struct EnrollPluginRegistryHandler {
    authorizer: Arc<dyn IPluginRegistryEnrollmentAuthorizer>,
    trust_roots: Arc<dyn IPluginTrustRootStore>,
    registries: Arc<dyn IPluginRegistryRepository>,
}

impl EnrollPluginRegistryHandler {
    pub fn new(
        authorizer: Arc<dyn IPluginRegistryEnrollmentAuthorizer>,
        trust_roots: Arc<dyn IPluginTrustRootStore>,
        registries: Arc<dyn IPluginRegistryRepository>,
    ) -> Self {
        Self {
            authorizer,
            trust_roots,
            registries,
        }
    }
}

impl CommandHandler<EnrollPluginRegistry> for EnrollPluginRegistryHandler {
    fn execute(
        &self,
        command: EnrollPluginRegistry,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<EnrollPluginRegistryResult>>>
    {
        let authorizer = Arc::clone(&self.authorizer);
        let trust_roots = Arc::clone(&self.trust_roots);
        let registries = Arc::clone(&self.registries);
        Box::pin(async move {
            let name = match ResourceName::parse(command.name) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let endpoint = match PluginRegistryEndpoint::parse(command.endpoint) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let authorization = match authorizer
                .authorize_enrollment(command.organization_id, command.actor_id)
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(map_authorization_error(error))),
            };
            let evidence = match inspect_bootstrap_root(&command.bootstrap_root) {
                Ok(value) => value,
                Err(error) => {
                    return Ok(Err(ApplicationError::Invalid(format!(
                        "plugin bootstrap root was rejected ({})",
                        error.code
                    ))))
                }
            };
            let digest = Sha256Digest::parse(format!("sha256:{}", evidence.root_sha256))
                .map_err(BootError::Internal)?;
            let trust_root = match PluginTrustRoot::from_digest(digest, evidence.root_version) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let registry = match PluginRegistry::enroll(NewPluginRegistry {
                organization_id: command.organization_id,
                id: PluginRegistryId::new(),
                name,
                endpoint,
                trust_root,
                actor_id: command.actor_id,
                request_id: command.request_id,
                enrolled_at: command.requested_at,
            }) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let idempotency = match CreatePluginRegistryWrite::idempotency_for(
                &registry,
                command.idempotency_key,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = PluginRegistryEnrolled::envelope(&registry)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            if let Err(error) = trust_roots
                .put(&registry.trust_root, command.bootstrap_root)
                .await
            {
                return Ok(Err(map_trust_root_error(error)));
            }
            let write = match registries
                .create(CreatePluginRegistryWrite {
                    registry,
                    event,
                    authorization,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(EnrollPluginRegistryResult {
                registry: write.value,
                replayed: write.replayed,
            }))
        })
    }
}

fn map_authorization_error(error: PluginRegistryEnrollmentAuthorizationError) -> ApplicationError {
    match error {
        PluginRegistryEnrollmentAuthorizationError::Forbidden => ApplicationError::Forbidden(
            "plugin registry enrollment requires an active human organization member".into(),
        ),
        PluginRegistryEnrollmentAuthorizationError::Unavailable(_) => {
            ApplicationError::Unavailable(
                "plugin registry enrollment authorization is unavailable".into(),
            )
        }
    }
}

fn map_trust_root_error(error: PluginTrustRootStoreError) -> ApplicationError {
    match error {
        PluginTrustRootStoreError::Invalid(message) => ApplicationError::Invalid(message),
        PluginTrustRootStoreError::Conflict => ApplicationError::Conflict(
            "plugin trust-root object conflicts with stored content".into(),
        ),
        PluginTrustRootStoreError::NotFound
        | PluginTrustRootStoreError::Integrity(_)
        | PluginTrustRootStoreError::Storage(_) => {
            ApplicationError::Unavailable("plugin trust-root storage is unavailable".into())
        }
    }
}
