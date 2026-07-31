use crate::modules::edge::domain::events::McpCredentialChanged;
use crate::modules::edge::domain::repositories::{
    IMcpCredentialAuthorityRepository, McpCredentialLifecycleResult, StoreMcpCredentialLifecycle,
    MCP_CREDENTIAL_IDENTITY_CONFLICT,
};
use crate::modules::edge::domain::{
    mcp_credential_audit_record, McpCredential, McpCredentialDelivery,
    MAX_MCP_CREDENTIAL_DELIVERY_TTL,
};
use crate::modules::edge::infrastructure::{
    McpCredentialIssuanceError, McpCredentialIssueRequest, McpCredentialMaterial,
    McpCredentialMaterialGenerator,
};
use crate::modules::secrets::domain::{
    EncryptedSecretValue, ISecretEncryptionService, SecretEncryptionError,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, IdempotencyRequest, OrganizationId, ProjectId,
};
use chrono::{DateTime, Duration, Utc};
use std::sync::Arc;
use uuid::Uuid;
use zeroize::Zeroizing;

const MAX_ISSUANCE_ATTEMPTS: usize = 4;

pub struct McpCredentialSecret(Zeroizing<String>);

impl McpCredentialSecret {
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn into_zeroizing(self) -> Zeroizing<String> {
        self.0
    }
}

impl std::fmt::Debug for McpCredentialSecret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("<redacted-mcp-credential-secret>")
    }
}

pub struct McpCredentialMutationResult {
    pub credential: McpCredential,
    pub replayed: bool,
    secret: Option<McpCredentialSecret>,
}

impl McpCredentialMutationResult {
    pub fn secret(&self) -> Option<&McpCredentialSecret> {
        self.secret.as_ref()
    }

    pub fn into_parts(self) -> (McpCredential, Option<McpCredentialSecret>, bool) {
        (self.credential, self.secret, self.replayed)
    }

    pub(crate) fn without_secret(credential: McpCredential, replayed: bool) -> Self {
        Self {
            credential,
            replayed,
            secret: None,
        }
    }
}

impl std::fmt::Debug for McpCredentialMutationResult {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpCredentialMutationResult")
            .field("credential", &self.credential)
            .field(
                "secret",
                &self.secret.as_ref().map(|_| "<redacted-mcp-secret>"),
            )
            .field("replayed", &self.replayed)
            .finish()
    }
}

#[derive(Clone)]
pub struct McpCredentialLifecycleService {
    repository: Arc<dyn IMcpCredentialAuthorityRepository>,
    encryption: Arc<dyn ISecretEncryptionService>,
    material: McpCredentialMaterialGenerator,
    delivery_ttl: Duration,
}

impl McpCredentialLifecycleService {
    pub fn new(
        repository: Arc<dyn IMcpCredentialAuthorityRepository>,
        encryption: Arc<dyn ISecretEncryptionService>,
        delivery_ttl: Duration,
    ) -> Result<Self, String> {
        if delivery_ttl <= Duration::zero() || delivery_ttl > MAX_MCP_CREDENTIAL_DELIVERY_TTL {
            return Err("MCP credential recovery TTL must be positive and at most one hour".into());
        }
        Ok(Self {
            repository,
            encryption,
            material: McpCredentialMaterialGenerator::new(),
            delivery_ttl,
        })
    }

    pub async fn replay(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        idempotency: &IdempotencyRequest,
        observed_at: DateTime<Utc>,
    ) -> ApplicationResult<Option<McpCredentialMutationResult>> {
        let replay = self
            .repository
            .replay_mcp_credential_lifecycle(
                organization_id,
                project_id,
                environment_id,
                idempotency,
                canonical_timestamp(observed_at),
            )
            .await
            .map_err(ApplicationError::from)?;
        match replay {
            Some(replay) => self.present(replay).await.map(Some),
            None => Ok(None),
        }
    }

    pub async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        credential_id: crate::modules::shared_kernel::domain::McpCredentialId,
    ) -> ApplicationResult<McpCredential> {
        let credential = self
            .repository
            .find_mcp_credential(organization_id, credential_id)
            .await
            .map_err(ApplicationError::from)?;
        exact_credential(credential, project_id, environment_id)
    }

    pub async fn issue(
        &self,
        request: McpCredentialIssueRequest,
        idempotency: IdempotencyRequest,
        actor_id: Uuid,
        correlation_id: Uuid,
    ) -> ApplicationResult<McpCredentialMutationResult> {
        for attempt in 0..MAX_ISSUANCE_ATTEMPTS {
            let material = self
                .material
                .issue(request.clone())
                .await
                .map_err(material_error)?;
            match self
                .store_material(
                    material,
                    None,
                    idempotency.clone(),
                    actor_id,
                    correlation_id,
                )
                .await
            {
                Ok(result) => return Ok(result),
                Err(ApplicationError::Conflict(message))
                    if message == MCP_CREDENTIAL_IDENTITY_CONFLICT =>
                {
                    if attempt + 1 == MAX_ISSUANCE_ATTEMPTS {
                        return Err(ApplicationError::Unavailable(
                            "MCP credential issuance exhausted its bounded identity retries".into(),
                        ));
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Err(ApplicationError::Unavailable(
            "MCP credential issuance exhausted its bounded identity retries".into(),
        ))
    }

    pub async fn rotate(
        &self,
        credential: &McpCredential,
        expires_at: DateTime<Utc>,
        rotated_at: DateTime<Utc>,
        idempotency: IdempotencyRequest,
        actor_id: Uuid,
        correlation_id: Uuid,
    ) -> ApplicationResult<McpCredentialMutationResult> {
        let expected_version = credential.aggregate_version();
        let material = self
            .material
            .rotate(credential, expires_at, rotated_at)
            .await
            .map_err(material_error)?;
        self.store_material(
            material,
            Some(expected_version),
            idempotency,
            actor_id,
            correlation_id,
        )
        .await
    }

    pub async fn revoke(
        &self,
        credential: McpCredential,
        expected_version: u64,
        idempotency: IdempotencyRequest,
        actor_id: Uuid,
        correlation_id: Uuid,
        observed_at: DateTime<Utc>,
    ) -> ApplicationResult<McpCredentialMutationResult> {
        let event = McpCredentialChanged::envelope(&credential, correlation_id)
            .map_err(|_| internal_delivery_error())?;
        let audit = mcp_credential_audit_record(
            &credential,
            Some(expected_version),
            actor_id,
            correlation_id,
        )
        .map_err(|_| internal_audit_error())?;
        let stored = self
            .repository
            .store_mcp_credential_lifecycle(StoreMcpCredentialLifecycle {
                credential,
                expected_aggregate_version: Some(expected_version),
                delivery: None,
                observed_at: canonical_timestamp(observed_at),
                idempotency,
                event,
                audit,
            })
            .await
            .map_err(ApplicationError::from)?;
        self.present(stored).await
    }

    async fn store_material(
        &self,
        material: McpCredentialMaterial,
        expected_aggregate_version: Option<u64>,
        idempotency: IdempotencyRequest,
        actor_id: Uuid,
        correlation_id: Uuid,
    ) -> ApplicationResult<McpCredentialMutationResult> {
        let (credential, secret) = material.into_parts();
        let delivery_expires_at = credential
            .updated_at()
            .checked_add_signed(self.delivery_ttl)
            .map(|expires_at| expires_at.min(credential.expires_at()))
            .ok_or_else(internal_delivery_error)?;
        let context = McpCredentialDelivery::encryption_context_for(
            credential.organization_id,
            credential.project_id,
            credential.environment_id,
            credential.id,
            credential.generation(),
            credential.updated_at(),
            delivery_expires_at,
        )
        .map_err(|_| internal_delivery_error())?;
        let encrypted = self
            .encryption
            .encrypt(secret.as_bytes(), &context)
            .await
            .map_err(encryption_error)?;
        drop(secret);
        let delivery = McpCredentialDelivery::new(
            credential.organization_id,
            credential.project_id,
            credential.environment_id,
            credential.id,
            credential.generation(),
            encrypted.key_id(),
            encrypted.ciphertext(),
            credential.updated_at(),
            delivery_expires_at,
        )
        .map_err(|_| internal_delivery_error())?;
        let event = McpCredentialChanged::envelope(&credential, correlation_id)
            .map_err(|_| internal_delivery_error())?;
        let audit = mcp_credential_audit_record(
            &credential,
            expected_aggregate_version,
            actor_id,
            correlation_id,
        )
        .map_err(|_| internal_audit_error())?;
        let observed_at = credential.updated_at();
        let stored = self
            .repository
            .store_mcp_credential_lifecycle(StoreMcpCredentialLifecycle {
                credential,
                expected_aggregate_version,
                delivery: Some(delivery),
                observed_at,
                idempotency,
                event,
                audit,
            })
            .await
            .map_err(ApplicationError::from)?;
        self.present(stored).await
    }

    async fn present(
        &self,
        result: McpCredentialLifecycleResult,
    ) -> ApplicationResult<McpCredentialMutationResult> {
        let secret = match &result.delivery {
            Some(delivery) => {
                let encrypted = EncryptedSecretValue::new(delivery.key_id(), delivery.ciphertext())
                    .map_err(|_| internal_delivery_error())?;
                let plaintext = self
                    .encryption
                    .decrypt(&encrypted, &delivery.encryption_context())
                    .await
                    .map_err(encryption_error)?;
                let plaintext = Zeroizing::new(plaintext);
                let secret = std::str::from_utf8(&plaintext)
                    .map(|value| Zeroizing::new(value.to_owned()))
                    .map_err(|_| internal_delivery_error())?;
                Some(McpCredentialSecret(
                    self.material
                        .verify_secret(&result.credential, secret)
                        .await
                        .map_err(|_| internal_delivery_error())?,
                ))
            }
            None => None,
        };
        Ok(McpCredentialMutationResult {
            credential: result.credential,
            secret,
            replayed: result.replayed,
        })
    }
}

fn material_error(error: McpCredentialIssuanceError) -> ApplicationError {
    match error {
        McpCredentialIssuanceError::InvalidRequest(message) => ApplicationError::Invalid(message),
        McpCredentialIssuanceError::Unavailable => ApplicationError::Unavailable(
            "MCP credential material generation is temporarily unavailable".into(),
        ),
        McpCredentialIssuanceError::IdentityCollision => ApplicationError::Unavailable(
            "MCP credential issuance exhausted its bounded identity retries".into(),
        ),
        McpCredentialIssuanceError::Repository(error) => error.into(),
    }
}

fn encryption_error(_error: SecretEncryptionError) -> ApplicationError {
    ApplicationError::Unavailable(
        "MCP credential encrypted delivery is temporarily unavailable".into(),
    )
}

fn internal_delivery_error() -> ApplicationError {
    ApplicationError::Internal("MCP credential encrypted delivery failed validation".into())
}

fn internal_audit_error() -> ApplicationError {
    ApplicationError::Internal("MCP credential audit record failed validation".into())
}

pub(crate) fn exact_credential(
    credential: Option<McpCredential>,
    project_id: ProjectId,
    environment_id: EnvironmentId,
) -> ApplicationResult<McpCredential> {
    credential
        .filter(|credential| {
            credential.project_id == project_id && credential.environment_id == environment_id
        })
        .ok_or_else(|| {
            ApplicationError::NotFound(
                "MCP credential not found in organization, project, and environment".into(),
            )
        })
}
