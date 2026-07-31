use std::time::Duration;

use a3s_runtime::contract::{
    RuntimeActionRequest, RuntimeApplyRequest, RuntimeCapabilities, RuntimeExecRequest,
    RuntimeExecResult, RuntimeInspection, RuntimeLogChunk, RuntimeLogQuery, RuntimeObservation,
    RuntimeRemoval,
};
use a3s_runtime::{RuntimeClient, RuntimeError, RuntimeResult};
use async_trait::async_trait;
use reqwest::{Client, RequestBuilder};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct RemoteRuntimeClient {
    endpoint: String,
    token: String,
    client: Client,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InspectRequest<'a> {
    unit_id: &'a str,
}

#[derive(Debug, Deserialize)]
struct RuntimeErrorBody {
    message: Option<String>,
}

impl RemoteRuntimeClient {
    pub fn new(endpoint: impl Into<String>, token: impl Into<String>) -> RuntimeResult<Self> {
        let endpoint = endpoint.into().trim_end_matches('/').to_string();
        url::Url::parse(&endpoint)
            .map_err(|error| RuntimeError::InvalidRequest(format!("invalid endpoint: {error}")))?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| RuntimeError::Transport(error.to_string()))?;
        Ok(Self {
            endpoint,
            token: token.into(),
            client,
        })
    }

    fn authenticated(&self, request: RequestBuilder) -> RequestBuilder {
        if self.token.is_empty() {
            request
        } else {
            request.bearer_auth(&self.token)
        }
    }

    async fn decode<T>(&self, request: RequestBuilder) -> RuntimeResult<T>
    where
        T: DeserializeOwned,
    {
        let response = self
            .authenticated(request)
            .send()
            .await
            .map_err(|error| RuntimeError::Transport(error.to_string()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| RuntimeError::Transport(error.to_string()))?;
        if !status.is_success() {
            let message = serde_json::from_slice::<RuntimeErrorBody>(&bytes)
                .ok()
                .and_then(|body| body.message)
                .unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned());
            return Err(RuntimeError::ProviderUnavailable(format!(
                "provider returned {status}: {message}"
            )));
        }
        serde_json::from_slice(&bytes).map_err(|error| {
            RuntimeError::Protocol(format!("invalid provider response JSON: {error}"))
        })
    }

    async fn post<I, O>(&self, path: &str, input: &I) -> RuntimeResult<O>
    where
        I: Serialize + ?Sized,
        O: DeserializeOwned,
    {
        self.decode(
            self.client
                .post(format!("{}{path}", self.endpoint))
                .json(input),
        )
        .await
    }
}

#[async_trait]
impl RuntimeClient for RemoteRuntimeClient {
    async fn capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
        self.decode(
            self.client
                .get(format!("{}/v1/capabilities", self.endpoint)),
        )
        .await
    }

    async fn apply(&self, request: &RuntimeApplyRequest) -> RuntimeResult<RuntimeObservation> {
        self.post("/v1/units/apply", request).await
    }

    async fn inspect(&self, unit_id: &str) -> RuntimeResult<RuntimeInspection> {
        self.post("/v1/units/inspect", &InspectRequest { unit_id })
            .await
    }

    async fn stop(&self, request: &RuntimeActionRequest) -> RuntimeResult<RuntimeInspection> {
        self.post("/v1/units/stop", request).await
    }

    async fn remove(&self, request: &RuntimeActionRequest) -> RuntimeResult<RuntimeRemoval> {
        self.post("/v1/units/remove", request).await
    }

    async fn logs(&self, query: &RuntimeLogQuery) -> RuntimeResult<Vec<RuntimeLogChunk>> {
        self.post("/v1/units/logs", query).await
    }

    async fn exec(&self, request: &RuntimeExecRequest) -> RuntimeResult<RuntimeExecResult> {
        self.post("/v1/units/exec", request).await
    }
}
