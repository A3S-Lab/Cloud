use crate::{
    ControlPlaneConfig, EnrolledNodeIdentity, FileNodeIdentityStore, IdentityStoreError,
    NodeSecretTransport, PendingNodeIdentity, SecretMaterial,
};
use a3s_cloud_contracts::{
    ApiErrorResponse, CloudSecretReference, GatewayCertificateSigningRequest,
    GatewayCertificateSigningResponse, NodeCertificateRotationRequest,
    NodeCertificateRotationResponse, NodeCommandAck, NodeCommandAckReceipt,
    NodeCommandLeaseRequest, NodeCommandLeaseResponse, NodeEnrollmentResponse, NodeGatewayAck,
    NodeGatewayAckReceipt, NodeLogChunkBatch, NodeLogChunkReceipt, NodeObservationBatch,
    NodeObservationReceipt, NodeProtocolError, NodeSecretMaterialRequest,
    NodeSecretMaterialResponse,
};
use async_trait::async_trait;
use chrono::Utc;
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::RwLock;
use url::Url;
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct NodeControlClient {
    client: Client,
    base_url: Url,
    node_id: uuid::Uuid,
    agent_instance_id: uuid::Uuid,
    request_timeout: Duration,
    long_poll_margin: Duration,
    maximum_response_bytes: usize,
}

#[async_trait]
pub trait NodeControlTransport: Send + Sync {
    async fn lease(
        &self,
        after_sequence: u64,
        max_commands: u16,
        wait_ms: u64,
    ) -> Result<NodeCommandLeaseResponse, NodeControlClientError>;

    async fn acknowledge(
        &self,
        acknowledgement: &NodeCommandAck,
    ) -> Result<NodeCommandAckReceipt, NodeControlClientError>;

    async fn record_observations(
        &self,
        batch: &NodeObservationBatch,
    ) -> Result<NodeObservationReceipt, NodeControlClientError>;

    async fn record_log_chunks(
        &self,
        batch: &NodeLogChunkBatch,
    ) -> Result<NodeLogChunkReceipt, NodeControlClientError>;

    async fn record_gateway_acknowledgement(
        &self,
        acknowledgement: &NodeGatewayAck,
    ) -> Result<NodeGatewayAckReceipt, NodeControlClientError>;
}

#[async_trait]
pub trait GatewayCertificateSigningTransport: Send + Sync {
    async fn sign_gateway_certificate(
        &self,
        request: &GatewayCertificateSigningRequest,
    ) -> Result<GatewayCertificateSigningResponse, NodeControlClientError>;
}

impl NodeControlClient {
    pub async fn enroll(
        config: &ControlPlaneConfig,
        pending: &PendingNodeIdentity,
        enrollment_token: String,
    ) -> Result<NodeEnrollmentResponse, NodeControlClientError> {
        let client = build_client(config, None).await?;
        let request = pending.enrollment_request(enrollment_token);
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        let response = client
            .post(config.enrollment_url.clone())
            .timeout(Duration::from_millis(config.request_timeout_ms))
            .json(&request)
            .send()
            .await
            .map_err(transport_error)?;
        let response: NodeEnrollmentResponse =
            decode_response(response, config.max_response_bytes).await?;
        response
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        Ok(response)
    }

    pub async fn new(
        config: &ControlPlaneConfig,
        identity: &EnrolledNodeIdentity,
    ) -> Result<Self, NodeControlClientError> {
        identity
            .response
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        let client = build_client(config, Some(identity.identity_pem())).await?;
        Ok(Self {
            client,
            base_url: config.node_control_url.clone(),
            node_id: identity.response.node_id,
            agent_instance_id: identity.agent_instance_id,
            request_timeout: Duration::from_millis(config.request_timeout_ms),
            long_poll_margin: Duration::from_millis(config.long_poll_margin_ms),
            maximum_response_bytes: config.max_response_bytes,
        })
    }

    pub async fn lease(
        &self,
        after_sequence: u64,
        max_commands: u16,
        wait_ms: u64,
    ) -> Result<NodeCommandLeaseResponse, NodeControlClientError> {
        let request = NodeCommandLeaseRequest {
            schema: NodeCommandLeaseRequest::SCHEMA.into(),
            node_id: self.node_id,
            agent_instance_id: self.agent_instance_id,
            after_sequence,
            max_commands,
            wait_ms,
        };
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        let timeout = Duration::from_millis(wait_ms)
            .checked_add(self.long_poll_margin)
            .ok_or_else(|| {
                NodeControlClientError::Invalid("long-poll timeout overflowed".into())
            })?;
        let response: NodeCommandLeaseResponse = self
            .send(
                self.client
                    .post(self.endpoint("v1/node-control/commands:lease")?)
                    .timeout(timeout)
                    .json(&request),
            )
            .await?;
        response
            .validate(Utc::now())
            .map_err(NodeControlClientError::Invalid)?;
        if response.node_id != self.node_id || response.agent_instance_id != self.agent_instance_id
        {
            return Err(NodeControlClientError::Invalid(
                "command lease response changed the node identity".into(),
            ));
        }
        Ok(response)
    }

    pub async fn rotate_certificate(
        &self,
        request: &NodeCertificateRotationRequest,
    ) -> Result<NodeCertificateRotationResponse, NodeControlClientError> {
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        if request.node_id != self.node_id {
            return Err(NodeControlClientError::Invalid(
                "certificate rotation request changed the node identity".into(),
            ));
        }
        let response: NodeCertificateRotationResponse = self
            .send(
                self.client
                    .post(self.endpoint("v1/node-control/certificate:rotate")?)
                    .timeout(self.request_timeout)
                    .json(request),
            )
            .await?;
        response
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        if response.node_id != self.node_id
            || response.previous_certificate_id != request.current_certificate_id
        {
            return Err(NodeControlClientError::Invalid(
                "certificate rotation response changed the certificate identity".into(),
            ));
        }
        Ok(response)
    }

    pub async fn sign_gateway_certificate(
        &self,
        request: &GatewayCertificateSigningRequest,
    ) -> Result<GatewayCertificateSigningResponse, NodeControlClientError> {
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        if request.node_id != self.node_id {
            return Err(NodeControlClientError::Invalid(
                "Gateway certificate signing request changed the node identity".into(),
            ));
        }
        let response: GatewayCertificateSigningResponse = self
            .send(
                self.client
                    .post(self.endpoint("v1/node-control/gateway-certificates:sign")?)
                    .timeout(self.request_timeout)
                    .json(request),
            )
            .await?;
        response
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        if response.node_id != self.node_id || response.certificate_id != request.certificate_id {
            return Err(NodeControlClientError::Invalid(
                "Gateway certificate signing response changed the requested identity".into(),
            ));
        }
        Ok(response)
    }

    pub async fn acknowledge(
        &self,
        acknowledgement: &NodeCommandAck,
    ) -> Result<NodeCommandAckReceipt, NodeControlClientError> {
        let endpoint = format!(
            "v1/node-control/commands/{}:ack",
            acknowledgement.command_id
        );
        let receipt: NodeCommandAckReceipt = self
            .send(
                self.client
                    .post(self.endpoint(&endpoint)?)
                    .timeout(self.request_timeout)
                    .json(acknowledgement),
            )
            .await?;
        receipt
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        if receipt.command_id != acknowledgement.command_id
            || receipt.node_id != acknowledgement.node_id
            || receipt.node_id != self.node_id
        {
            return Err(NodeControlClientError::Invalid(
                "command acknowledgement receipt changed the command identity".into(),
            ));
        }
        Ok(receipt)
    }

    pub async fn record_observations(
        &self,
        batch: &NodeObservationBatch,
    ) -> Result<NodeObservationReceipt, NodeControlClientError> {
        let receipt: NodeObservationReceipt = self
            .send(
                self.client
                    .post(self.endpoint("v1/node-control/observations")?)
                    .timeout(self.request_timeout)
                    .json(batch),
            )
            .await?;
        receipt
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        if receipt.node_id != batch.node_id
            || receipt.node_id != self.node_id
            || usize::from(receipt.accepted_reports) + usize::from(receipt.replayed_reports)
                != batch.observations.len()
        {
            return Err(NodeControlClientError::Invalid(
                "node observation receipt changed the batch identity or count".into(),
            ));
        }
        Ok(receipt)
    }

    pub async fn record_log_chunks(
        &self,
        batch: &NodeLogChunkBatch,
    ) -> Result<NodeLogChunkReceipt, NodeControlClientError> {
        let receipt: NodeLogChunkReceipt = self
            .send(
                self.client
                    .post(self.endpoint("v1/node-control/log-chunks")?)
                    .timeout(self.request_timeout)
                    .json(batch),
            )
            .await?;
        receipt
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        if receipt.batch_id != batch.batch_id
            || receipt.node_id != batch.node_id
            || receipt.node_id != self.node_id
            || usize::from(receipt.accepted_chunks) != batch.chunks.len()
        {
            return Err(NodeControlClientError::Invalid(
                "node log receipt changed the batch identity or count".into(),
            ));
        }
        Ok(receipt)
    }

    pub async fn record_gateway_acknowledgement(
        &self,
        acknowledgement: &NodeGatewayAck,
    ) -> Result<NodeGatewayAckReceipt, NodeControlClientError> {
        let receipt: NodeGatewayAckReceipt = self
            .send(
                self.client
                    .post(self.endpoint("v1/node-control/gateway-acks")?)
                    .timeout(self.request_timeout)
                    .json(acknowledgement),
            )
            .await?;
        receipt
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        if receipt.acknowledgement_id != acknowledgement.acknowledgement_id
            || receipt.command_id != acknowledgement.command_id
            || receipt.node_id != acknowledgement.node_id
            || receipt.node_id != self.node_id
        {
            return Err(NodeControlClientError::Invalid(
                "Gateway acknowledgement receipt changed the acknowledgement identity".into(),
            ));
        }
        Ok(receipt)
    }

    pub async fn resolve_secret(
        &self,
        reference: CloudSecretReference,
    ) -> Result<SecretMaterial, NodeControlClientError> {
        let request = NodeSecretMaterialRequest::new(self.node_id, reference)
            .map_err(NodeControlClientError::Invalid)?;
        let response: NodeSecretMaterialResponse = self
            .send(
                self.client
                    .post(self.endpoint("v1/node-control/secrets:materialize")?)
                    .timeout(self.request_timeout)
                    .json(&request),
            )
            .await?;
        response
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        if response.node_id != self.node_id || response.reference != reference {
            return Err(NodeControlClientError::Invalid(
                "Secret material response changed its node or reference identity".into(),
            ));
        }
        let value = response
            .decode_at(Utc::now())
            .map_err(NodeControlClientError::Invalid)?;
        SecretMaterial::new(value).map_err(NodeControlClientError::Invalid)
    }

    async fn send<T>(&self, request: RequestBuilder) -> Result<T, NodeControlClientError>
    where
        T: DeserializeOwned,
    {
        let response = request.send().await.map_err(transport_error)?;
        decode_response(response, self.maximum_response_bytes).await
    }

    fn endpoint(&self, path: &str) -> Result<Url, NodeControlClientError> {
        self.base_url
            .join(path)
            .map_err(|error| NodeControlClientError::Invalid(error.to_string()))
    }
}

pub(crate) struct ReloadableNodeControlClient {
    inner: RwLock<NodeControlClient>,
}

impl ReloadableNodeControlClient {
    pub(crate) fn new(client: NodeControlClient) -> Self {
        Self {
            inner: RwLock::new(client),
        }
    }

    pub(crate) async fn rotate(
        &self,
        config: &ControlPlaneConfig,
        store: &FileNodeIdentityStore,
        prepared: &EnrolledNodeIdentity,
    ) -> Result<EnrolledNodeIdentity, CertificateReloadError> {
        let request = prepared.pending_rotation_request().ok_or_else(|| {
            CertificateReloadError::Identity(IdentityStoreError::Conflict(
                "certificate rotation was not prepared".into(),
            ))
        })?;
        let mut client = self.inner.write().await;
        let response = client.rotate_certificate(&request).await?;
        let identity = store.complete_rotation(response).await?;
        *client = NodeControlClient::new(config, &identity).await?;
        Ok(identity)
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CertificateReloadError {
    #[error(transparent)]
    ControlPlane(#[from] NodeControlClientError),
    #[error(transparent)]
    Identity(#[from] IdentityStoreError),
}

impl CertificateReloadError {
    pub(crate) fn retryable(&self) -> bool {
        matches!(self, Self::ControlPlane(error) if error.retryable())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NodeControlClientError {
    #[error("invalid node-control data: {0}")]
    Invalid(String),
    #[error("node-control transport failed: {0}")]
    Transport(String),
    #[error("node-control request was rejected with HTTP {status} ({code}): {message}")]
    Rejected {
        status: u16,
        code: String,
        message: String,
        retryable: bool,
    },
}

impl NodeControlClientError {
    pub fn retryable(&self) -> bool {
        match self {
            Self::Invalid(_) => false,
            Self::Transport(_) => true,
            Self::Rejected { retryable, .. } => *retryable,
        }
    }

    pub fn requires_command_redelivery(&self) -> bool {
        matches!(
            self,
            Self::Rejected {
                status: 409,
                code,
                ..
            } if code == "conflict"
        )
    }
}

#[async_trait]
impl NodeControlTransport for NodeControlClient {
    async fn lease(
        &self,
        after_sequence: u64,
        max_commands: u16,
        wait_ms: u64,
    ) -> Result<NodeCommandLeaseResponse, NodeControlClientError> {
        NodeControlClient::lease(self, after_sequence, max_commands, wait_ms).await
    }

    async fn acknowledge(
        &self,
        acknowledgement: &NodeCommandAck,
    ) -> Result<NodeCommandAckReceipt, NodeControlClientError> {
        NodeControlClient::acknowledge(self, acknowledgement).await
    }

    async fn record_observations(
        &self,
        batch: &NodeObservationBatch,
    ) -> Result<NodeObservationReceipt, NodeControlClientError> {
        NodeControlClient::record_observations(self, batch).await
    }

    async fn record_log_chunks(
        &self,
        batch: &NodeLogChunkBatch,
    ) -> Result<NodeLogChunkReceipt, NodeControlClientError> {
        NodeControlClient::record_log_chunks(self, batch).await
    }

    async fn record_gateway_acknowledgement(
        &self,
        acknowledgement: &NodeGatewayAck,
    ) -> Result<NodeGatewayAckReceipt, NodeControlClientError> {
        NodeControlClient::record_gateway_acknowledgement(self, acknowledgement).await
    }
}

#[async_trait]
impl GatewayCertificateSigningTransport for NodeControlClient {
    async fn sign_gateway_certificate(
        &self,
        request: &GatewayCertificateSigningRequest,
    ) -> Result<GatewayCertificateSigningResponse, NodeControlClientError> {
        NodeControlClient::sign_gateway_certificate(self, request).await
    }
}

#[async_trait]
impl NodeSecretTransport for NodeControlClient {
    async fn resolve_secret(
        &self,
        reference: CloudSecretReference,
    ) -> Result<SecretMaterial, NodeControlClientError> {
        NodeControlClient::resolve_secret(self, reference).await
    }
}

#[async_trait]
impl NodeControlTransport for ReloadableNodeControlClient {
    async fn lease(
        &self,
        after_sequence: u64,
        max_commands: u16,
        wait_ms: u64,
    ) -> Result<NodeCommandLeaseResponse, NodeControlClientError> {
        self.inner
            .read()
            .await
            .lease(after_sequence, max_commands, wait_ms)
            .await
    }

    async fn acknowledge(
        &self,
        acknowledgement: &NodeCommandAck,
    ) -> Result<NodeCommandAckReceipt, NodeControlClientError> {
        self.inner.read().await.acknowledge(acknowledgement).await
    }

    async fn record_observations(
        &self,
        batch: &NodeObservationBatch,
    ) -> Result<NodeObservationReceipt, NodeControlClientError> {
        self.inner.read().await.record_observations(batch).await
    }

    async fn record_log_chunks(
        &self,
        batch: &NodeLogChunkBatch,
    ) -> Result<NodeLogChunkReceipt, NodeControlClientError> {
        self.inner.read().await.record_log_chunks(batch).await
    }

    async fn record_gateway_acknowledgement(
        &self,
        acknowledgement: &NodeGatewayAck,
    ) -> Result<NodeGatewayAckReceipt, NodeControlClientError> {
        self.inner
            .read()
            .await
            .record_gateway_acknowledgement(acknowledgement)
            .await
    }
}

#[async_trait]
impl GatewayCertificateSigningTransport for ReloadableNodeControlClient {
    async fn sign_gateway_certificate(
        &self,
        request: &GatewayCertificateSigningRequest,
    ) -> Result<GatewayCertificateSigningResponse, NodeControlClientError> {
        self.inner
            .read()
            .await
            .sign_gateway_certificate(request)
            .await
    }
}

#[async_trait]
impl NodeSecretTransport for ReloadableNodeControlClient {
    async fn resolve_secret(
        &self,
        reference: CloudSecretReference,
    ) -> Result<SecretMaterial, NodeControlClientError> {
        self.inner.read().await.resolve_secret(reference).await
    }
}

async fn build_client(
    config: &ControlPlaneConfig,
    identity_pem: Option<String>,
) -> Result<Client, NodeControlClientError> {
    let path = config.server_ca_file.clone();
    let bundle = read_file(path).await?;
    let roots = reqwest::Certificate::from_pem_bundle(&bundle).map_err(|error| {
        NodeControlClientError::Invalid(format!("server CA bundle is invalid: {error}"))
    })?;
    if roots.is_empty() {
        return Err(NodeControlClientError::Invalid(
            "server CA bundle is empty".into(),
        ));
    }
    let mut builder = Client::builder()
        .connect_timeout(Duration::from_millis(config.connect_timeout_ms))
        .tls_built_in_root_certs(false)
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .user_agent(format!("a3s-cloud-node/{}", env!("CARGO_PKG_VERSION")));
    for root in roots {
        builder = builder.add_root_certificate(root);
    }
    if let Some(identity_pem) = identity_pem {
        let identity = reqwest::Identity::from_pem(identity_pem.as_bytes()).map_err(|error| {
            NodeControlClientError::Invalid(format!("node TLS identity is invalid: {error}"))
        })?;
        builder = builder.identity(identity);
    }
    builder
        .build()
        .map_err(|error| NodeControlClientError::Invalid(error.to_string()))
}

async fn read_file(path: PathBuf) -> Result<Vec<u8>, NodeControlClientError> {
    tokio::task::spawn_blocking(move || std::fs::read(&path))
        .await
        .map_err(|error| {
            NodeControlClientError::Transport(format!("server CA read task failed: {error}"))
        })?
        .map_err(|error| {
            NodeControlClientError::Invalid(format!("could not read server CA bundle: {error}"))
        })
}

async fn decode_response<T>(
    mut response: Response,
    maximum_bytes: usize,
) -> Result<T, NodeControlClientError>
where
    T: DeserializeOwned,
{
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|length| length > maximum_bytes as u64)
    {
        return Err(NodeControlClientError::Invalid(format!(
            "node-control response exceeds {maximum_bytes} bytes"
        )));
    }
    let mut body = Zeroizing::new(Vec::new());
    while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
        let next = body.len().checked_add(chunk.len()).ok_or_else(|| {
            NodeControlClientError::Invalid("node-control response size overflowed".into())
        })?;
        if next > maximum_bytes {
            return Err(NodeControlClientError::Invalid(format!(
                "node-control response exceeds {maximum_bytes} bytes"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    if status.is_success() {
        return serde_json::from_slice(&body).map_err(|error| {
            NodeControlClientError::Invalid(format!(
                "node-control success response is invalid: {error}"
            ))
        });
    }
    Err(rejected_error(status, &body))
}

fn rejected_error(status: StatusCode, body: &[u8]) -> NodeControlClientError {
    if let Ok(error) = serde_json::from_slice::<NodeProtocolError>(body) {
        let code = serde_json::to_value(error.code)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "unknown_protocol_error".into());
        return NodeControlClientError::Rejected {
            status: status.as_u16(),
            code,
            message: error.message,
            retryable: error.retryable,
        };
    }
    if let Ok(error) = serde_json::from_slice::<ApiErrorResponse>(body) {
        return NodeControlClientError::Rejected {
            status: status.as_u16(),
            code: error.status_code,
            message: error.message,
            retryable: status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS,
        };
    }
    NodeControlClientError::Rejected {
        status: status.as_u16(),
        code: "invalid_error_response".into(),
        message: "control plane returned an invalid error response".into(),
        retryable: status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS,
    }
}

fn transport_error(error: reqwest::Error) -> NodeControlClientError {
    NodeControlClientError::Transport(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryability_is_explicit_and_does_not_depend_on_message_text() {
        assert!(!NodeControlClientError::Invalid("bad certificate".into()).retryable());
        assert!(NodeControlClientError::Transport("connection reset".into()).retryable());
        assert!(!NodeControlClientError::Rejected {
            status: 409,
            code: "conflict".into(),
            message: "conflict".into(),
            retryable: false,
        }
        .retryable());
    }
}
