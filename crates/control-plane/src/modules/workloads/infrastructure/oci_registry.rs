use crate::modules::secrets::domain::{
    secret_encryption_context, ISecretEncryptionService, ISecretRepository,
};
use crate::modules::workloads::domain::entities::{OciArtifact, OciArtifactReference};
use crate::modules::workloads::domain::services::{
    IOciArtifactResolver, OciArtifactResolutionError, OciRegistryCredentialReference,
};
use a3s_cloud_contracts::RegistryCredentialMaterial;
use async_trait::async_trait;
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, WWW_AUTHENTICATE,
};
use reqwest::{Client, Response, StatusCode, Url};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;
use zeroize::{Zeroize, Zeroizing};

const DOCKER_CONTENT_DIGEST: &str = "docker-content-digest";
const MANIFEST_ACCEPT: &str = "application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json";
const MAX_TOKEN_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
pub struct OciRegistryArtifactResolver {
    client: Client,
    insecure_hosts: BTreeSet<String>,
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
        if request_timeout.is_zero() {
            return Err("OCI registry request timeout must be positive".into());
        }
        let insecure_hosts = insecure_hosts.into_iter().collect::<BTreeSet<_>>();
        if insecure_hosts.iter().any(|host| !valid_registry_host(host)) {
            return Err("OCI insecure registry hosts must be explicit host[:port] values".into());
        }
        let client = Client::builder()
            .timeout(request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("a3s-cloud-control-plane/0.1")
            .build()
            .map_err(|error| format!("could not build OCI registry client: {error}"))?;
        Ok(Self {
            client,
            insecure_hosts,
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

    fn manifest_url(
        &self,
        registry: &str,
        repository: &str,
        manifest_reference: &str,
    ) -> Result<Url, OciArtifactResolutionError> {
        let scheme = if self.insecure_hosts.contains(registry) {
            "http"
        } else {
            "https"
        };
        Url::parse(&format!(
            "{scheme}://{registry}/v2/{repository}/manifests/{manifest_reference}"
        ))
        .map_err(|error| OciArtifactResolutionError::InvalidReference(error.to_string()))
    }

    async fn request_manifest(
        &self,
        url: Url,
        repository: &str,
        registry_credential: Option<&OciRegistryCredentialReference>,
    ) -> Result<Response, OciArtifactResolutionError> {
        let response = self
            .client
            .head(url.clone())
            .header(ACCEPT, MANIFEST_ACCEPT)
            .send()
            .await
            .map_err(registry_error)?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return Ok(response);
        }
        let challenge = response
            .headers()
            .get(WWW_AUTHENTICATE)
            .ok_or(OciArtifactResolutionError::Unauthorized)?;
        if authentication_scheme_is(challenge, "Basic")? {
            let credential = self
                .materialize_registry_credential(registry_credential)
                .await?;
            return self
                .client
                .head(url)
                .header(ACCEPT, MANIFEST_ACCEPT)
                .basic_auth(credential.username(), Some(credential.password()))
                .send()
                .await
                .map_err(registry_error);
        }
        if authentication_scheme_is(challenge, "Bearer")? {
            let credential = match registry_credential {
                Some(reference) => Some(
                    self.materialize_registry_credential(Some(reference))
                        .await?,
                ),
                None => None,
            };
            let token = self
                .bearer_token(challenge, repository, credential.as_ref())
                .await?;
            let authorization_value = Zeroizing::new(format!("Bearer {}", token.as_str()));
            let mut authorization = HeaderValue::from_bytes(authorization_value.as_bytes())
                .map_err(|_| {
                    OciArtifactResolutionError::Protocol(
                        "registry returned an invalid bearer token".into(),
                    )
                })?;
            authorization.set_sensitive(true);
            return self
                .client
                .head(url)
                .header(ACCEPT, MANIFEST_ACCEPT)
                .header(AUTHORIZATION, authorization)
                .send()
                .await
                .map_err(registry_error);
        }
        Err(OciArtifactResolutionError::Unauthorized)
    }

    async fn bearer_token(
        &self,
        challenge: &HeaderValue,
        repository: &str,
        credential: Option<&RegistryCredentialMaterial>,
    ) -> Result<Zeroizing<String>, OciArtifactResolutionError> {
        let challenge = challenge
            .to_str()
            .map_err(|_| OciArtifactResolutionError::Unauthorized)?;
        let parameters = parse_bearer_challenge(challenge)?;
        let realm = parameters
            .get("realm")
            .ok_or(OciArtifactResolutionError::Unauthorized)?;
        let mut url = Url::parse(realm).map_err(|_| OciArtifactResolutionError::Unauthorized)?;
        if url.scheme() != "https"
            && !(url.scheme() == "http"
                && url
                    .host_str()
                    .map(|host| {
                        let authority = url
                            .port()
                            .map(|port| format!("{host}:{port}"))
                            .unwrap_or_else(|| host.to_owned());
                        self.insecure_hosts.contains(&authority)
                    })
                    .unwrap_or(false))
        {
            return Err(OciArtifactResolutionError::Unauthorized);
        }
        {
            let mut query = url.query_pairs_mut();
            if let Some(service) = parameters.get("service") {
                query.append_pair("service", service);
            }
            query.append_pair("scope", &format!("repository:{repository}:pull"));
        }
        let mut request = self.client.get(url);
        if let Some(credential) = credential {
            request = request.basic_auth(credential.username(), Some(credential.password()));
        }
        let response = request.send().await.map_err(registry_error)?;
        if !response.status().is_success() {
            return Err(OciArtifactResolutionError::Unauthorized);
        }
        if response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok())
            .is_some_and(|length| length > MAX_TOKEN_RESPONSE_BYTES)
        {
            return Err(OciArtifactResolutionError::Protocol(
                "registry token response exceeds the supported size".into(),
            ));
        }
        let body = Zeroizing::new(response.bytes().await.map_err(registry_error)?.to_vec());
        if body.len() > MAX_TOKEN_RESPONSE_BYTES {
            return Err(OciArtifactResolutionError::Protocol(
                "registry token response exceeds the supported size".into(),
            ));
        }
        let mut token: TokenResponse = serde_json::from_slice(&body).map_err(|error| {
            OciArtifactResolutionError::Protocol(format!(
                "registry token response is invalid: {error}"
            ))
        })?;
        let token = token
            .token
            .take()
            .or_else(|| token.access_token.take())
            .ok_or_else(|| {
                OciArtifactResolutionError::Protocol(
                    "registry token response omitted its token".into(),
                )
            })?;
        if token.is_empty() || token.len() > 64 * 1024 || token.contains(['\0', '\r', '\n']) {
            return Err(OciArtifactResolutionError::Protocol(
                "registry returned an invalid bearer token".into(),
            ));
        }
        Ok(Zeroizing::new(token))
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
                self.manifest_url(registry, repository, manifest_reference)?,
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
        let digest = required_header(response.headers(), DOCKER_CONTENT_DIGEST)?;
        let media_type = required_header(response.headers(), CONTENT_TYPE.as_str())?
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

#[derive(Deserialize)]
struct TokenResponse {
    token: Option<String>,
    access_token: Option<String>,
}

impl Drop for TokenResponse {
    fn drop(&mut self) {
        if let Some(token) = &mut self.token {
            token.zeroize();
        }
        if let Some(token) = &mut self.access_token {
            token.zeroize();
        }
    }
}

fn required_header(headers: &HeaderMap, name: &str) -> Result<String, OciArtifactResolutionError> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= 4096)
        .map(str::to_owned)
        .ok_or_else(|| {
            OciArtifactResolutionError::Protocol(format!(
                "registry response omitted a valid {name} header"
            ))
        })
}

fn parse_bearer_challenge(
    challenge: &str,
) -> Result<std::collections::BTreeMap<String, String>, OciArtifactResolutionError> {
    let (scheme, parameters) = challenge
        .split_once(' ')
        .ok_or(OciArtifactResolutionError::Unauthorized)?;
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return Err(OciArtifactResolutionError::Unauthorized);
    }
    let mut result = std::collections::BTreeMap::new();
    for parameter in parameters.split(',') {
        let (key, value) = parameter
            .trim()
            .split_once('=')
            .ok_or(OciArtifactResolutionError::Unauthorized)?;
        let value = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .ok_or(OciArtifactResolutionError::Unauthorized)?;
        if key.is_empty()
            || value.is_empty()
            || value.contains(['\0', '\r', '\n', '"'])
            || result.insert(key.to_owned(), value.to_owned()).is_some()
        {
            return Err(OciArtifactResolutionError::Unauthorized);
        }
    }
    Ok(result)
}

fn authentication_scheme_is(
    challenge: &HeaderValue,
    expected: &str,
) -> Result<bool, OciArtifactResolutionError> {
    let challenge = challenge
        .to_str()
        .map_err(|_| OciArtifactResolutionError::Unauthorized)?;
    Ok(challenge
        .split_ascii_whitespace()
        .next()
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case(expected)))
}

fn valid_registry_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 255
        && !host.contains(['/', '@', '\\', '\0', '\r', '\n', ' ', '\t'])
}

fn registry_error(error: reqwest::Error) -> OciArtifactResolutionError {
    OciArtifactResolutionError::Registry(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::secrets::domain::{
        CreateSecretWrite, EncryptedSecretValue, ISecretEncryptionService, ISecretRepository,
        Secret, SecretChanged, SecretEncryptionError,
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
    use zeroize::Zeroizing;

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
        let encrypted =
            EncryptedSecretValue::new("test:key", "test:ciphertext").expect("encrypted value");
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
        let material = serde_json::to_vec(&serde_json::json!({
            "schema": RegistryCredentialMaterial::SCHEMA,
            "username": username,
            "password": password,
        }))
        .expect("credential material");
        let secret_repository: Arc<dyn ISecretRepository> = secrets;
        let encryption: Arc<dyn ISecretEncryptionService> =
            Arc::new(FixedEncryption(Zeroizing::new(material)));
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
        assert_eq!(state.anonymous_requests.load(Ordering::SeqCst), 3);
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

    struct FixedEncryption(Zeroizing<Vec<u8>>);

    #[async_trait]
    impl ISecretEncryptionService for FixedEncryption {
        async fn encrypt(
            &self,
            _plaintext: &[u8],
            _context: &[u8],
        ) -> Result<EncryptedSecretValue, SecretEncryptionError> {
            Err(SecretEncryptionError::Unavailable(
                "test encryption is read-only".into(),
            ))
        }

        async fn decrypt(
            &self,
            _value: &EncryptedSecretValue,
            _context: &[u8],
        ) -> Result<Vec<u8>, SecretEncryptionError> {
            Ok(self.0.as_slice().to_vec())
        }

        async fn health(&self) -> Result<bool, SecretEncryptionError> {
            Ok(true)
        }
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
