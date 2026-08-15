use a3s_cloud_contracts::NodeDurableCellOperatorBindingV1;
use a3s_runtime::contract::{
    RuntimeHealthState, RuntimeInspection, RuntimeServiceEndpoint, RuntimeUnitClass,
    RuntimeUnitState, TransportProtocol,
};
use a3s_runtime::RuntimeClient;
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use url::Url;
use zeroize::Zeroizing;

const MAXIMUM_OPERATOR_STATE_BYTES: usize = 64 * 1024;

/// Sanitized subset of celld's alpha `/state` response. Cell names, dynamic
/// phase labels, ownership-provider names, memory values, and raw bytes never
/// leave this transport adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DurableCellOperatorCounters {
    pub occupied: u32,
    pub evicting: u32,
    pub restoring: u32,
    pub activating: u32,
    pub activation_waiting: u32,
    pub capacity_waiting: u32,
}

#[derive(Deserialize)]
struct ProviderStateSnapshot {
    occupied: u32,
    evicting: u32,
    restoring: u32,
    activating: u32,
    activation_waiting: u32,
    capacity_waiting: u32,
}

impl From<ProviderStateSnapshot> for DurableCellOperatorCounters {
    fn from(snapshot: ProviderStateSnapshot) -> Self {
        Self {
            occupied: snapshot.occupied,
            evicting: snapshot.evicting,
            restoring: snapshot.restoring,
            activating: snapshot.activating,
            activation_waiting: snapshot.activation_waiting,
            capacity_waiting: snapshot.capacity_waiting,
        }
    }
}

/// Transport-only adapter for a reviewed Durable Cell provider's node-local
/// operator endpoint. Fleet's existing command journal owns delivery/replay;
/// this adapter owns no provider or Runtime lifecycle.
#[async_trait]
pub(crate) trait DurableCellOperatorTransport: Send + Sync {
    async fn observe(
        &self,
        endpoint: &RuntimeServiceEndpoint,
        timeout: Duration,
    ) -> Result<DurableCellOperatorCounters, DurableCellOperatorError>;
}

pub(crate) struct HttpDurableCellOperatorTransport {
    client: Client,
}

impl HttpDurableCellOperatorTransport {
    pub(crate) fn new() -> Result<Self, DurableCellOperatorError> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .user_agent(format!("a3s-cloud-node/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| DurableCellOperatorError::Invalid(error.to_string()))?;
        Ok(Self { client })
    }
}

#[async_trait]
impl DurableCellOperatorTransport for HttpDurableCellOperatorTransport {
    async fn observe(
        &self,
        endpoint: &RuntimeServiceEndpoint,
        timeout: Duration,
    ) -> Result<DurableCellOperatorCounters, DurableCellOperatorError> {
        endpoint
            .validate()
            .map_err(DurableCellOperatorError::Invalid)?;
        if endpoint.protocol != TransportProtocol::Tcp || timeout.is_zero() {
            return Err(DurableCellOperatorError::Invalid(
                "Durable Cell operator observation requires a node-local TCP endpoint and positive timeout"
                    .into(),
            ));
        }
        let base_url = Url::parse(&format!("http://{}/", endpoint.socket_addr()))
            .map_err(|error| DurableCellOperatorError::Invalid(error.to_string()))?;
        let url = base_url
            .join("state")
            .map_err(|error| DurableCellOperatorError::Invalid(error.to_string()))?;
        let mut response = self
            .client
            .get(url)
            .timeout(timeout)
            .send()
            .await
            .map_err(|error| DurableCellOperatorError::Transport(error.to_string()))?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > MAXIMUM_OPERATOR_STATE_BYTES as u64)
        {
            return Err(DurableCellOperatorError::Protocol(
                "Durable Cell operator state exceeds its protocol bound".into(),
            ));
        }
        // celld's provider-native response can contain tenant Cell names in
        // fields this adapter ignores. Keep the bounded raw buffer scoped here
        // and erase it as soon as the anonymous counters are parsed.
        let mut body = Zeroizing::new(Vec::new());
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| DurableCellOperatorError::Transport(error.to_string()))?
        {
            let next = body.len().checked_add(chunk.len()).ok_or_else(|| {
                DurableCellOperatorError::Protocol("response size overflowed".into())
            })?;
            if next > MAXIMUM_OPERATOR_STATE_BYTES {
                return Err(DurableCellOperatorError::Protocol(
                    "Durable Cell operator state exceeds its protocol bound".into(),
                ));
            }
            body.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            return Err(DurableCellOperatorError::Rejected { status });
        }
        parse_provider_state(body.as_slice())
    }
}

fn parse_provider_state(
    body: &[u8],
) -> Result<DurableCellOperatorCounters, DurableCellOperatorError> {
    let snapshot: ProviderStateSnapshot = serde_json::from_slice(body)
        .map_err(|error| DurableCellOperatorError::Protocol(error.to_string()))?;
    Ok(snapshot.into())
}

pub(crate) async fn resolve_runtime_endpoint(
    runtime: &dyn RuntimeClient,
    binding: &NodeDurableCellOperatorBindingV1,
) -> Result<RuntimeServiceEndpoint, DurableCellOperatorError> {
    binding
        .validate()
        .map_err(DurableCellOperatorError::Invalid)?;
    let inspection = runtime
        .inspect(&binding.runtime_unit_id)
        .await
        .map_err(DurableCellOperatorError::Runtime)?;
    let observation = match inspection {
        RuntimeInspection::Found { observation, .. } => observation,
        RuntimeInspection::NotFound { .. } => {
            return Err(DurableCellOperatorError::Unavailable(format!(
                "Runtime unit {:?} is not available",
                binding.runtime_unit_id
            )));
        }
    };
    if observation.unit_id != binding.runtime_unit_id
        || observation.generation != binding.runtime_generation
        || observation.spec_digest != binding.runtime_spec_digest
        || observation.class != RuntimeUnitClass::Service
        || observation.evidence.as_ref().is_none_or(|evidence| {
            evidence.semantics_profile_digest.as_deref()
                != Some(binding.service_profile_digest.as_str())
        })
    {
        return Err(DurableCellOperatorError::Invalid(
            "Runtime observation does not match the Durable Cell operator binding".into(),
        ));
    }
    if observation.state != RuntimeUnitState::Running
        || observation
            .health
            .as_ref()
            .is_none_or(|health| health.state != RuntimeHealthState::Healthy)
    {
        return Err(DurableCellOperatorError::Unavailable(format!(
            "Durable Cell Runtime Service {:?} is not healthy and running",
            binding.runtime_unit_id
        )));
    }
    let endpoint =
        RuntimeServiceEndpoint::from_observation(&observation, &binding.internal_service_port_name)
            .map_err(DurableCellOperatorError::Invalid)?;
    if endpoint.protocol != TransportProtocol::Tcp {
        return Err(DurableCellOperatorError::Invalid(
            "Durable Cell operator Runtime port is not TCP".into(),
        ));
    }
    Ok(endpoint)
}

pub(crate) type SharedDurableCellOperatorTransport = Arc<dyn DurableCellOperatorTransport>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum DurableCellOperatorError {
    #[error("invalid Durable Cell operator binding or protocol: {0}")]
    Invalid(String),
    #[error("Durable Cell Runtime is unavailable: {0}")]
    Unavailable(String),
    #[error("Durable Cell Runtime inspection failed: {0}")]
    Runtime(a3s_runtime::RuntimeError),
    #[error("Durable Cell operator transport failed: {0}")]
    Transport(String),
    #[error("Durable Cell operator rejected observation with HTTP {status}")]
    Rejected { status: StatusCode },
    #[error("Durable Cell operator returned an invalid state response: {0}")]
    Protocol(String),
}

impl DurableCellOperatorError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "invalid_durable_cell_operator_binding",
            Self::Unavailable(_) => "durable_cell_runtime_unavailable",
            Self::Runtime(_) => "durable_cell_runtime_inspection",
            Self::Transport(_) => "durable_cell_operator_transport",
            Self::Rejected { .. } => "durable_cell_operator_rejected",
            Self::Protocol(_) => "durable_cell_operator_protocol",
        }
    }

    pub(crate) fn retryable(&self) -> bool {
        match self {
            Self::Unavailable(_) | Self::Transport(_) => true,
            Self::Runtime(error) => matches!(
                error,
                a3s_runtime::RuntimeError::ProviderUnavailable(_)
                    | a3s_runtime::RuntimeError::Transport(_)
            ),
            Self::Rejected { status } => {
                status.is_server_error()
                    || matches!(
                        *status,
                        StatusCode::REQUEST_TIMEOUT
                            | StatusCode::TOO_EARLY
                            | StatusCode::TOO_MANY_REQUESTS
                    )
            }
            Self::Invalid(_) | Self::Protocol(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn provider_state_is_reduced_to_bounded_anonymous_counters() {
        let counters = parse_provider_state(
            br#"{
                "ownership":"bucket",
                "occupied":3,
                "evicting":1,
                "restoring":2,
                "activating":4,
                "activation_waiting":5,
                "capacity_waiting":6,
                "phases":{"Resident":3},
                "residents":["tenant-secret-cell"],
                "published":["tenant-secret-cell"],
                "rss_bytes":1024
            }"#,
        )
        .expect("provider state");
        assert_eq!(
            counters,
            DurableCellOperatorCounters {
                occupied: 3,
                evicting: 1,
                restoring: 2,
                activating: 4,
                activation_waiting: 5,
                capacity_waiting: 6,
            }
        );
    }

    #[test]
    fn provider_state_rejects_missing_or_unbounded_counters() {
        assert!(parse_provider_state(br#"{"occupied":1}"#).is_err());
        assert!(parse_provider_state(
            br#"{"occupied":4294967296,"evicting":0,"restoring":0,"activating":0,"activation_waiting":0,"capacity_waiting":0}"#
        )
        .is_err());
    }

    #[tokio::test]
    async fn http_adapter_reads_only_node_local_state_without_redirects() {
        let body = br#"{"ownership":"bucket","occupied":3,"evicting":1,"restoring":2,"activating":4,"activation_waiting":5,"capacity_waiting":6,"residents":["tenant-secret-cell"],"published":["tenant-secret-cell"]}"#;
        let (endpoint, server) = serve_once(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .into_bytes()
            .into_iter()
            .chain(body.iter().copied())
            .collect(),
        )
        .await;
        let transport = HttpDurableCellOperatorTransport::new().expect("HTTP transport");
        let counters = transport
            .observe(&endpoint, Duration::from_secs(2))
            .await
            .expect("operator state");
        assert_eq!(counters.occupied, 3);
        assert_eq!(counters.capacity_waiting, 6);
        server.await.expect("fixture server");

        let (redirect_endpoint, redirect_server) = serve_once(
            b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/state\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec(),
        )
        .await;
        assert!(matches!(
            transport
                .observe(&redirect_endpoint, Duration::from_secs(2))
                .await,
            Err(DurableCellOperatorError::Rejected {
                status: StatusCode::FOUND
            })
        ));
        redirect_server.await.expect("redirect fixture server");
    }

    #[tokio::test]
    async fn http_adapter_rejects_operator_state_above_the_wire_bound() {
        let (endpoint, server) = serve_once(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                MAXIMUM_OPERATOR_STATE_BYTES + 1
            )
            .into_bytes(),
        )
        .await;
        let transport = HttpDurableCellOperatorTransport::new().expect("HTTP transport");
        assert!(matches!(
            transport.observe(&endpoint, Duration::from_secs(2)).await,
            Err(DurableCellOperatorError::Protocol(_))
        ));
        server.await.expect("oversized fixture server");
    }

    async fn serve_once(
        response: Vec<u8>,
    ) -> (RuntimeServiceEndpoint, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fixture server");
        let port = listener.local_addr().expect("fixture address").port();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept fixture request");
            let mut request = vec![0; 4096];
            let read = stream
                .read(&mut request)
                .await
                .expect("read fixture request");
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(request.starts_with("GET /state HTTP/1.1\r\n"));
            stream
                .write_all(&response)
                .await
                .expect("write fixture response");
        });
        (
            RuntimeServiceEndpoint::node_local_tcp("cell-internal", port)
                .expect("fixture endpoint"),
            server,
        )
    }
}
