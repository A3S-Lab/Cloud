use crate::modules::workloads::domain::entities::{OciArtifact, OciArtifactReference};
use crate::modules::workloads::domain::services::{
    IOciArtifactResolver, OciArtifactResolutionError,
};
use async_trait::async_trait;
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, WWW_AUTHENTICATE,
};
use reqwest::{Client, Response, StatusCode, Url};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::time::Duration;

const DOCKER_CONTENT_DIGEST: &str = "docker-content-digest";
const MANIFEST_ACCEPT: &str = "application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json";
const MAX_TOKEN_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
pub struct OciRegistryArtifactResolver {
    client: Client,
    insecure_hosts: BTreeSet<String>,
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
        })
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
        let token = self.bearer_token(challenge, repository).await?;
        self.client
            .head(url)
            .header(ACCEPT, MANIFEST_ACCEPT)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .send()
            .await
            .map_err(registry_error)
    }

    async fn bearer_token(
        &self,
        challenge: &HeaderValue,
        repository: &str,
    ) -> Result<String, OciArtifactResolutionError> {
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
        let response = self.client.get(url).send().await.map_err(registry_error)?;
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
        let body = response.bytes().await.map_err(registry_error)?;
        if body.len() > MAX_TOKEN_RESPONSE_BYTES {
            return Err(OciArtifactResolutionError::Protocol(
                "registry token response exceeds the supported size".into(),
            ));
        }
        let token: TokenResponse = serde_json::from_slice(&body).map_err(|error| {
            OciArtifactResolutionError::Protocol(format!(
                "registry token response is invalid: {error}"
            ))
        })?;
        let token = token.token.or(token.access_token).ok_or_else(|| {
            OciArtifactResolutionError::Protocol("registry token response omitted its token".into())
        })?;
        if token.is_empty() || token.len() > 64 * 1024 || token.contains(['\0', '\r', '\n']) {
            return Err(OciArtifactResolutionError::Protocol(
                "registry returned an invalid bearer token".into(),
            ));
        }
        Ok(token)
    }
}

#[async_trait]
impl IOciArtifactResolver for OciRegistryArtifactResolver {
    async fn resolve(
        &self,
        reference: &OciArtifactReference,
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
    let parameters = challenge
        .strip_prefix("Bearer ")
        .ok_or(OciArtifactResolutionError::Unauthorized)?;
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
    use axum::extract::State;
    use axum::http::Response;
    use axum::routing::head;
    use axum::Router;
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

        let initial = resolver.resolve(&reference).await.expect("initial digest");
        assert_eq!(initial.digest, first);
        *digest.write().expect("test registry digest") = second.clone();
        let moved = resolver.resolve(&reference).await.expect("moved digest");
        assert_eq!(moved.digest, second);

        let pinned = OciArtifactReference {
            expected_digest: Some(initial.digest),
            ..reference
        };
        assert!(matches!(
            resolver.resolve(&pinned).await,
            Err(OciArtifactResolutionError::Protocol(_))
        ));
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
}
