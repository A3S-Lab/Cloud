use crate::infrastructure::{
    ImmutableObjectClient, ImmutableObjectError, S3ImmutableObjectOptions,
};
use crate::modules::data::application::{
    MaterializedObjectNamespaceCredentials, ObjectNamespaceAccess,
    ObjectNamespaceCredentialMaterializer, ObjectNamespaceFlowBinding,
    ObjectNamespaceRecoveryStore,
};
use crate::modules::data::domain::{IObjectNamespace, ObjectNamespaceError};
use crate::modules::shared_kernel::application::ApplicationError;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const RETRY_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_RETRIES: usize = 3;

#[async_trait]
pub(crate) trait IObjectNamespaceAccessResolver: Send + Sync {
    async fn source_and_recovery(
        &self,
        binding: &ObjectNamespaceFlowBinding,
    ) -> Result<(ObjectNamespaceAccess, ObjectNamespaceRecoveryStore), ObjectNamespaceError>;

    async fn access(
        &self,
        binding: &ObjectNamespaceFlowBinding,
    ) -> Result<ObjectNamespaceAccess, ObjectNamespaceError>;
}

/// The sole production S0 provider resolver. It reuses the shared immutable
/// object client and Secrets exact-version materializer for every Flow step;
/// no credential or provider client is cached across steps.
#[derive(Clone)]
pub(crate) struct SharedObjectNamespaceAccessResolver {
    credentials: ObjectNamespaceCredentialMaterializer,
}

impl SharedObjectNamespaceAccessResolver {
    pub(crate) const fn new(credentials: ObjectNamespaceCredentialMaterializer) -> Self {
        Self { credentials }
    }

    async fn materialize(
        &self,
        binding: &ObjectNamespaceFlowBinding,
    ) -> Result<MaterializedObjectNamespaceCredentials, ObjectNamespaceError> {
        binding
            .provider_profile
            .validate()
            .map_err(ObjectNamespaceError::Invalid)?;
        binding
            .credentials
            .validate_provider_profile(&binding.provider_profile)
            .map_err(ObjectNamespaceError::Invalid)?;
        let material = self
            .credentials
            .materialize(&binding.credentials)
            .await
            .map_err(map_application_error)?;
        if material.namespace_id() != binding.credentials.spec().namespace_id
            || material.generation() != binding.credentials.spec().generation
            || material.provider_profile_digest() != binding.provider_profile.digest()
            || material.binding_digest() != binding.credentials.digest()
        {
            return Err(ObjectNamespaceError::Corrupt(
                "materialized object namespace credentials changed their exact binding".into(),
            ));
        }
        Ok(material)
    }
}

#[async_trait]
impl IObjectNamespaceAccessResolver for SharedObjectNamespaceAccessResolver {
    async fn source_and_recovery(
        &self,
        binding: &ObjectNamespaceFlowBinding,
    ) -> Result<(ObjectNamespaceAccess, ObjectNamespaceRecoveryStore), ObjectNamespaceError> {
        let material = self.materialize(binding).await?;
        let namespace_id = binding.credentials.spec().namespace_id;
        let live_prefix = binding
            .provider_profile
            .namespace_prefix(namespace_id)
            .map_err(ObjectNamespaceError::Invalid)?;
        let recovery_prefix = binding
            .provider_profile
            .recovery_prefix(namespace_id)
            .map_err(ObjectNamespaceError::Invalid)?;
        let credentials = ProviderCredentialText::from_material(&material)?;
        let live = namespace_client(binding, &credentials, live_prefix)?;
        let recovery = namespace_client(binding, &credentials, recovery_prefix)?;
        drop(material);
        Ok((
            ObjectNamespaceAccess::new(
                namespace_id,
                binding.provider_profile.digest().clone(),
                live,
            )
            .map_err(ObjectNamespaceError::Invalid)?,
            ObjectNamespaceRecoveryStore::new(binding.provider_profile.digest().clone(), recovery)
                .map_err(ObjectNamespaceError::Invalid)?,
        ))
    }

    async fn access(
        &self,
        binding: &ObjectNamespaceFlowBinding,
    ) -> Result<ObjectNamespaceAccess, ObjectNamespaceError> {
        let material = self.materialize(binding).await?;
        let namespace_id = binding.credentials.spec().namespace_id;
        let prefix = binding
            .provider_profile
            .namespace_prefix(namespace_id)
            .map_err(ObjectNamespaceError::Invalid)?;
        let credentials = ProviderCredentialText::from_material(&material)?;
        let namespace = namespace_client(binding, &credentials, prefix)?;
        drop(material);
        ObjectNamespaceAccess::new(
            namespace_id,
            binding.provider_profile.digest().clone(),
            namespace,
        )
        .map_err(ObjectNamespaceError::Invalid)
    }
}

struct ProviderCredentialText {
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
}

impl ProviderCredentialText {
    fn from_material(
        material: &MaterializedObjectNamespaceCredentials,
    ) -> Result<Self, ObjectNamespaceError> {
        Ok(Self {
            access_key_id: credential_text(material.access_key_id(), "access key ID")?,
            secret_access_key: credential_text(material.secret_access_key(), "secret access key")?,
            session_token: material
                .session_token()
                .map(|value| credential_text(value, "session token"))
                .transpose()?,
        })
    }
}

fn namespace_client(
    binding: &ObjectNamespaceFlowBinding,
    credentials: &ProviderCredentialText,
    prefix: String,
) -> Result<Arc<dyn IObjectNamespace>, ObjectNamespaceError> {
    let profile = binding.provider_profile.spec();
    ImmutableObjectClient::s3(S3ImmutableObjectOptions {
        endpoint: Some(profile.endpoint.clone()),
        region: profile.region.clone(),
        bucket: profile.bucket.clone(),
        prefix,
        access_key_id: credentials.access_key_id.clone(),
        secret_access_key: credentials.secret_access_key.clone(),
        session_token: credentials.session_token.clone(),
        allow_http: false,
        virtual_hosted_style: profile.virtual_hosted_style,
        request_timeout: REQUEST_TIMEOUT,
        connect_timeout: CONNECT_TIMEOUT,
        retry_timeout: RETRY_TIMEOUT,
        max_retries: MAX_RETRIES,
    })
    .map(|client| Arc::new(client) as Arc<dyn IObjectNamespace>)
    .map_err(map_immutable_error)
}

fn credential_text(bytes: &[u8], label: &str) -> Result<String, ObjectNamespaceError> {
    let value = std::str::from_utf8(bytes).map_err(|_| {
        ObjectNamespaceError::Invalid(format!(
            "object namespace {label} Secret must contain UTF-8 provider credential material"
        ))
    })?;
    if value.is_empty() || value.len() > 16 * 1024 || value.contains(['\0', '\r', '\n']) {
        return Err(ObjectNamespaceError::Invalid(format!(
            "object namespace {label} Secret is outside provider credential bounds"
        )));
    }
    Ok(value.to_owned())
}

fn map_application_error(error: ApplicationError) -> ObjectNamespaceError {
    match error {
        ApplicationError::Invalid(message) => ObjectNamespaceError::Invalid(message),
        ApplicationError::NotFound(message)
        | ApplicationError::Conflict(message)
        | ApplicationError::Forbidden(message) => ObjectNamespaceError::Precondition(message),
        ApplicationError::Unavailable(message) | ApplicationError::Internal(message) => {
            ObjectNamespaceError::Unavailable(message)
        }
    }
}

fn map_immutable_error(error: ImmutableObjectError) -> ObjectNamespaceError {
    match error {
        ImmutableObjectError::Invalid(message) => ObjectNamespaceError::Invalid(message),
        ImmutableObjectError::Conflict(message) => ObjectNamespaceError::Precondition(message),
        ImmutableObjectError::Integrity(message) => ObjectNamespaceError::Corrupt(message),
        ImmutableObjectError::Unsupported(message) => ObjectNamespaceError::Unsupported(message),
        ImmutableObjectError::Unavailable(message) => ObjectNamespaceError::Unavailable(message),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn production_resolver_reuses_shared_s0_and_secrets_mechanisms_only() {
        let source = include_str!("object_namespace_access.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        for forbidden in [
            "AmazonS3Builder",
            "object_store::",
            "std::env",
            "tokio::spawn",
            "ISecretRepository",
            "ISecretEncryptionService",
        ] {
            assert!(
                !production.contains(forbidden),
                "S0 provider resolution must reuse shared mechanisms; found {forbidden}"
            );
        }
    }
}
