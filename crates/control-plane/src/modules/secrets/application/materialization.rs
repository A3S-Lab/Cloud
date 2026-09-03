use super::{encryption_error, SecretPlaintext};
use crate::modules::secrets::domain::{
    secret_encryption_context, ISecretEncryptionService, ISecretRepository, SecretVersion,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError, SecretId, SecretVersionReference,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Published Secrets boundary for validating one exact, scoped Secret version.
///
/// Consumers receive no Secret aggregate or lifecycle state. The owning
/// implementation performs the scope and active-version decision atomically.
#[async_trait]
pub trait IExactSecretVersionAccess: Send + Sync {
    async fn require_reference(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        reference: SecretVersionReference,
    ) -> ApplicationResult<()>;
}

/// Published Secrets boundary for just-in-time materialization of one exact,
/// scoped Secret version.
///
/// The returned value remains Secrets-owned so its memory is redacted and
/// zeroized by the single canonical plaintext mechanism.
#[async_trait]
pub trait IExactSecretMaterializer: Send + Sync {
    async fn materialize_reference(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        reference: SecretVersionReference,
    ) -> ApplicationResult<SecretPlaintext>;
}

/// Secrets-owned exact-version access used by admission and just-in-time materialization.
///
/// The repository performs scope plus active-state evaluation atomically. Callers receive no
/// Secret lifecycle state and cannot broaden an exact reference to a current-version lookup.
#[derive(Clone)]
pub(crate) struct ExactSecretVersionAccess {
    secrets: Arc<dyn ISecretRepository>,
}

impl ExactSecretVersionAccess {
    pub(crate) fn new(secrets: Arc<dyn ISecretRepository>) -> Self {
        Self { secrets }
    }

    pub(crate) async fn require(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        secret_id: SecretId,
        version: u64,
    ) -> ApplicationResult<()> {
        self.version(
            organization_id,
            project_id,
            environment_id,
            secret_id,
            version,
        )
        .await
        .map(drop)
    }

    pub(crate) async fn require_reference(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        reference: SecretVersionReference,
    ) -> ApplicationResult<()> {
        reference.validate().map_err(ApplicationError::Internal)?;
        self.require(
            organization_id,
            project_id,
            environment_id,
            reference.secret_id,
            reference.version,
        )
        .await
    }

    async fn version(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        secret_id: SecretId,
        version: u64,
    ) -> ApplicationResult<SecretVersion> {
        self.secrets
            .find_materializable_version(
                organization_id,
                project_id,
                environment_id,
                secret_id,
                version,
            )
            .await
            .map_err(materialization_repository_error)
    }
}

#[async_trait]
impl IExactSecretVersionAccess for ExactSecretVersionAccess {
    async fn require_reference(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        reference: SecretVersionReference,
    ) -> ApplicationResult<()> {
        ExactSecretVersionAccess::require_reference(
            self,
            organization_id,
            project_id,
            environment_id,
            reference,
        )
        .await
    }
}

/// Decrypts one exact active Secret version only for the duration of an owning operation.
///
/// This service owns no binding authorization. The caller must first prove that the exact Secret
/// reference belongs to its immutable definition; this service then rechecks canonical scope and
/// active state immediately before decryption.
#[derive(Clone)]
pub(crate) struct ExactSecretMaterializer {
    access: ExactSecretVersionAccess,
    encryption: Arc<dyn ISecretEncryptionService>,
}

impl ExactSecretMaterializer {
    pub(crate) fn new(
        secrets: Arc<dyn ISecretRepository>,
        encryption: Arc<dyn ISecretEncryptionService>,
    ) -> Self {
        Self {
            access: ExactSecretVersionAccess::new(secrets),
            encryption,
        }
    }

    pub(crate) async fn materialize(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        secret_id: SecretId,
        version: u64,
    ) -> ApplicationResult<SecretPlaintext> {
        let version = self
            .access
            .version(
                organization_id,
                project_id,
                environment_id,
                secret_id,
                version,
            )
            .await?;
        let context = secret_encryption_context(organization_id, secret_id, version.version)
            .map_err(ApplicationError::Internal)?;
        self.encryption
            .decrypt(&version.encrypted_value, &context)
            .await
            .map_err(encryption_error)
            .and_then(|value| SecretPlaintext::new(value).map_err(ApplicationError::Internal))
    }

    pub(crate) async fn materialize_reference(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        reference: SecretVersionReference,
    ) -> ApplicationResult<SecretPlaintext> {
        reference.validate().map_err(ApplicationError::Internal)?;
        self.materialize(
            organization_id,
            project_id,
            environment_id,
            reference.secret_id,
            reference.version,
        )
        .await
    }
}

#[async_trait]
impl IExactSecretMaterializer for ExactSecretMaterializer {
    async fn materialize_reference(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        reference: SecretVersionReference,
    ) -> ApplicationResult<SecretPlaintext> {
        ExactSecretMaterializer::materialize_reference(
            self,
            organization_id,
            project_id,
            environment_id,
            reference,
        )
        .await
    }
}

/// Composes the published exact-version access port for an outer bounded
/// context. The returned interface exposes no Secret aggregate or plaintext;
/// all scope and active-state decisions remain inside Secrets.
pub fn exact_secret_version_access(
    secrets: Arc<dyn ISecretRepository>,
) -> Arc<dyn IExactSecretVersionAccess> {
    Arc::new(ExactSecretVersionAccess::new(secrets))
}

pub(crate) fn exact_secret_materializer(
    secrets: Arc<dyn ISecretRepository>,
    encryption: Arc<dyn ISecretEncryptionService>,
) -> Arc<dyn IExactSecretMaterializer> {
    Arc::new(ExactSecretMaterializer::new(secrets, encryption))
}

fn materialization_repository_error(error: RepositoryError) -> ApplicationError {
    match error {
        RepositoryError::NotFound => ApplicationError::Forbidden(
            "Secret material is not authorized for this exact scope and version".into(),
        ),
        other => other.into(),
    }
}
