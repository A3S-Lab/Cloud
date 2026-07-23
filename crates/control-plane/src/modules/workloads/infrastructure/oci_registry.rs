use crate::infrastructure::{required_registry_header, OciRegistryClient, OciRegistryClientError};
use crate::modules::secrets::domain::{
    secret_encryption_context, ISecretEncryptionService, ISecretRepository,
};
use crate::modules::workloads::domain::entities::{OciArtifact, OciArtifactReference};
use crate::modules::workloads::domain::services::{
    IOciArtifactResolver, OciArtifactResolutionError, OciRegistryCredentialReference,
};
use a3s_cloud_contracts::RegistryCredentialMaterial;
use async_trait::async_trait;
use reqwest::header::CONTENT_TYPE;
use reqwest::{Response, StatusCode, Url};
use std::sync::Arc;
use std::time::Duration;
use zeroize::Zeroizing;

#[cfg(test)]
use reqwest::header::{HeaderValue, AUTHORIZATION, WWW_AUTHENTICATE};

const DOCKER_CONTENT_DIGEST: &str = "docker-content-digest";
const MANIFEST_ACCEPT: &str = "application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json";

#[derive(Clone)]
pub struct OciRegistryArtifactResolver {
    client: OciRegistryClient,
    registry_secret_material: Option<RegistrySecretMaterialAccess>,
}

#[derive(Clone)]
struct RegistrySecretMaterialAccess {
    secrets: Arc<dyn ISecretRepository>,
    encryption: Arc<dyn ISecretEncryptionService>,
}

impl OciRegistryArtifactResolver {
    pub fn new(
        request_timeout: Duration,
        insecure_hosts: impl IntoIterator<Item = String>,
    ) -> Result<Self, String> {
        Ok(Self {
            client: OciRegistryClient::new(request_timeout, insecure_hosts)?,
            registry_secret_material: None,
        })
    }

    pub fn with_registry_secret_material(
        mut self,
        secrets: Arc<dyn ISecretRepository>,
        encryption: Arc<dyn ISecretEncryptionService>,
    ) -> Self {
        self.registry_secret_material = Some(RegistrySecretMaterialAccess {
            secrets,
            encryption,
        });
        self
    }

    async fn request_manifest(
        &self,
        url: Url,
        repository: &str,
        registry_credential: Option<&OciRegistryCredentialReference>,
    ) -> Result<Response, OciArtifactResolutionError> {
        match self
            .client
            .head_manifest(url.clone(), repository, "pull", None, MANIFEST_ACCEPT)
            .await
        {
            Ok(response) => return Ok(response),
            Err(OciRegistryClientError::Unauthorized) if registry_credential.is_some() => {}
            Err(error) => return Err(map_registry_client_error(error)),
        }
        let credential = self
            .materialize_registry_credential(registry_credential)
            .await?;
        self.client
            .head_manifest(url, repository, "pull", Some(&credential), MANIFEST_ACCEPT)
            .await
            .map_err(map_registry_client_error)
    }

    #[cfg(test)]
    async fn bearer_token(
        &self,
        challenge: &HeaderValue,
        repository: &str,
        credential: Option<&RegistryCredentialMaterial>,
    ) -> Result<Zeroizing<String>, OciArtifactResolutionError> {
        self.client
            .bearer_token(challenge, repository, "pull", credential)
            .await
            .map_err(map_registry_client_error)
    }

    async fn materialize_registry_credential(
        &self,
        reference: Option<&OciRegistryCredentialReference>,
    ) -> Result<RegistryCredentialMaterial, OciArtifactResolutionError> {
        let reference = reference.ok_or(OciArtifactResolutionError::Unauthorized)?;
        reference
            .validate()
            .map_err(OciArtifactResolutionError::Credential)?;
        let access = self.registry_secret_material.as_ref().ok_or_else(|| {
            OciArtifactResolutionError::Credential(
                "Secret material access is not configured".into(),
            )
        })?;
        let secret = access
            .secrets
            .find(reference.organization_id, reference.secret_id)
            .await
            .map_err(|_| {
                OciArtifactResolutionError::Credential("bound Secret is not available".into())
            })?;
        if secret.id != reference.secret_id
            || secret.organization_id != reference.organization_id
            || secret.project_id != reference.project_id
            || secret.environment_id != reference.environment_id
        {
            return Err(OciArtifactResolutionError::Credential(
                "bound Secret is outside the workload environment".into(),
            ));
        }
        let version = access
            .secrets
            .find_version(
                reference.organization_id,
                reference.secret_id,
                reference.version,
            )
            .await
            .map_err(|_| {
                OciArtifactResolutionError::Credential(
                    "bound Secret version is not available".into(),
                )
            })?;
        if version.secret_id != reference.secret_id
            || version.version != reference.version
            || !version.is_materializable(&secret)
        {
            return Err(OciArtifactResolutionError::Credential(
                "bound Secret version is not active".into(),
            ));
        }
        let context = secret_encryption_context(
            reference.organization_id,
            reference.secret_id,
            reference.version,
        )
        .map_err(|_| {
            OciArtifactResolutionError::Credential(
                "bound Secret encryption context is invalid".into(),
            )
        })?;
        let plaintext = Zeroizing::new(
            access
                .encryption
                .decrypt(&version.encrypted_value, &context)
                .await
                .map_err(|_| {
                    OciArtifactResolutionError::Credential(
                        "bound Secret could not be decrypted".into(),
                    )
                })?,
        );
        RegistryCredentialMaterial::parse(&plaintext).map_err(|_| {
            OciArtifactResolutionError::Credential(
                "bound Secret does not contain valid registry credential material".into(),
            )
        })
    }
}

#[async_trait]
impl IOciArtifactResolver for OciRegistryArtifactResolver {
    async fn resolve(
        &self,
        reference: &OciArtifactReference,
        registry_credential: Option<&OciRegistryCredentialReference>,
    ) -> Result<OciArtifact, OciArtifactResolutionError> {
        reference
            .validate()
            .map_err(OciArtifactResolutionError::InvalidReference)?;
        let (registry, repository) = reference
            .registry_and_repository()
            .map_err(OciArtifactResolutionError::InvalidReference)?;
        let manifest_reference = reference
            .manifest_reference()
            .map_err(OciArtifactResolutionError::InvalidReference)?;
        let response = self
            .request_manifest(
                self.client
                    .manifest_url(registry, repository, manifest_reference)
                    .map_err(map_registry_client_error)?,
                repository,
                registry_credential,
            )
            .await?;
        match response.status() {
            status if status.is_success() => {}
            StatusCode::NOT_FOUND => return Err(OciArtifactResolutionError::NotFound),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return Err(OciArtifactResolutionError::Unauthorized)
            }
            status => {
                return Err(OciArtifactResolutionError::Registry(format!(
                    "registry returned HTTP {status}"
                )))
            }
        }
        let digest = required_registry_header(response.headers(), DOCKER_CONTENT_DIGEST)
            .map_err(map_registry_client_error)?;
        let media_type = required_registry_header(response.headers(), CONTENT_TYPE.as_str())
            .map_err(map_registry_client_error)?
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned();
        if reference
            .expected_digest
            .as_ref()
            .is_some_and(|expected| expected != &digest)
            || reference
                .bound_digest()
                .map_err(OciArtifactResolutionError::InvalidReference)?
                .is_some_and(|expected| expected != digest)
        {
            return Err(OciArtifactResolutionError::Protocol(
                "registry manifest digest does not match the requested digest".into(),
            ));
        }
        let artifact = OciArtifact {
            uri: format!("oci://{registry}/{repository}@{digest}"),
            digest,
            media_type,
        };
        artifact
            .validate()
            .map_err(OciArtifactResolutionError::Protocol)?;
        Ok(artifact)
    }
}

fn map_registry_client_error(error: OciRegistryClientError) -> OciArtifactResolutionError {
    match error {
        OciRegistryClientError::Invalid(message) => {
            OciArtifactResolutionError::InvalidReference(message)
        }
        OciRegistryClientError::Unauthorized => OciArtifactResolutionError::Unauthorized,
        OciRegistryClientError::Protocol(message) => OciArtifactResolutionError::Protocol(message),
        OciRegistryClientError::Transport(message) | OciRegistryClientError::Storage(message) => {
            OciArtifactResolutionError::Registry(message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::fleet::infrastructure::LocalKeyEncryptionService;
    use crate::modules::secrets::domain::{
        secret_encryption_context, CreateSecretWrite, ISecretEncryptionService, ISecretRepository,
        Secret, SecretChanged,
    };
    use crate::modules::secrets::infrastructure::InMemorySecretRepository;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, IdempotencyRequest, OrganizationId, ProjectId, ResourceName, SecretId,
    };
    use crate::modules::workloads::domain::services::OciRegistryCredentialReference;
    use axum::extract::State;
    use axum::http::{HeaderMap as AxumHeaderMap, Response};
    use axum::routing::{get, head};
    use axum::Router;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, RwLock};

    #[tokio::test]
    async fn resolves_the_registry_digest_and_detects_a_moved_tag() {
        let first = format!("sha256:{}", "a".repeat(64));
        let second = format!("sha256:{}", "b".repeat(64));
        let digest = Arc::new(RwLock::new(first.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test registry listener");
        let address = listener.local_addr().expect("test registry address");
        let router = Router::new()
            .route("/v2/team/app/manifests/stable", head(manifest_head))
            .with_state(digest.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("test registry server")
        });
        let resolver =
            OciRegistryArtifactResolver::new(Duration::from_secs(2), [address.to_string()])
                .expect("resolver");
        let reference = OciArtifactReference {
            uri: format!("oci://{address}/team/app:stable"),
            expected_digest: None,
        };

        let initial = resolver
            .resolve(&reference, None)
            .await
            .expect("initial digest");
        assert_eq!(initial.digest, first);
        *digest.write().expect("test registry digest") = second.clone();
        let moved = resolver
            .resolve(&reference, None)
            .await
            .expect("moved digest");
        assert_eq!(moved.digest, second);

        let pinned = OciArtifactReference {
            expected_digest: Some(initial.digest),
            ..reference
        };
        assert!(matches!(
            resolver.resolve(&pinned, None).await,
            Err(OciArtifactResolutionError::Protocol(_))
        ));
        server.abort();
    }

    #[tokio::test]
    async fn resolves_a_basic_authenticated_manifest_from_bound_secret_material() {
        let digest = format!("sha256:{}", "c".repeat(64));
        let username = "registry-user";
        let password = "registry-password";
        let state = Arc::new(BasicRegistryState {
            digest: digest.clone(),
            expected_authorization: format!(
                "Basic {}",
                STANDARD.encode(format!("{username}:{password}"))
            ),
            anonymous_requests: AtomicUsize::new(0),
            authenticated_requests: AtomicUsize::new(0),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test registry listener");
        let address = listener.local_addr().expect("test registry address");
        let router = Router::new()
            .route(
                "/v2/team/private/manifests/stable",
                head(basic_manifest_head),
            )
            .with_state(Arc::clone(&state));
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("test registry server")
        });

        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let secret_id = SecretId::new();
        let secrets = Arc::new(InMemorySecretRepository::new());
        let material = serde_json::to_vec(&serde_json::json!({
            "schema": RegistryCredentialMaterial::SCHEMA,
            "username": username,
            "password": password,
        }))
        .expect("credential material");
        let key_directory = tempfile::tempdir().expect("key directory");
        let key_encryption = Arc::new(
            LocalKeyEncryptionService::load_or_create(key_directory.path().join("key"))
                .expect("local encryption"),
        );
        let context = secret_encryption_context(organization_id, secret_id, 1)
            .expect("Secret encryption context");
        let encrypted = key_encryption
            .encrypt(&material, &context)
            .await
            .expect("encrypted value");
        let (secret, version) = Secret::create(
            secret_id,
            organization_id,
            project_id,
            environment_id,
            ResourceName::parse("registry credential").expect("Secret name"),
            encrypted,
            chrono::Utc::now(),
        )
        .expect("Secret");
        secrets
            .create(CreateSecretWrite {
                secret: secret.clone(),
                version: version.clone(),
                idempotency: IdempotencyRequest::new(
                    "test.registry-credential",
                    "create",
                    secret_id.as_uuid().as_bytes(),
                )
                .expect("Secret idempotency"),
                event: SecretChanged::created(&secret, &version, uuid::Uuid::now_v7())
                    .expect("Secret event"),
            })
            .await
            .expect("store Secret");
        let secret_repository: Arc<dyn ISecretRepository> = secrets;
        let encryption: Arc<dyn ISecretEncryptionService> = key_encryption;
        let resolver =
            OciRegistryArtifactResolver::new(Duration::from_secs(2), [address.to_string()])
                .expect("resolver")
                .with_registry_secret_material(secret_repository, encryption);
        let reference = OciArtifactReference {
            uri: format!("oci://{address}/team/private:stable"),
            expected_digest: None,
        };
        let credential = OciRegistryCredentialReference {
            organization_id,
            project_id,
            environment_id,
            secret_id,
            version: 1,
        };

        assert!(matches!(
            resolver.resolve(&reference, None).await,
            Err(OciArtifactResolutionError::Unauthorized)
        ));
        let wrong_scope = OciRegistryCredentialReference {
            project_id: ProjectId::new(),
            ..credential
        };
        let error = resolver
            .resolve(&reference, Some(&wrong_scope))
            .await
            .expect_err("cross-environment registry credential");
        assert!(matches!(&error, OciArtifactResolutionError::Credential(_)));
        assert!(!format!("{error:?}").contains(password));
        let artifact = resolver
            .resolve(&reference, Some(&credential))
            .await
            .expect("authenticated digest");
        assert_eq!(artifact.digest, digest);
        assert_eq!(state.anonymous_requests.load(Ordering::SeqCst), 4);
        assert_eq!(state.authenticated_requests.load(Ordering::SeqCst), 1);
        server.abort();
    }

    #[tokio::test]
    async fn authenticates_a_bearer_token_request_with_registry_secret_material() {
        let username = "token-user";
        let password = "token-password";
        let state = Arc::new(TokenServiceState {
            expected_authorization: format!(
                "Basic {}",
                STANDARD.encode(format!("{username}:{password}"))
            ),
            authenticated_requests: AtomicUsize::new(0),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test token listener");
        let address = listener.local_addr().expect("test token address");
        let router = Router::new()
            .route("/token", get(registry_token))
            .with_state(Arc::clone(&state));
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("test token server")
        });
        let resolver =
            OciRegistryArtifactResolver::new(Duration::from_secs(2), [address.to_string()])
                .expect("resolver");
        let challenge = HeaderValue::from_str(&format!(
            "Bearer realm=\"http://{address}/token\",service=\"test-registry\""
        ))
        .expect("Bearer challenge");
        let credential = RegistryCredentialMaterial::parse(
            serde_json::to_string(&serde_json::json!({
                "schema": RegistryCredentialMaterial::SCHEMA,
                "username": username,
                "password": password,
            }))
            .expect("credential JSON")
            .as_bytes(),
        )
        .expect("registry credential");

        assert!(matches!(
            resolver
                .bearer_token(&challenge, "team/private", None)
                .await,
            Err(OciArtifactResolutionError::Unauthorized)
        ));
        let token = resolver
            .bearer_token(&challenge, "team/private", Some(&credential))
            .await
            .expect("authenticated token");
        assert_eq!(token.as_str(), "issued-test-token");
        assert_eq!(state.authenticated_requests.load(Ordering::SeqCst), 1);
        server.abort();
    }

    async fn manifest_head(State(digest): State<Arc<RwLock<String>>>) -> Response<String> {
        Response::builder()
            .status(StatusCode::OK)
            .header(
                DOCKER_CONTENT_DIGEST,
                digest.read().expect("test registry digest").as_str(),
            )
            .header(CONTENT_TYPE, "application/vnd.oci.image.manifest.v1+json")
            .body(String::new())
            .expect("test registry response")
    }

    struct BasicRegistryState {
        digest: String,
        expected_authorization: String,
        anonymous_requests: AtomicUsize,
        authenticated_requests: AtomicUsize,
    }

    async fn basic_manifest_head(
        State(state): State<Arc<BasicRegistryState>>,
        headers: AxumHeaderMap,
    ) -> Response<String> {
        if headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            != Some(state.expected_authorization.as_str())
        {
            state.anonymous_requests.fetch_add(1, Ordering::SeqCst);
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header(WWW_AUTHENTICATE, "Basic realm=\"A3S Cloud test\"")
                .body(String::new())
                .expect("unauthorized registry response");
        }
        state.authenticated_requests.fetch_add(1, Ordering::SeqCst);
        Response::builder()
            .status(StatusCode::OK)
            .header(DOCKER_CONTENT_DIGEST, &state.digest)
            .header(CONTENT_TYPE, "application/vnd.oci.image.manifest.v1+json")
            .body(String::new())
            .expect("authenticated registry response")
    }

    struct TokenServiceState {
        expected_authorization: String,
        authenticated_requests: AtomicUsize,
    }

    async fn registry_token(
        State(state): State<Arc<TokenServiceState>>,
        headers: AxumHeaderMap,
    ) -> Response<String> {
        if headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            != Some(state.expected_authorization.as_str())
        {
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(String::new())
                .expect("unauthorized token response");
        }
        state.authenticated_requests.fetch_add(1, Ordering::SeqCst);
        Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "application/json")
            .body(r#"{"token":"issued-test-token"}"#.into())
            .expect("token response")
    }
}
