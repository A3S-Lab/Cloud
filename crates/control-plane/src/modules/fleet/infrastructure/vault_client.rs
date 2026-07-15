use reqwest::header::{HeaderMap, HeaderValue, ACCEPT};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use url::Url;

const MAX_ERROR_BODY: usize = 16 * 1024;

#[derive(Clone)]
pub(super) struct VaultClient {
    address: Url,
    client: reqwest::Client,
}

impl VaultClient {
    pub(super) fn new(
        address: &str,
        token: &str,
        timeout: Duration,
    ) -> Result<Self, VaultClientError> {
        let mut address = Url::parse(address)
            .map_err(|error| VaultClientError::Configuration(error.to_string()))?;
        if address.scheme() != "https"
            || address.host_str().is_none()
            || address.query().is_some()
            || address.fragment().is_some()
        {
            return Err(VaultClientError::Configuration(
                "Vault address must be an absolute HTTPS origin".into(),
            ));
        }
        if !address.path().ends_with('/') {
            let path = format!("{}/", address.path());
            address.set_path(&path);
        }
        if token.is_empty() || token.len() > 8192 || token.contains(['\0', '\r', '\n']) {
            return Err(VaultClientError::Configuration(
                "Vault token is invalid".into(),
            ));
        }
        if timeout.is_zero() {
            return Err(VaultClientError::Configuration(
                "Vault timeout must be positive".into(),
            ));
        }
        let mut token = HeaderValue::from_str(token)
            .map_err(|error| VaultClientError::Configuration(error.to_string()))?;
        token.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert("x-vault-token", token);
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .https_only(true)
            .timeout(timeout)
            .build()
            .map_err(|error| VaultClientError::Configuration(error.to_string()))?;
        Ok(Self { address, client })
    }

    pub(super) async fn post<Input, Output>(
        &self,
        path: &str,
        input: &Input,
    ) -> Result<Output, VaultClientError>
    where
        Input: Serialize + Sync,
        Output: DeserializeOwned,
    {
        validate_path(path)?;
        let response = self
            .client
            .post(self.endpoint(path)?)
            .json(input)
            .send()
            .await
            .map_err(|error| VaultClientError::Unavailable(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(VaultClientError::Rejected(format!(
                "Vault returned {status}: {}",
                bounded_body(response).await
            )));
        }
        response
            .json::<VaultEnvelope<Output>>()
            .await
            .map(|envelope| envelope.data)
            .map_err(|error| VaultClientError::Rejected(format!("invalid Vault response: {error}")))
    }

    pub(super) async fn health(&self) -> Result<bool, VaultClientError> {
        let response = self
            .client
            .get(self.endpoint("sys/health")?)
            .send()
            .await
            .map_err(|error| VaultClientError::Unavailable(error.to_string()))?;
        Ok(matches!(response.status().as_u16(), 200 | 429 | 472 | 473))
    }

    fn endpoint(&self, path: &str) -> Result<Url, VaultClientError> {
        self.address
            .join(&format!("v1/{path}"))
            .map_err(|error| VaultClientError::Configuration(error.to_string()))
    }
}

#[derive(Debug, Deserialize)]
struct VaultEnvelope<T> {
    data: T,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum VaultClientError {
    #[error("invalid Vault configuration: {0}")]
    Configuration(String),
    #[error("Vault rejected the request: {0}")]
    Rejected(String),
    #[error("Vault is unavailable: {0}")]
    Unavailable(String),
}

fn validate_path(path: &str) -> Result<(), VaultClientError> {
    if path.is_empty()
        || path.len() > 1024
        || path.starts_with('/')
        || path.ends_with('/')
        || path.split('/').any(|segment| {
            segment.is_empty()
                || segment == "."
                || segment == ".."
                || segment.bytes().any(|byte| {
                    !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
                })
        })
    {
        return Err(VaultClientError::Configuration(
            "Vault API path is invalid".into(),
        ));
    }
    Ok(())
}

async fn bounded_body(response: reqwest::Response) -> String {
    match response.bytes().await {
        Ok(bytes) if bytes.len() <= MAX_ERROR_BODY => String::from_utf8_lossy(&bytes).into_owned(),
        Ok(_) => "response body exceeded 16 KiB".into(),
        Err(error) => format!("could not read response body: {error}"),
    }
}
