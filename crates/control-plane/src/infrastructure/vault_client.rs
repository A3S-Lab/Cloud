use reqwest::header::{HeaderMap, HeaderValue, ACCEPT};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use url::Url;

const MAX_ERROR_BODY: usize = 16 * 1024;
const MAX_RESPONSE_BODY: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct VaultClient {
    address: Url,
    client: reqwest::Client,
}

impl VaultClient {
    pub(crate) fn new(
        address: &str,
        token: &str,
        timeout: Duration,
    ) -> Result<Self, VaultClientError> {
        let mut address = Url::parse(address)
            .map_err(|error| VaultClientError::Configuration(error.to_string()))?;
        if address.scheme() != "https"
            || address.host_str().is_none()
            || !address.username().is_empty()
            || address.password().is_some()
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
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .build()
            .map_err(|error| VaultClientError::Configuration(error.to_string()))?;
        Ok(Self { address, client })
    }

    pub(crate) async fn post<Input, Output>(
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
            if is_transient_status(status) {
                return Err(VaultClientError::Unavailable(format!(
                    "Vault returned transient status {status}"
                )));
            }
            return Err(VaultClientError::Rejected(format!(
                "Vault returned {status}: {}",
                bounded_body(response).await
            )));
        }
        let bytes = bounded_response(response).await?;
        serde_json::from_slice::<VaultEnvelope<Output>>(&bytes)
            .map(|envelope| envelope.data)
            .map_err(|error| VaultClientError::Rejected(format!("invalid Vault response: {error}")))
    }

    pub(crate) async fn get<Output>(&self, path: &str) -> Result<Output, VaultClientError>
    where
        Output: DeserializeOwned,
    {
        validate_path(path)?;
        let response = self
            .client
            .get(self.endpoint(path)?)
            .send()
            .await
            .map_err(|error| VaultClientError::Unavailable(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            if is_transient_status(status) {
                return Err(VaultClientError::Unavailable(format!(
                    "Vault returned transient status {status}"
                )));
            }
            return Err(VaultClientError::Rejected(format!(
                "Vault returned {status}: {}",
                bounded_body(response).await
            )));
        }
        let bytes = bounded_response(response).await?;
        serde_json::from_slice::<VaultEnvelope<Output>>(&bytes)
            .map(|envelope| envelope.data)
            .map_err(|error| VaultClientError::Rejected(format!("invalid Vault response: {error}")))
    }

    pub(crate) async fn health(&self) -> Result<bool, VaultClientError> {
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
pub(crate) enum VaultClientError {
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

fn is_transient_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429) || status.is_server_error()
}

async fn bounded_body(response: reqwest::Response) -> String {
    let mut response = response;
    let mut bytes = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) if bytes.len().saturating_add(chunk.len()) <= MAX_ERROR_BODY => {
                bytes.extend_from_slice(&chunk);
            }
            Ok(Some(_)) => return "response body exceeded 16 KiB".into(),
            Ok(None) => return String::from_utf8_lossy(&bytes).into_owned(),
            Err(error) => return format!("could not read response body: {error}"),
        }
    }
}

async fn bounded_response(mut response: reqwest::Response) -> Result<Vec<u8>, VaultClientError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| VaultClientError::Unavailable(error.to_string()))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BODY {
            return Err(VaultClientError::Rejected(
                "Vault response body exceeded 2 MiB".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_only_timeout_rate_limit_and_server_statuses_as_transient() {
        assert!(!is_transient_status(reqwest::StatusCode::BAD_REQUEST));
        assert!(!is_transient_status(reqwest::StatusCode::FORBIDDEN));
        assert!(is_transient_status(reqwest::StatusCode::REQUEST_TIMEOUT));
        assert!(is_transient_status(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(is_transient_status(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR
        ));
        assert!(is_transient_status(reqwest::StatusCode::BAD_GATEWAY));
    }
}
