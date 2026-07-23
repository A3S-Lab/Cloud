use a3s_cloud_contracts::RegistryCredentialMaterial;
use futures_util::StreamExt;
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, WWW_AUTHENTICATE,
};
use reqwest::{Client, Method, Response, StatusCode, Url};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;
use tokio_util::io::ReaderStream;
use zeroize::{Zeroize, Zeroizing};

const MAX_TOKEN_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum OciRegistryClientError {
    #[error("OCI registry request is invalid: {0}")]
    Invalid(String),
    #[error("OCI registry authorization was rejected")]
    Unauthorized,
    #[error("OCI registry protocol error: {0}")]
    Protocol(String),
    #[error("OCI registry transport failed: {0}")]
    Transport(String),
    #[error("OCI registry request body failed: {0}")]
    Storage(String),
}

#[derive(Clone)]
pub(crate) struct OciRegistryClient {
    client: Client,
    insecure_hosts: BTreeSet<String>,
}

impl OciRegistryClient {
    pub(crate) fn new(
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

    pub(crate) fn manifest_url(
        &self,
        registry: &str,
        repository: &str,
        reference: &str,
    ) -> Result<Url, OciRegistryClientError> {
        self.repository_url(registry, repository, &format!("manifests/{reference}"))
    }

    pub(crate) fn blob_url(
        &self,
        registry: &str,
        repository: &str,
        digest: &str,
    ) -> Result<Url, OciRegistryClientError> {
        self.repository_url(registry, repository, &format!("blobs/{digest}"))
    }

    pub(crate) fn upload_start_url(
        &self,
        registry: &str,
        repository: &str,
    ) -> Result<Url, OciRegistryClientError> {
        self.repository_url(registry, repository, "blobs/uploads/")
    }

    pub(crate) fn upload_completion_url(
        &self,
        registry: &str,
        repository: &str,
        response_url: &Url,
        location: &HeaderValue,
        digest: &str,
    ) -> Result<Url, OciRegistryClientError> {
        let location = location
            .to_str()
            .ok()
            .filter(|value| {
                !value.is_empty() && value.len() <= 4096 && !value.contains(['\0', '\r', '\n'])
            })
            .ok_or_else(|| {
                OciRegistryClientError::Protocol(
                    "registry blob upload omitted a valid Location header".into(),
                )
            })?;
        let mut url = response_url.join(location).map_err(|_| {
            OciRegistryClientError::Protocol(
                "registry blob upload returned an invalid Location".into(),
            )
        })?;
        let origin = self.registry_root(registry)?;
        if url.scheme() != origin.scheme()
            || url.host_str() != origin.host_str()
            || url.port_or_known_default() != origin.port_or_known_default()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || path_has_unsafe_percent_encoding(url.path())
            || !url
                .path()
                .starts_with(&format!("/v2/{repository}/blobs/uploads/"))
            || url.query_pairs().any(|(name, _)| name == "digest")
        {
            return Err(OciRegistryClientError::Protocol(
                "registry blob upload Location escaped its registry repository".into(),
            ));
        }
        url.query_pairs_mut().append_pair("digest", digest);
        Ok(url)
    }

    pub(crate) async fn head_manifest(
        &self,
        url: Url,
        repository: &str,
        actions: &str,
        credential: Option<&RegistryCredentialMaterial>,
        accept: &str,
    ) -> Result<Response, OciRegistryClientError> {
        self.send(RegistryRequest {
            method: Method::HEAD,
            url,
            repository,
            actions,
            credential,
            accept: Some(accept),
            content_type: None,
            body: RequestBody::Empty,
        })
        .await
    }

    pub(crate) async fn head_blob(
        &self,
        url: Url,
        repository: &str,
        actions: &str,
        credential: Option<&RegistryCredentialMaterial>,
    ) -> Result<Response, OciRegistryClientError> {
        self.send(RegistryRequest {
            method: Method::HEAD,
            url,
            repository,
            actions,
            credential,
            accept: None,
            content_type: None,
            body: RequestBody::Empty,
        })
        .await
    }

    pub(crate) async fn start_blob_upload(
        &self,
        url: Url,
        repository: &str,
        credential: Option<&RegistryCredentialMaterial>,
    ) -> Result<Response, OciRegistryClientError> {
        self.send(RegistryRequest {
            method: Method::POST,
            url,
            repository,
            actions: "pull,push",
            credential,
            accept: None,
            content_type: None,
            body: RequestBody::Empty,
        })
        .await
    }

    pub(crate) async fn complete_blob_upload(
        &self,
        url: Url,
        repository: &str,
        credential: Option<&RegistryCredentialMaterial>,
        path: &Path,
        size: u64,
    ) -> Result<Response, OciRegistryClientError> {
        self.send(RegistryRequest {
            method: Method::PUT,
            url,
            repository,
            actions: "pull,push",
            credential,
            accept: None,
            content_type: Some("application/octet-stream"),
            body: RequestBody::File { path, size },
        })
        .await
    }

    pub(crate) async fn put_manifest(
        &self,
        url: Url,
        repository: &str,
        credential: Option<&RegistryCredentialMaterial>,
        media_type: &str,
        body: &[u8],
    ) -> Result<Response, OciRegistryClientError> {
        self.send(RegistryRequest {
            method: Method::PUT,
            url,
            repository,
            actions: "pull,push",
            credential,
            accept: None,
            content_type: Some(media_type),
            body: RequestBody::Bytes(body),
        })
        .await
    }

    fn repository_url(
        &self,
        registry: &str,
        repository: &str,
        suffix: &str,
    ) -> Result<Url, OciRegistryClientError> {
        if !valid_registry_host(registry)
            || repository.is_empty()
            || repository.contains(['?', '#', '\0', '\r', '\n'])
            || suffix.is_empty()
            || suffix.contains(['?', '#', '\0', '\r', '\n'])
        {
            return Err(OciRegistryClientError::Invalid(
                "registry URL components are invalid".into(),
            ));
        }
        self.registry_root(registry)?
            .join(&format!("v2/{repository}/{suffix}"))
            .map_err(|_| OciRegistryClientError::Invalid("registry URL is invalid".into()))
    }

    fn registry_root(&self, registry: &str) -> Result<Url, OciRegistryClientError> {
        if !valid_registry_host(registry) {
            return Err(OciRegistryClientError::Invalid(
                "registry host is invalid".into(),
            ));
        }
        let scheme = if self.insecure_hosts.contains(registry) {
            "http"
        } else {
            "https"
        };
        Url::parse(&format!("{scheme}://{registry}/")).map_err(|_| {
            OciRegistryClientError::Invalid("registry host cannot form an origin".into())
        })
    }

    async fn send(&self, request: RegistryRequest<'_>) -> Result<Response, OciRegistryClientError> {
        validate_actions(request.actions)?;
        let response = self.send_once(&request, None).await?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return Ok(response);
        }
        let challenge = response
            .headers()
            .get(WWW_AUTHENTICATE)
            .cloned()
            .ok_or(OciRegistryClientError::Unauthorized)?;
        let authorization = if authentication_scheme_is(&challenge, "Basic")? {
            RequestAuthorization::Basic(
                request
                    .credential
                    .ok_or(OciRegistryClientError::Unauthorized)?,
            )
        } else if authentication_scheme_is(&challenge, "Bearer")? {
            RequestAuthorization::Bearer(
                self.bearer_token(
                    &challenge,
                    request.repository,
                    request.actions,
                    request.credential,
                )
                .await?,
            )
        } else {
            return Err(OciRegistryClientError::Unauthorized);
        };
        self.send_once(&request, Some(&authorization)).await
    }

    async fn send_once(
        &self,
        request: &RegistryRequest<'_>,
        authorization: Option<&RequestAuthorization<'_>>,
    ) -> Result<Response, OciRegistryClientError> {
        let mut builder = self
            .client
            .request(request.method.clone(), request.url.clone());
        if let Some(accept) = request.accept {
            builder = builder.header(ACCEPT, accept);
        }
        if let Some(content_type) = request.content_type {
            builder = builder.header(CONTENT_TYPE, content_type);
        }
        if let Some(authorization) = authorization {
            builder = match authorization {
                RequestAuthorization::Basic(credential) => {
                    builder.basic_auth(credential.username(), Some(credential.password()))
                }
                RequestAuthorization::Bearer(token) => {
                    let value = Zeroizing::new(format!("Bearer {}", token.as_str()));
                    let mut header = HeaderValue::from_bytes(value.as_bytes()).map_err(|_| {
                        OciRegistryClientError::Protocol(
                            "registry returned an invalid bearer token".into(),
                        )
                    })?;
                    header.set_sensitive(true);
                    builder.header(AUTHORIZATION, header)
                }
            };
        }
        builder = match request.body {
            RequestBody::Empty => builder,
            RequestBody::Bytes(body) => builder
                .header(CONTENT_LENGTH, body.len())
                .body(body.to_vec()),
            RequestBody::File { path, size } => {
                let metadata = tokio::fs::symlink_metadata(path).await.map_err(|error| {
                    OciRegistryClientError::Storage(format!(
                        "could not inspect registry upload body: {error}"
                    ))
                })?;
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.len() != size
                {
                    return Err(OciRegistryClientError::Storage(
                        "registry upload body changed before transmission".into(),
                    ));
                }
                let file = tokio::fs::File::open(path).await.map_err(|error| {
                    OciRegistryClientError::Storage(format!(
                        "could not open registry upload body: {error}"
                    ))
                })?;
                builder
                    .header(CONTENT_LENGTH, size)
                    .body(reqwest::Body::wrap_stream(ReaderStream::new(file)))
            }
        };
        builder
            .send()
            .await
            .map_err(|error| OciRegistryClientError::Transport(error.to_string()))
    }

    pub(crate) async fn bearer_token(
        &self,
        challenge: &HeaderValue,
        repository: &str,
        actions: &str,
        credential: Option<&RegistryCredentialMaterial>,
    ) -> Result<Zeroizing<String>, OciRegistryClientError> {
        let challenge = challenge
            .to_str()
            .map_err(|_| OciRegistryClientError::Unauthorized)?;
        let parameters = parse_bearer_challenge(challenge)?;
        let realm = parameters
            .get("realm")
            .ok_or(OciRegistryClientError::Unauthorized)?;
        let mut url = Url::parse(realm).map_err(|_| OciRegistryClientError::Unauthorized)?;
        if !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || (url.scheme() != "https"
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
                        .unwrap_or(false)))
        {
            return Err(OciRegistryClientError::Unauthorized);
        }
        {
            let mut query = url.query_pairs_mut();
            if let Some(service) = parameters.get("service") {
                query.append_pair("service", service);
            }
            query.append_pair("scope", &format!("repository:{repository}:{actions}"));
        }
        let mut request = self.client.get(url);
        if let Some(credential) = credential {
            request = request.basic_auth(credential.username(), Some(credential.password()));
        }
        let response = request
            .send()
            .await
            .map_err(|error| OciRegistryClientError::Transport(error.to_string()))?;
        if !response.status().is_success() {
            return Err(OciRegistryClientError::Unauthorized);
        }
        if response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok())
            .is_some_and(|length| length > MAX_TOKEN_RESPONSE_BYTES)
        {
            return Err(OciRegistryClientError::Protocol(
                "registry token response exceeds the supported size".into(),
            ));
        }
        let mut body = Zeroizing::new(Vec::new());
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            let chunk =
                chunk.map_err(|error| OciRegistryClientError::Transport(error.to_string()))?;
            if chunk.len() > MAX_TOKEN_RESPONSE_BYTES - body.len() {
                return Err(OciRegistryClientError::Protocol(
                    "registry token response exceeds the supported size".into(),
                ));
            }
            body.extend_from_slice(&chunk);
        }
        let mut token: TokenResponse = serde_json::from_slice(&body).map_err(|_| {
            OciRegistryClientError::Protocol("registry token response is invalid".into())
        })?;
        let token = token
            .token
            .take()
            .or_else(|| token.access_token.take())
            .ok_or_else(|| {
                OciRegistryClientError::Protocol("registry token response omitted its token".into())
            })?;
        if token.is_empty() || token.len() > 64 * 1024 || token.contains(['\0', '\r', '\n']) {
            return Err(OciRegistryClientError::Protocol(
                "registry returned an invalid bearer token".into(),
            ));
        }
        Ok(Zeroizing::new(token))
    }
}

struct RegistryRequest<'a> {
    method: Method,
    url: Url,
    repository: &'a str,
    actions: &'a str,
    credential: Option<&'a RegistryCredentialMaterial>,
    accept: Option<&'a str>,
    content_type: Option<&'a str>,
    body: RequestBody<'a>,
}

#[derive(Clone, Copy)]
enum RequestBody<'a> {
    Empty,
    Bytes(&'a [u8]),
    File { path: &'a Path, size: u64 },
}

enum RequestAuthorization<'a> {
    Basic(&'a RegistryCredentialMaterial),
    Bearer(Zeroizing<String>),
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

fn validate_actions(actions: &str) -> Result<(), OciRegistryClientError> {
    if !matches!(actions, "pull" | "pull,push") {
        return Err(OciRegistryClientError::Invalid(
            "registry repository scope is invalid".into(),
        ));
    }
    Ok(())
}

fn parse_bearer_challenge(
    challenge: &str,
) -> Result<BTreeMap<String, String>, OciRegistryClientError> {
    let (scheme, parameters) = challenge
        .split_once(' ')
        .ok_or(OciRegistryClientError::Unauthorized)?;
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return Err(OciRegistryClientError::Unauthorized);
    }
    let mut result = BTreeMap::new();
    for parameter in parameters.split(',') {
        let (key, value) = parameter
            .trim()
            .split_once('=')
            .ok_or(OciRegistryClientError::Unauthorized)?;
        let value = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .ok_or(OciRegistryClientError::Unauthorized)?;
        if key.is_empty()
            || value.is_empty()
            || value.contains(['\0', '\r', '\n', '"'])
            || result.insert(key.to_owned(), value.to_owned()).is_some()
        {
            return Err(OciRegistryClientError::Unauthorized);
        }
    }
    Ok(result)
}

fn authentication_scheme_is(
    challenge: &HeaderValue,
    expected: &str,
) -> Result<bool, OciRegistryClientError> {
    let challenge = challenge
        .to_str()
        .map_err(|_| OciRegistryClientError::Unauthorized)?;
    Ok(challenge
        .split_ascii_whitespace()
        .next()
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case(expected)))
}

pub(crate) fn required_registry_header(
    headers: &HeaderMap,
    name: &str,
) -> Result<String, OciRegistryClientError> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty() && value.len() <= 4096 && !value.contains(['\0', '\r', '\n'])
        })
        .map(str::to_owned)
        .ok_or_else(|| {
            OciRegistryClientError::Protocol(format!(
                "registry response omitted a valid {name} header"
            ))
        })
}

fn valid_registry_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 255
        && !host.ends_with(':')
        && !host.contains(['/', '@', '\\', '\0', '\r', '\n', ' ', '\t'])
        && Url::parse(&format!("https://{host}/")).is_ok_and(|origin| {
            origin.host_str().is_some()
                && origin.path() == "/"
                && origin.query().is_none()
                && origin.fragment().is_none()
                && origin.username().is_empty()
                && origin.password().is_none()
        })
}

fn path_has_unsafe_percent_encoding(path: &str) -> bool {
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }
        let Some(high) = bytes.get(index + 1).and_then(|byte| hex_value(*byte)) else {
            return true;
        };
        let Some(low) = bytes.get(index + 2).and_then(|byte| hex_value(*byte)) else {
            return true;
        };
        let decoded = high * 16 + low;
        if decoded.is_ascii_control() || matches!(decoded, b'.' | b'/' | b'\\' | b'%') {
            return true;
        }
        index += 3;
    }
    false
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
