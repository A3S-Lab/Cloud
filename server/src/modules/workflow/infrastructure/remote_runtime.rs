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
        let parsed = url::Url::parse(&endpoint)
            .map_err(|error| RuntimeError::InvalidRequest(format!("invalid endpoint: {error}")))?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(RuntimeError::InvalidRequest(
                "Runtime endpoint must be an absolute http:// or https:// URL".to_string(),
            ));
        }
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use a3s_runtime::contract::{
        ArtifactRef, HealthCheckKind, IsolationLevel, MountKind, NetworkMode, ResourceControl,
        ResourceLimits, RestartPolicy, RuntimeExecRequest, RuntimeFeature, RuntimeLogStream,
        RuntimeNetworkSpec, RuntimeProcessSpec, RuntimeUnitClass, RuntimeUnitSpec,
        RuntimeUnitState,
    };
    use a3s_runtime::ProviderId;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    fn spec() -> RuntimeUnitSpec {
        RuntimeUnitSpec {
            schema: RuntimeUnitSpec::SCHEMA.to_string(),
            unit_id: "unit-1".to_string(),
            generation: 1,
            class: RuntimeUnitClass::Task,
            artifact: ArtifactRef {
                uri: "file:///runner".to_string(),
                digest: format!("sha256:{}", "a".repeat(64)),
                media_type: "application/vnd.a3s.workflow.node-runner.v1".to_string(),
            },
            process: RuntimeProcessSpec {
                command: vec!["/runner".to_string()],
                args: Vec::new(),
                working_directory: None,
                environment: BTreeMap::new(),
            },
            mounts: Vec::new(),
            secrets: Vec::new(),
            network: RuntimeNetworkSpec {
                mode: NetworkMode::None,
                ports: Vec::new(),
            },
            resources: ResourceLimits {
                cpu_millis: 100,
                memory_bytes: 1024,
                pids: 8,
                ephemeral_storage_bytes: None,
                execution_timeout_ms: Some(1_000),
            },
            isolation: IsolationLevel::Process,
            health: None,
            restart: RestartPolicy::Never,
            outputs: Vec::new(),
            semantics_profile_digest: None,
        }
    }

    fn observation(spec: &RuntimeUnitSpec) -> RuntimeObservation {
        RuntimeObservation {
            schema: RuntimeObservation::SCHEMA.to_string(),
            unit_id: spec.unit_id.clone(),
            generation: spec.generation,
            spec_digest: spec.digest().expect("spec digest"),
            class: spec.class,
            state: RuntimeUnitState::Accepted,
            provider_resource_id: None,
            provider_build: None,
            observed_at_ms: 1,
            started_at_ms: None,
            finished_at_ms: None,
            health: None,
            outputs: Vec::new(),
            usage: None,
            evidence: None,
            provider_attestation: None,
            failure: None,
        }
    }

    fn capabilities() -> RuntimeCapabilities {
        RuntimeCapabilities {
            schema: RuntimeCapabilities::SCHEMA.to_string(),
            provider_id: ProviderId::parse("test-provider").expect("provider ID"),
            provider_build: "test/1".to_string(),
            unit_classes: vec![RuntimeUnitClass::Task],
            artifact_media_types: vec!["application/vnd.a3s.workflow.node-runner.v1".to_string()],
            isolation_levels: vec![IsolationLevel::Process],
            network_modes: vec![NetworkMode::None],
            mount_kinds: vec![MountKind::Artifact],
            health_check_kinds: Vec::<HealthCheckKind>::new(),
            resource_controls: vec![
                ResourceControl::Cpu,
                ResourceControl::Memory,
                ResourceControl::Pids,
            ],
            features: vec![RuntimeFeature::DurableIdentity],
        }
    }

    async fn mock_server(
        responses: Vec<(u16, String)>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock Runtime");
        let address = listener.local_addr().expect("mock Runtime address");
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for (status, body) in responses {
                let (mut socket, _) = listener.accept().await.expect("accept Runtime request");
                let mut request = Vec::new();
                loop {
                    let mut buffer = [0_u8; 4096];
                    let read = socket
                        .read(&mut buffer)
                        .await
                        .expect("read Runtime request");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                    let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n")
                    else {
                        continue;
                    };
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if request.len() >= header_end + 4 + content_length {
                        break;
                    }
                }
                requests.push(String::from_utf8_lossy(&request).into_owned());
                let response = format!(
                    "HTTP/1.1 {status} Test\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("write Runtime response");
            }
            requests
        });
        (format!("http://{address}"), task)
    }

    #[test]
    fn endpoint_requires_an_absolute_http_url() {
        for invalid in [
            "not-a-url",
            "file:///tmp/runtime",
            "mailto:runtime@example.test",
        ] {
            assert!(matches!(
                RemoteRuntimeClient::new(invalid, ""),
                Err(RuntimeError::InvalidRequest(_))
            ));
        }
        RemoteRuntimeClient::new("https://runtime.example.test/", "")
            .expect("valid Runtime endpoint");
    }

    #[tokio::test]
    async fn forwards_every_runtime_operation_with_bearer_auth() {
        let spec = spec();
        let observation = observation(&spec);
        let inspection = RuntimeInspection::NotFound {
            schema: RuntimeInspection::SCHEMA.to_string(),
            unit_id: spec.unit_id.clone(),
            last_generation: Some(spec.generation),
        };
        let removal = RuntimeRemoval {
            schema: RuntimeRemoval::SCHEMA.to_string(),
            request_id: "remove-1".to_string(),
            unit_id: spec.unit_id.clone(),
            generation: spec.generation,
            removed_at_ms: 2,
            already_absent: false,
        };
        let chunk = RuntimeLogChunk {
            schema: RuntimeLogChunk::SCHEMA.to_string(),
            cursor: "cursor-1".to_string(),
            sequence: 1,
            observed_at_ms: 1,
            stream: RuntimeLogStream::Stdout,
            data: "ready".to_string(),
        };
        let exec_result = RuntimeExecResult {
            schema: RuntimeExecResult::SCHEMA.to_string(),
            request_id: "exec-1".to_string(),
            observation: observation.clone(),
            exit_code: 0,
            stdout: "ok".to_string(),
            stderr: String::new(),
            truncated: false,
        };
        let responses = [
            serde_json::to_string(&capabilities()).expect("capabilities JSON"),
            serde_json::to_string(&observation).expect("observation JSON"),
            serde_json::to_string(&inspection).expect("inspection JSON"),
            serde_json::to_string(&inspection).expect("stop JSON"),
            serde_json::to_string(&removal).expect("removal JSON"),
            serde_json::to_string(&vec![chunk.clone()]).expect("logs JSON"),
            serde_json::to_string(&exec_result).expect("exec JSON"),
        ]
        .into_iter()
        .map(|body| (200, body))
        .collect();
        let (endpoint, task) = mock_server(responses).await;
        let client = RemoteRuntimeClient::new(endpoint, "secret").expect("Runtime client");

        assert_eq!(
            client.capabilities().await.expect("capabilities"),
            capabilities()
        );
        let apply = RuntimeApplyRequest {
            schema: RuntimeApplyRequest::SCHEMA.to_string(),
            request_id: "apply-1".to_string(),
            deadline_at_ms: None,
            spec: spec.clone(),
        };
        assert_eq!(client.apply(&apply).await.expect("apply"), observation);
        assert_eq!(
            client.inspect(&spec.unit_id).await.expect("inspect"),
            inspection
        );
        let action = RuntimeActionRequest {
            schema: RuntimeActionRequest::SCHEMA.to_string(),
            request_id: "remove-1".to_string(),
            unit_id: spec.unit_id.clone(),
            generation: spec.generation,
            deadline_at_ms: None,
        };
        assert_eq!(client.stop(&action).await.expect("stop"), inspection);
        assert_eq!(client.remove(&action).await.expect("remove"), removal);
        let query = RuntimeLogQuery {
            schema: RuntimeLogQuery::SCHEMA.to_string(),
            unit_id: spec.unit_id.clone(),
            generation: spec.generation,
            cursor: None,
            limit: 10,
            stream: None,
        };
        assert_eq!(client.logs(&query).await.expect("logs"), vec![chunk]);
        let exec = RuntimeExecRequest {
            schema: RuntimeExecRequest::SCHEMA.to_string(),
            request_id: "exec-1".to_string(),
            unit_id: spec.unit_id,
            generation: spec.generation,
            command: vec!["true".to_string()],
            timeout_ms: 1_000,
            deadline_at_ms: None,
        };
        assert_eq!(client.exec(&exec).await.expect("exec"), exec_result);

        let requests = task.await.expect("mock Runtime task");
        let expected_paths = [
            "GET /v1/capabilities ",
            "POST /v1/units/apply ",
            "POST /v1/units/inspect ",
            "POST /v1/units/stop ",
            "POST /v1/units/remove ",
            "POST /v1/units/logs ",
            "POST /v1/units/exec ",
        ];
        assert_eq!(requests.len(), expected_paths.len());
        for (request, expected_path) in requests.iter().zip(expected_paths) {
            assert!(
                request.starts_with(expected_path),
                "unexpected request: {request}"
            );
            assert!(request
                .to_ascii_lowercase()
                .contains("authorization: bearer secret"));
        }
        assert!(
            requests[2].contains(r#""unitId":"unit-1""#),
            "inspect request did not preserve the unit ID: {}",
            requests[2]
        );
    }

    #[tokio::test]
    async fn maps_provider_errors_and_invalid_json_without_losing_context() {
        let (endpoint, task) = mock_server(vec![
            (503, json!({ "message": "pool unavailable" }).to_string()),
            (500, "plain failure".to_string()),
            (200, "not-json".to_string()),
        ])
        .await;
        let client = RemoteRuntimeClient::new(endpoint, "").expect("Runtime client");

        let error = client
            .capabilities()
            .await
            .expect_err("JSON provider error");
        assert!(matches!(error, RuntimeError::ProviderUnavailable(_)));
        assert!(error.to_string().contains("pool unavailable"));
        let error = client
            .capabilities()
            .await
            .expect_err("plain provider error");
        assert!(error.to_string().contains("plain failure"));
        let error = client
            .capabilities()
            .await
            .expect_err("invalid JSON response");
        assert!(matches!(error, RuntimeError::Protocol(_)));
        assert!(error.to_string().contains("invalid provider response JSON"));

        let requests = task.await.expect("mock Runtime task");
        assert!(requests
            .iter()
            .all(|request| !request.to_ascii_lowercase().contains("authorization:")));
    }
}
