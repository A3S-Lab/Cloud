use crate::modules::data::domain::ObjectNamespaceCredentialBinding;
use crate::modules::secrets::{
    IExactSecretMaterializer, IExactSecretVersionAccess, SecretPlaintext,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{Sha256Digest, StorageNamespaceId};
use std::sync::Arc;

/// Admission-time check for every exact Secret version in one S0 credential
/// binding. Secrets remains the sole active/revoked and tenant-scope authority.
#[derive(Clone)]
pub struct ObjectNamespaceCredentialAdmission {
    secrets: Arc<dyn IExactSecretVersionAccess>,
}

impl ObjectNamespaceCredentialAdmission {
    pub fn from_secret_version_access(secrets: Arc<dyn IExactSecretVersionAccess>) -> Self {
        Self { secrets }
    }

    pub async fn require_active(
        &self,
        binding: &ObjectNamespaceCredentialBinding,
    ) -> ApplicationResult<()> {
        binding.validate().map_err(ApplicationError::Internal)?;
        let spec = binding.spec();
        for reference in spec.references() {
            match self
                .secrets
                .require_reference(
                    spec.organization_id,
                    spec.project_id,
                    spec.environment_id,
                    reference,
                )
                .await
            {
                Ok(()) => {}
                Err(ApplicationError::Forbidden(_)) | Err(ApplicationError::NotFound(_)) => {
                    return Err(ApplicationError::Invalid(
                        "object namespace credential references must be active in the exact scope"
                            .into(),
                    ));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

/// Short-lived, non-serializable credential material for one provider call.
///
/// Values are zeroized by Secrets-owned `SecretPlaintext`; Debug output never
/// exposes them. Callers must not cache this value and must materialize again
/// for a later operation so revocation is rechecked atomically.
pub struct MaterializedObjectNamespaceCredentials {
    namespace_id: StorageNamespaceId,
    generation: u64,
    provider_profile_digest: Sha256Digest,
    binding_digest: Sha256Digest,
    access_key_id: SecretPlaintext,
    secret_access_key: SecretPlaintext,
    session_token: Option<SecretPlaintext>,
}

impl MaterializedObjectNamespaceCredentials {
    pub fn namespace_id(&self) -> StorageNamespaceId {
        self.namespace_id
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn provider_profile_digest(&self) -> &Sha256Digest {
        &self.provider_profile_digest
    }

    pub fn binding_digest(&self) -> &Sha256Digest {
        &self.binding_digest
    }

    pub fn access_key_id(&self) -> &[u8] {
        self.access_key_id.as_bytes()
    }

    pub fn secret_access_key(&self) -> &[u8] {
        self.secret_access_key.as_bytes()
    }

    pub fn session_token(&self) -> Option<&[u8]> {
        self.session_token.as_ref().map(SecretPlaintext::as_bytes)
    }
}

impl std::fmt::Debug for MaterializedObjectNamespaceCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MaterializedObjectNamespaceCredentials")
            .field("namespace_id", &self.namespace_id)
            .field("generation", &self.generation)
            .field("provider_profile_digest", &self.provider_profile_digest)
            .field("binding_digest", &self.binding_digest)
            .field("access_key_id", &"<redacted-secret-plaintext>")
            .field("secret_access_key", &"<redacted-secret-plaintext>")
            .field(
                "session_token",
                &self
                    .session_token
                    .as_ref()
                    .map(|_| "<redacted-secret-plaintext>"),
            )
            .finish()
    }
}

#[derive(Clone)]
pub struct ObjectNamespaceCredentialMaterializer {
    secrets: Arc<dyn IExactSecretMaterializer>,
}

impl ObjectNamespaceCredentialMaterializer {
    pub fn from_secret_materializer(secrets: Arc<dyn IExactSecretMaterializer>) -> Self {
        Self { secrets }
    }

    pub async fn materialize(
        &self,
        binding: &ObjectNamespaceCredentialBinding,
    ) -> ApplicationResult<MaterializedObjectNamespaceCredentials> {
        binding.validate().map_err(ApplicationError::Internal)?;
        let spec = binding.spec();
        let access_key_id = self
            .secrets
            .materialize_reference(
                spec.organization_id,
                spec.project_id,
                spec.environment_id,
                spec.access_key_id,
            )
            .await?;
        let secret_access_key = self
            .secrets
            .materialize_reference(
                spec.organization_id,
                spec.project_id,
                spec.environment_id,
                spec.secret_access_key,
            )
            .await?;
        let session_token = match spec.session_token {
            Some(reference) => Some(
                self.secrets
                    .materialize_reference(
                        spec.organization_id,
                        spec.project_id,
                        spec.environment_id,
                        reference,
                    )
                    .await?,
            ),
            None => None,
        };
        Ok(MaterializedObjectNamespaceCredentials {
            namespace_id: spec.namespace_id,
            generation: spec.generation,
            provider_profile_digest: spec.provider_profile_digest.clone(),
            binding_digest: binding.digest().clone(),
            access_key_id,
            secret_access_key,
            session_token,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::data::domain::ObjectNamespaceCredentialBindingSpec;
    use crate::modules::secrets::domain::{
        CreateSecretWrite, EncryptedSecretValue, ISecretEncryptionService, ISecretRepository,
        Secret, SecretChanged, SecretEncryptionError, TransitionSecretVersion,
    };
    use crate::modules::secrets::infrastructure::InMemorySecretRepository;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, IdempotencyRequest, OrganizationId, ProjectId, ResourceName, SecretId,
        SecretVersionReference,
    };
    use async_trait::async_trait;
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    struct TestEncryption;

    #[async_trait]
    impl ISecretEncryptionService for TestEncryption {
        async fn encrypt(
            &self,
            _plaintext: &[u8],
            _context: &[u8],
        ) -> Result<EncryptedSecretValue, SecretEncryptionError> {
            Err(SecretEncryptionError::Rejected(
                "test decryptor only".into(),
            ))
        }

        async fn decrypt(
            &self,
            value: &EncryptedSecretValue,
            _context: &[u8],
        ) -> Result<Vec<u8>, SecretEncryptionError> {
            Ok(format!("plain:{}", value.ciphertext()).into_bytes())
        }

        async fn health(&self) -> Result<bool, SecretEncryptionError> {
            Ok(true)
        }
    }

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }

    #[test]
    fn credential_path_delegates_secret_and_provider_mechanisms() {
        let source = include_str!("object_namespace_credentials.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        for forbidden in [
            "EncryptedSecretValue",
            ".decrypt(",
            "object_store::",
            "ImmutableObjectClient",
            "std::env",
            "reqwest::",
        ] {
            assert!(
                !production.contains(forbidden),
                "S0 credentials must delegate to Secrets and the provider adapter; found {forbidden}"
            );
        }
    }

    #[tokio::test]
    async fn admission_and_materialization_reuse_exact_secret_authority_without_leaking() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let repository = Arc::new(InMemorySecretRepository::new());
        let access_key_id = create_secret(
            repository.as_ref(),
            organization_id,
            project_id,
            environment_id,
            "access-key",
        )
        .await;
        let secret_access_key = create_secret(
            repository.as_ref(),
            organization_id,
            project_id,
            environment_id,
            "secret-key",
        )
        .await;
        let session_token = create_secret(
            repository.as_ref(),
            organization_id,
            project_id,
            environment_id,
            "session-token",
        )
        .await;
        let binding =
            ObjectNamespaceCredentialBinding::from_spec(ObjectNamespaceCredentialBindingSpec {
                organization_id,
                project_id,
                environment_id,
                namespace_id: StorageNamespaceId::new(),
                generation: 1,
                provider_profile_digest: digest('a'),
                access_key_id,
                secret_access_key,
                session_token: Some(session_token),
            })
            .expect("binding");

        ObjectNamespaceCredentialAdmission::new(repository.clone())
            .require_active(&binding)
            .await
            .expect("active binding");
        let material = ObjectNamespaceCredentialMaterializer::new(
            repository.clone(),
            Arc::new(TestEncryption),
        )
        .materialize(&binding)
        .await
        .expect("materialized credentials");
        assert_eq!(material.namespace_id(), binding.spec().namespace_id);
        assert_eq!(material.generation(), 1);
        assert_eq!(material.access_key_id(), b"plain:access-key");
        assert_eq!(material.secret_access_key(), b"plain:secret-key");
        assert_eq!(
            material.session_token(),
            Some(b"plain:session-token".as_slice())
        );
        let debug = format!("{material:?}");
        for plaintext in [
            "plain:access-key",
            "plain:secret-key",
            "plain:session-token",
        ] {
            assert!(!debug.contains(plaintext));
        }
        drop(material);

        revoke_secret_version(repository.as_ref(), organization_id, session_token).await;
        assert!(matches!(
            ObjectNamespaceCredentialAdmission::new(repository.clone())
                .require_active(&binding)
                .await,
            Err(ApplicationError::Invalid(_))
        ));
        assert!(matches!(
            ObjectNamespaceCredentialMaterializer::new(
                repository.clone(),
                Arc::new(TestEncryption)
            )
            .materialize(&binding)
            .await,
            Err(ApplicationError::Forbidden(_))
        ));

        let mut foreign = binding.spec().clone();
        foreign.environment_id = EnvironmentId::new();
        let foreign = ObjectNamespaceCredentialBinding::from_spec(foreign).expect("foreign");
        assert!(matches!(
            ObjectNamespaceCredentialAdmission::new(repository.clone())
                .require_active(&foreign)
                .await,
            Err(ApplicationError::Invalid(_))
        ));
        assert!(matches!(
            ObjectNamespaceCredentialMaterializer::new(repository, Arc::new(TestEncryption))
                .materialize(&foreign)
                .await,
            Err(ApplicationError::Forbidden(_))
        ));
    }

    async fn create_secret(
        repository: &InMemorySecretRepository,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        ciphertext: &str,
    ) -> SecretVersionReference {
        let secret_id = SecretId::new();
        let (secret, version) = Secret::create(
            secret_id,
            organization_id,
            project_id,
            environment_id,
            ResourceName::parse(format!("credential-{ciphertext}")).expect("name"),
            EncryptedSecretValue::new("test-key", ciphertext).expect("encrypted value"),
            Utc::now(),
        )
        .expect("Secret");
        repository
            .create(CreateSecretWrite {
                secret: secret.clone(),
                version: version.clone(),
                idempotency: IdempotencyRequest::new(
                    "object-namespace-credential-test",
                    ciphertext,
                    ciphertext.as_bytes(),
                )
                .expect("idempotency"),
                event: SecretChanged::created(&secret, &version, Uuid::now_v7()).expect("event"),
            })
            .await
            .expect("store Secret");
        SecretVersionReference::new(secret_id, 1).expect("reference")
    }

    async fn revoke_secret_version(
        repository: &InMemorySecretRepository,
        organization_id: OrganizationId,
        reference: SecretVersionReference,
    ) {
        let mut secret = repository
            .find(organization_id, reference.secret_id)
            .await
            .expect("Secret");
        let mut version = repository
            .find_version(organization_id, reference.secret_id, reference.version)
            .await
            .expect("Secret version");
        let expected_secret_version = secret.aggregate_version;
        let expected_version = version.aggregate_version;
        secret
            .revoke_version(&mut version, secret.updated_at + Duration::seconds(1))
            .expect("revoke version");
        let event = SecretChanged::version_revoked(&secret, &version, Uuid::now_v7())
            .expect("revocation event");
        repository
            .transition_version(TransitionSecretVersion {
                secret,
                version,
                expected_secret_version,
                expected_version,
                idempotency: IdempotencyRequest::new(
                    "object-namespace-credential-test",
                    format!("revoke-{}-{}", reference.secret_id, reference.version),
                    reference.secret_id.as_uuid().as_bytes(),
                )
                .expect("revocation idempotency"),
                event,
            })
            .await
            .expect("store revocation");
    }
}
