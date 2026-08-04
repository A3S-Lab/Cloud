use a3s_cloud_contracts::{
    AgentProtocolCommandReceiptV1, AgentProtocolCommandV1, NodeCodeAgentRuntimeBindingV1,
    AGENT_PROTOCOL_COMMAND_HTTP_PATH_V1,
};
use a3s_runtime::contract::{
    RuntimeInspection, RuntimeServiceEndpoint, RuntimeUnitClass, RuntimeUnitState,
    TransportProtocol,
};
use a3s_runtime::RuntimeClient;
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

const MAXIMUM_RECEIPT_BYTES: usize = 64 * 1024;

/// Transport-only adapter for the Harness served by the existing `a3s code`
/// process. It deliberately owns no Agent lifecycle or execution state.
#[async_trait]
pub(crate) trait CodeHarnessTransport: Send + Sync {
    async fn send_command(
        &self,
        endpoint: &RuntimeServiceEndpoint,
        command: &AgentProtocolCommandV1,
        timeout: Duration,
    ) -> Result<AgentProtocolCommandReceiptV1, CodeHarnessError>;
}

pub(crate) struct HttpCodeHarnessTransport {
    client: Client,
}

impl HttpCodeHarnessTransport {
    pub(crate) fn new() -> Result<Self, CodeHarnessError> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .user_agent(format!("a3s-cloud-node/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| CodeHarnessError::Invalid(error.to_string()))?;
        Ok(Self { client })
    }
}

#[async_trait]
impl CodeHarnessTransport for HttpCodeHarnessTransport {
    async fn send_command(
        &self,
        endpoint: &RuntimeServiceEndpoint,
        command: &AgentProtocolCommandV1,
        timeout: Duration,
    ) -> Result<AgentProtocolCommandReceiptV1, CodeHarnessError> {
        endpoint.validate().map_err(CodeHarnessError::Invalid)?;
        command
            .validate()
            .map_err(|error| CodeHarnessError::Invalid(error.code().into()))?;
        if endpoint.protocol != TransportProtocol::Tcp || timeout.is_zero() {
            return Err(CodeHarnessError::Invalid(
                "A3S Code Harness requires a TCP endpoint and positive timeout".into(),
            ));
        }
        let base_url = Url::parse(&format!("http://{}/", endpoint.socket_addr()))
            .map_err(|error| CodeHarnessError::Invalid(error.to_string()))?;
        let url = base_url
            .join(AGENT_PROTOCOL_COMMAND_HTTP_PATH_V1.trim_start_matches('/'))
            .map_err(|error| CodeHarnessError::Invalid(error.to_string()))?;
        let mut response = self
            .client
            .post(url)
            .timeout(timeout)
            .json(command)
            .send()
            .await
            .map_err(|error| CodeHarnessError::Transport(error.to_string()))?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > MAXIMUM_RECEIPT_BYTES as u64)
        {
            return Err(CodeHarnessError::Protocol(
                "A3S Code Harness response exceeds its protocol bound".into(),
            ));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| CodeHarnessError::Transport(error.to_string()))?
        {
            let next = body
                .len()
                .checked_add(chunk.len())
                .ok_or_else(|| CodeHarnessError::Protocol("response size overflowed".into()))?;
            if next > MAXIMUM_RECEIPT_BYTES {
                return Err(CodeHarnessError::Protocol(
                    "A3S Code Harness response exceeds its protocol bound".into(),
                ));
            }
            body.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            return Err(CodeHarnessError::Rejected { status });
        }
        let receipt: AgentProtocolCommandReceiptV1 = serde_json::from_slice(&body)
            .map_err(|error| CodeHarnessError::Protocol(error.to_string()))?;
        receipt
            .validate_for(command)
            .map_err(|error| CodeHarnessError::Protocol(error.code().into()))?;
        Ok(receipt)
    }
}

pub(crate) async fn resolve_runtime_endpoint(
    runtime: &dyn RuntimeClient,
    binding: &NodeCodeAgentRuntimeBindingV1,
) -> Result<RuntimeServiceEndpoint, CodeHarnessError> {
    binding.validate().map_err(CodeHarnessError::Invalid)?;
    let inspection = runtime
        .inspect(&binding.runtime_unit_id)
        .await
        .map_err(CodeHarnessError::Runtime)?;
    let observation = match inspection {
        RuntimeInspection::Found { observation, .. } => observation,
        RuntimeInspection::NotFound { .. } => {
            return Err(CodeHarnessError::Unavailable(format!(
                "Runtime unit {:?} is not available",
                binding.runtime_unit_id
            )));
        }
    };
    if observation.unit_id != binding.runtime_unit_id
        || observation.generation != binding.runtime_generation
        || observation.spec_digest != binding.runtime_spec_digest
        || observation.class != RuntimeUnitClass::Service
    {
        return Err(CodeHarnessError::Invalid(
            "Runtime observation does not match the Code Harness binding".into(),
        ));
    }
    if observation.state != RuntimeUnitState::Running {
        return Err(CodeHarnessError::Unavailable(format!(
            "Runtime Service {:?} is not running",
            binding.runtime_unit_id
        )));
    }
    let endpoint =
        RuntimeServiceEndpoint::from_observation(&observation, &binding.service_port_name)
            .map_err(CodeHarnessError::Invalid)?;
    if endpoint.protocol != TransportProtocol::Tcp {
        return Err(CodeHarnessError::Invalid(
            "A3S Code Harness Runtime port is not TCP".into(),
        ));
    }
    Ok(endpoint)
}

pub(crate) type SharedCodeHarnessTransport = Arc<dyn CodeHarnessTransport>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum CodeHarnessError {
    #[error("invalid A3S Code Harness binding or protocol: {0}")]
    Invalid(String),
    #[error("A3S Code Harness Runtime is unavailable: {0}")]
    Unavailable(String),
    #[error("A3S Code Harness Runtime inspection failed: {0}")]
    Runtime(a3s_runtime::RuntimeError),
    #[error("A3S Code Harness transport failed: {0}")]
    Transport(String),
    #[error("A3S Code Harness rejected the command with HTTP {status}")]
    Rejected { status: StatusCode },
    #[error("A3S Code Harness returned an invalid protocol response: {0}")]
    Protocol(String),
}

impl CodeHarnessError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "invalid_code_harness_binding",
            Self::Unavailable(_) => "code_harness_unavailable",
            Self::Runtime(_) => "code_harness_runtime",
            Self::Transport(_) => "code_harness_transport",
            Self::Rejected { .. } => "code_harness_rejected",
            Self::Protocol(_) => "code_harness_protocol",
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
