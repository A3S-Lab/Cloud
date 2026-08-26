use a3s_cloud_contracts::{
    AgentProviderCommandReceiptV1, AgentProviderCommandV1, AgentProviderEventPageRequestV1,
    AgentProviderEventPageV1, NodeAgentProviderRuntimeBindingV1,
    AGENT_PROVIDER_COMMAND_HTTP_PATH_V1, AGENT_PROVIDER_EVENT_PAGE_HTTP_PATH_V1,
    AGENT_PROVIDER_MAX_COMMAND_RECEIPT_BYTES, AGENT_PROVIDER_MAX_EVENT_PAGE_BYTES,
};
use a3s_runtime::contract::{
    RuntimeInspection, RuntimeServiceEndpoint, RuntimeUnitClass, RuntimeUnitState,
    TransportProtocol,
};
use a3s_runtime::RuntimeClient;
use async_trait::async_trait;
use reqwest::{Client, Response, StatusCode};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

const REFERENCE_ECHO_PROVIDER_PROFILE_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/a1.3/reference-echo-provider-profile.acl"
));

/// Transport-only adapter for a provider that speaks the common Agent
/// provider protocol. It owns no execution, retry, or event lifecycle state.
#[async_trait]
pub(crate) trait AgentProviderHarnessTransport: Send + Sync {
    async fn send_command(
        &self,
        endpoint: &RuntimeServiceEndpoint,
        binding: &NodeAgentProviderRuntimeBindingV1,
        command: &AgentProviderCommandV1,
        timeout: Duration,
    ) -> Result<AgentProviderCommandReceiptV1, AgentProviderHarnessError>;

    async fn event_page(
        &self,
        endpoint: &RuntimeServiceEndpoint,
        binding: &NodeAgentProviderRuntimeBindingV1,
        request: &AgentProviderEventPageRequestV1,
        timeout: Duration,
    ) -> Result<AgentProviderEventPageV1, AgentProviderHarnessError>;
}

pub(crate) struct HttpAgentProviderHarnessTransport {
    client: Client,
}

impl HttpAgentProviderHarnessTransport {
    pub(crate) fn new() -> Result<Self, AgentProviderHarnessError> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .user_agent(format!("a3s-cloud-node/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| AgentProviderHarnessError::Invalid(error.to_string()))?;
        Ok(Self { client })
    }

    fn endpoint_url(
        endpoint: &RuntimeServiceEndpoint,
        path: &str,
        timeout: Duration,
    ) -> Result<Url, AgentProviderHarnessError> {
        endpoint
            .validate()
            .map_err(AgentProviderHarnessError::Invalid)?;
        if endpoint.protocol != TransportProtocol::Tcp || timeout.is_zero() {
            return Err(AgentProviderHarnessError::Invalid(
                "Agent provider Harness requires a TCP endpoint and positive timeout".into(),
            ));
        }
        Url::parse(&format!("http://{}/", endpoint.socket_addr()))
            .map_err(|error| AgentProviderHarnessError::Invalid(error.to_string()))?
            .join(path.trim_start_matches('/'))
            .map_err(|error| AgentProviderHarnessError::Invalid(error.to_string()))
    }
}

#[async_trait]
impl AgentProviderHarnessTransport for HttpAgentProviderHarnessTransport {
    async fn send_command(
        &self,
        endpoint: &RuntimeServiceEndpoint,
        binding: &NodeAgentProviderRuntimeBindingV1,
        command: &AgentProviderCommandV1,
        timeout: Duration,
    ) -> Result<AgentProviderCommandReceiptV1, AgentProviderHarnessError> {
        binding
            .validate_command(command)
            .map_err(AgentProviderHarnessError::Invalid)?;
        let profile = binding
            .profile()
            .map_err(AgentProviderHarnessError::Invalid)?;
        let url = Self::endpoint_url(endpoint, AGENT_PROVIDER_COMMAND_HTTP_PATH_V1, timeout)?;
        let response = self
            .client
            .post(url)
            .timeout(timeout)
            .json(command)
            .send()
            .await
            .map_err(|error| AgentProviderHarnessError::Transport(error.to_string()))?;
        let body = bounded_body(
            response,
            AGENT_PROVIDER_MAX_COMMAND_RECEIPT_BYTES,
            "command receipt",
        )
        .await?;
        let receipt: AgentProviderCommandReceiptV1 = serde_json::from_slice(&body)
            .map_err(|error| AgentProviderHarnessError::Protocol(error.to_string()))?;
        receipt
            .validate_for(&profile, command)
            .map_err(AgentProviderHarnessError::Protocol)?;
        Ok(receipt)
    }

    async fn event_page(
        &self,
        endpoint: &RuntimeServiceEndpoint,
        binding: &NodeAgentProviderRuntimeBindingV1,
        request: &AgentProviderEventPageRequestV1,
        timeout: Duration,
    ) -> Result<AgentProviderEventPageV1, AgentProviderHarnessError> {
        binding
            .validate()
            .map_err(AgentProviderHarnessError::Invalid)?;
        let profile = binding
            .profile()
            .map_err(AgentProviderHarnessError::Invalid)?;
        request
            .validate_for(&profile)
            .map_err(AgentProviderHarnessError::Invalid)?;
        if request.identity != binding.provider_run_identity {
            return Err(AgentProviderHarnessError::Invalid(
                "Agent provider event request does not match its Runtime binding".into(),
            ));
        }
        let url = Self::endpoint_url(endpoint, AGENT_PROVIDER_EVENT_PAGE_HTTP_PATH_V1, timeout)?;
        let response = self
            .client
            .post(url)
            .timeout(timeout)
            .json(request)
            .send()
            .await
            .map_err(|error| AgentProviderHarnessError::Transport(error.to_string()))?;
        let body =
            bounded_body(response, AGENT_PROVIDER_MAX_EVENT_PAGE_BYTES, "event page").await?;
        let page: AgentProviderEventPageV1 = serde_json::from_slice(&body)
            .map_err(|error| AgentProviderHarnessError::Protocol(error.to_string()))?;
        page.validate_for(&profile)
            .map_err(AgentProviderHarnessError::Protocol)?;
        if page.identity != request.identity
            || page.after_event_sequence != request.after_event_sequence
            || page.source_event_count > request.limit
        {
            return Err(AgentProviderHarnessError::Protocol(
                "Agent provider event page changed its request identity or limit".into(),
            ));
        }
        Ok(page)
    }
}

async fn bounded_body(
    mut response: Response,
    maximum_bytes: usize,
    label: &str,
) -> Result<Vec<u8>, AgentProviderHarnessError> {
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|length| length > maximum_bytes as u64)
    {
        return Err(AgentProviderHarnessError::Protocol(format!(
            "Agent provider Harness {label} exceeds its protocol bound"
        )));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| AgentProviderHarnessError::Transport(error.to_string()))?
    {
        let next = body.len().checked_add(chunk.len()).ok_or_else(|| {
            AgentProviderHarnessError::Protocol("response size overflowed".into())
        })?;
        if next > maximum_bytes {
            return Err(AgentProviderHarnessError::Protocol(format!(
                "Agent provider Harness {label} exceeds its protocol bound"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        return Err(AgentProviderHarnessError::Rejected { status });
    }
    Ok(body)
}

pub(crate) async fn resolve_runtime_endpoint(
    runtime: &dyn RuntimeClient,
    binding: &NodeAgentProviderRuntimeBindingV1,
) -> Result<RuntimeServiceEndpoint, AgentProviderHarnessError> {
    binding
        .validate()
        .map_err(AgentProviderHarnessError::Invalid)?;
    let inspection = runtime
        .inspect(&binding.runtime_unit_id)
        .await
        .map_err(AgentProviderHarnessError::Runtime)?;
    let observation = match inspection {
        RuntimeInspection::Found { observation, .. } => observation,
        RuntimeInspection::NotFound { .. } => {
            return Err(AgentProviderHarnessError::Unavailable(format!(
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
        return Err(AgentProviderHarnessError::Invalid(
            "Runtime observation does not match the Agent provider binding".into(),
        ));
    }
    if observation.state != RuntimeUnitState::Running {
        return Err(AgentProviderHarnessError::Unavailable(format!(
            "Runtime Service {:?} is not running",
            binding.runtime_unit_id
        )));
    }
    let endpoint =
        RuntimeServiceEndpoint::from_observation(&observation, &binding.service_port_name)
            .map_err(AgentProviderHarnessError::Invalid)?;
    if endpoint.protocol != TransportProtocol::Tcp {
        return Err(AgentProviderHarnessError::Invalid(
            "Agent provider Harness Runtime port is not TCP".into(),
        ));
    }
    Ok(endpoint)
}

pub(crate) fn validate_reference_echo_binding(
    binding: &NodeAgentProviderRuntimeBindingV1,
) -> Result<(), AgentProviderHarnessError> {
    binding
        .validate()
        .map_err(AgentProviderHarnessError::Invalid)?;
    let profile = binding
        .profile()
        .map_err(AgentProviderHarnessError::Invalid)?;
    let expected =
        a3s_cloud_contracts::AgentProviderProfile::parse_acl(REFERENCE_ECHO_PROVIDER_PROFILE_ACL)
            .map_err(AgentProviderHarnessError::Invalid)?;
    if profile != expected {
        return Err(AgentProviderHarnessError::Invalid(format!(
            "Agent provider profile {:?} is not admitted by this node build",
            profile.kind()
        )));
    }
    Ok(())
}

pub(crate) type SharedAgentProviderHarnessTransport = Arc<dyn AgentProviderHarnessTransport>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum AgentProviderHarnessError {
    #[error("invalid Agent provider Harness binding or protocol: {0}")]
    Invalid(String),
    #[error("Agent provider Harness Runtime is unavailable: {0}")]
    Unavailable(String),
    #[error("Agent provider Harness Runtime inspection failed: {0}")]
    Runtime(a3s_runtime::RuntimeError),
    #[error("Agent provider Harness transport failed: {0}")]
    Transport(String),
    #[error("Agent provider Harness rejected the request with HTTP {status}")]
    Rejected { status: StatusCode },
    #[error("Agent provider Harness returned an invalid protocol response: {0}")]
    Protocol(String),
}

impl AgentProviderHarnessError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "invalid_agent_provider_harness_binding",
            Self::Unavailable(_) => "agent_provider_harness_unavailable",
            Self::Runtime(_) => "agent_provider_harness_runtime",
            Self::Transport(_) => "agent_provider_harness_transport",
            Self::Rejected { .. } => "agent_provider_harness_rejected",
            Self::Protocol(_) => "agent_provider_harness_protocol",
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

    pub(crate) const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}
