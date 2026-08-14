use super::{ResolvedConnectorAuthentication, ResolvedConnectorHttpRevision};
use crate::modules::connectors::domain::{
    ConnectorDefinition, ConnectorHttpAuthentication, ConnectorHttpDestination, ConnectorRevision,
};
use crate::modules::secrets::application::ExactSecretMaterializer;
use crate::modules::secrets::domain::{ISecretEncryptionService, ISecretRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use std::sync::Arc;
use std::time::Duration;
use url::Url;
use zeroize::Zeroizing;

/// Materializes one immutable Connector revision for immediate single-attempt execution.
///
/// The returned value is intentionally non-serializable and redacts destination/authentication
/// material. Callers must not cache it: every later attempt must call this service again so
/// Secrets can atomically recheck the exact version's scope and active state.
#[derive(Clone)]
pub struct ConnectorHttpRevisionMaterializer {
    secrets: ExactSecretMaterializer,
}

impl ConnectorHttpRevisionMaterializer {
    pub fn new(
        secrets: Arc<dyn ISecretRepository>,
        encryption: Arc<dyn ISecretEncryptionService>,
    ) -> Self {
        Self {
            secrets: ExactSecretMaterializer::new(secrets, encryption),
        }
    }

    pub async fn materialize(
        &self,
        revision: &ConnectorRevision,
    ) -> ApplicationResult<ResolvedConnectorHttpRevision> {
        revision.validate().map_err(ApplicationError::Internal)?;
        match &revision.definition {
            ConnectorDefinition::Http(definition) => {
                let spec = definition.spec();
                let endpoint = match &spec.destination {
                    ConnectorHttpDestination::LiteralHttps { endpoint } => {
                        Url::parse(endpoint).map_err(|_| materialization_unavailable())?
                    }
                    ConnectorHttpDestination::SecretHttpsUrl { reference } => {
                        let plaintext = self
                            .secrets
                            .materialize(
                                revision.organization_id,
                                revision.project_id,
                                revision.environment_id,
                                reference.secret_id,
                                reference.version,
                            )
                            .await?;
                        let endpoint = Zeroizing::new(
                            String::from_utf8(plaintext.as_bytes().to_vec())
                                .map_err(|_| materialization_unavailable())?,
                        );
                        Url::parse(endpoint.as_str()).map_err(|_| materialization_unavailable())?
                    }
                };
                let authentication = match &spec.authentication {
                    ConnectorHttpAuthentication::None => ResolvedConnectorAuthentication::none(),
                    ConnectorHttpAuthentication::HmacSha256 {
                        secret,
                        signature_header,
                        value_prefix,
                    } => {
                        let plaintext = self
                            .secrets
                            .materialize(
                                revision.organization_id,
                                revision.project_id,
                                revision.environment_id,
                                secret.secret_id,
                                secret.version,
                            )
                            .await?;
                        ResolvedConnectorAuthentication::hmac_sha256(
                            Zeroizing::new(plaintext.as_bytes().to_vec()),
                            signature_header,
                            value_prefix.clone(),
                        )
                        .map_err(|_| materialization_unavailable())?
                    }
                };
                ResolvedConnectorHttpRevision::new(
                    revision.id,
                    endpoint,
                    spec.method,
                    spec.request_content_type.clone(),
                    usize::try_from(spec.maximum_request_bytes)
                        .map_err(|_| materialization_unavailable())?,
                    usize::try_from(spec.maximum_response_bytes)
                        .map_err(|_| materialization_unavailable())?,
                    Duration::from_millis(spec.timeout_milliseconds),
                    spec.status_policy.clone(),
                    authentication,
                )
                .map_err(|_| materialization_unavailable())
            }
        }
    }
}

fn materialization_unavailable() -> ApplicationError {
    ApplicationError::Unavailable("Connector revision could not be materialized".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::connectors::domain::{
        ConnectorHttpDefinition, ConnectorHttpDefinitionSpec, ConnectorHttpMethod,
        ConnectorHttpStatusPolicy, ConnectorSecretReference,
    };
    use crate::modules::secrets::domain::{
        CreateSecretWrite, EncryptedSecretValue, Secret, SecretChanged, SecretEncryptionError,
        TransitionSecretVersion,
    };
    use crate::modules::secrets::infrastructure::InMemorySecretRepository;
    use crate::modules::shared_kernel::domain::{
        ConnectorProfileId, ConnectorRevisionId, EnvironmentId, IdempotencyRequest, OrganizationId,
        PrincipalId, ProjectId, ResourceName, SecretId,
    };
    use async_trait::async_trait;
    use chrono::{Duration as ChronoDuration, Utc};
    use std::sync::Mutex;
    use uuid::Uuid;

    #[derive(Default)]
    struct RecordingEncryption {
        ciphertexts: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl ISecretEncryptionService for RecordingEncryption {
        async fn encrypt(
            &self,
            _plaintext: &[u8],
            _context: &[u8],
        ) -> Result<EncryptedSecretValue, SecretEncryptionError> {
            Err(SecretEncryptionError::Rejected(
                "test-only decryptor".into(),
            ))
        }

        async fn decrypt(
            &self,
            value: &EncryptedSecretValue,
            _context: &[u8],
        ) -> Result<Vec<u8>, SecretEncryptionError> {
            self.ciphertexts
                .lock()
                .expect("recording encryption lock")
                .push(value.ciphertext().to_owned());
            match value.ciphertext() {
                "destination-ciphertext" => {
                    Ok(b"https://hooks.example.test/delivery?token=resolved".to_vec())
                }
                "hmac-ciphertext" => Ok(vec![b's'; 32]),
                _ => Err(SecretEncryptionError::Rejected(
                    "unexpected test ciphertext".into(),
                )),
            }
        }

        async fn health(&self) -> Result<bool, SecretEncryptionError> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn materializes_exact_active_versions_and_rechecks_revocation() {
        let now = Utc::now();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let destination_id = SecretId::new();
        let hmac_id = SecretId::new();
        let secrets = Arc::new(InMemorySecretRepository::new());
        let (destination, destination_version) = create_secret(
            secrets.as_ref(),
            organization_id,
            project_id,
            environment_id,
            destination_id,
            "destination-ciphertext",
            now,
        )
        .await;
        let (mut hmac, mut hmac_version) = create_secret(
            secrets.as_ref(),
            organization_id,
            project_id,
            environment_id,
            hmac_id,
            "hmac-ciphertext",
            now,
        )
        .await;
        let definition = ConnectorDefinition::Http(
            ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
                destination: ConnectorHttpDestination::SecretHttpsUrl {
                    reference: ConnectorSecretReference::new(destination.id, 1)
                        .expect("destination reference"),
                },
                method: ConnectorHttpMethod::Post,
                request_content_type: "application/json".into(),
                maximum_request_bytes: 1024,
                maximum_response_bytes: 1024,
                timeout_milliseconds: 1_000,
                status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
                authentication: ConnectorHttpAuthentication::HmacSha256 {
                    secret: ConnectorSecretReference::new(hmac.id, 1).expect("HMAC reference"),
                    signature_header: "x-a3s-signature".into(),
                    value_prefix: "v1=".into(),
                },
            })
            .expect("HTTP definition"),
        );
        let revision = ConnectorRevision::initial(
            organization_id,
            project_id,
            environment_id,
            ConnectorProfileId::new(),
            ConnectorRevisionId::new(),
            definition,
            PrincipalId::new(),
            now,
        )
        .expect("Connector revision");
        let encryption = Arc::new(RecordingEncryption::default());
        let materializer =
            ConnectorHttpRevisionMaterializer::new(secrets.clone(), encryption.clone());
        let resolved = materializer
            .materialize(&revision)
            .await
            .expect("materialized revision");
        let debug = format!("{resolved:?}");
        assert!(!debug.contains("hooks.example.test"));
        assert!(!debug.contains("resolved"));
        assert!(!debug.contains(&"s".repeat(32)));
        assert_eq!(
            encryption
                .ciphertexts
                .lock()
                .expect("recorded ciphertexts")
                .as_slice(),
            ["destination-ciphertext", "hmac-ciphertext"]
        );

        let expected_secret_version = hmac.aggregate_version;
        let expected_version = hmac_version.aggregate_version;
        hmac.revoke_version(&mut hmac_version, now + ChronoDuration::seconds(1))
            .expect("revoke HMAC version");
        let event = SecretChanged::version_revoked(&hmac, &hmac_version, Uuid::now_v7())
            .expect("revocation event");
        secrets
            .transition_version(TransitionSecretVersion {
                secret: hmac,
                version: hmac_version,
                expected_secret_version,
                expected_version,
                idempotency: IdempotencyRequest::new(
                    "connector-materializer-test",
                    "revoke",
                    hmac_id.as_uuid().as_bytes(),
                )
                .expect("revocation idempotency"),
                event,
            })
            .await
            .expect("store revocation");
        assert!(matches!(
            materializer.materialize(&revision).await,
            Err(ApplicationError::Forbidden(_))
        ));

        let foreign_scope_revision = ConnectorRevision::initial(
            organization_id,
            project_id,
            EnvironmentId::new(),
            ConnectorProfileId::new(),
            ConnectorRevisionId::new(),
            revision.definition.clone(),
            PrincipalId::new(),
            now,
        )
        .expect("foreign-scope Connector revision");
        assert!(matches!(
            materializer.materialize(&foreign_scope_revision).await,
            Err(ApplicationError::Forbidden(_))
        ));

        assert_eq!(destination_version.version, 1);
    }

    async fn create_secret(
        repository: &InMemorySecretRepository,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        secret_id: SecretId,
        ciphertext: &str,
        created_at: chrono::DateTime<Utc>,
    ) -> (Secret, crate::modules::secrets::domain::SecretVersion) {
        let (secret, version) = Secret::create(
            secret_id,
            organization_id,
            project_id,
            environment_id,
            ResourceName::parse(format!("secret-{secret_id}")).expect("Secret name"),
            EncryptedSecretValue::new("test-key", ciphertext).expect("encrypted value"),
            created_at,
        )
        .expect("Secret");
        repository
            .create(CreateSecretWrite {
                secret: secret.clone(),
                version: version.clone(),
                idempotency: IdempotencyRequest::new(
                    "connector-materializer-test",
                    secret_id.to_string(),
                    ciphertext.as_bytes(),
                )
                .expect("Secret idempotency"),
                event: SecretChanged::created(&secret, &version, Uuid::now_v7())
                    .expect("Secret event"),
            })
            .await
            .expect("store Secret");
        (secret, version)
    }
}
