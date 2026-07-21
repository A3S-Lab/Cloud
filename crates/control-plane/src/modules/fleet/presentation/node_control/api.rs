use super::error::NodeControlHttpError;
use crate::modules::artifacts::domain::{
    INodeArtifactStore, NodeArtifactDescriptor, NodeArtifactStoreError,
};
use crate::modules::edge::application::{SignGatewayCertificate, SignGatewayCertificateHandler};
use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::domain::services::IGatewayCertificateAuthority;
use crate::modules::fleet::application::{
    AcknowledgeNodeCommand, AcknowledgeNodeCommandHandler, LeaseNodeCommands,
    LeaseNodeCommandsHandler, RecordGatewayAcknowledgement, RecordGatewayAcknowledgementHandler,
    RecordNodeLogChunks, RecordNodeLogChunksHandler, RecordNodeObservations,
    RecordNodeObservationsHandler, RotateNodeCertificate, RotateNodeCertificateHandler,
};
use crate::modules::fleet::application::{
    IGatewayAcknowledgementProjector, NodeArtifactAuthorizer,
};
use crate::modules::fleet::domain::repositories::{INodeControlRepository, INodeRepository};
use crate::modules::fleet::domain::services::{ICertificateAuthority, ILogChunkStore};
use crate::modules::secrets::application::{ResolveSecretMaterial, ResolveSecretMaterialHandler};
use crate::modules::secrets::domain::{ISecretEncryptionService, ISecretRepository};
use crate::modules::shared_kernel::domain::{NodeCertificateId, NodeId, RepositoryError};
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use a3s_cloud_contracts::{
    artifact_uri, GatewayCertificateSigningRequest, NodeArtifactDownloadRequest,
    NodeArtifactUploadReceipt, NodeArtifactUploadRequest,
    NodeCertificate as NodeCertificateContract, NodeCertificateRotationRequest,
    NodeCertificateRotationResponse, NodeCommandAck, NodeCommandAckReceipt,
    NodeCommandLeaseRequest, NodeGatewayAck, NodeLogChunkBatch, NodeObservationBatch,
    NodeSecretMaterialRequest, NodeSecretMaterialResponse,
};
use a3s_runtime::contract::{ArtifactRef, RuntimeOutputArtifact};
use axum::body::{to_bytes, Body};
use axum::extract::{Extension, Path, RawQuery, Request, State};
use axum::http::{
    header::CACHE_CONTROL, header::CONTENT_LENGTH, header::CONTENT_TYPE, header::PRAGMA,
    HeaderName, HeaderValue, StatusCode,
};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use futures_util::TryStreamExt;
use http_body_util::LengthLimitError;
use serde::de::DeserializeOwned;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::error::Error as _;
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio_util::io::{ReaderStream, StreamReader};
use uuid::Uuid;

const SECRET_MATERIAL_TTL: Duration = Duration::seconds(30);

type RotationClock = Arc<dyn Fn() -> chrono::DateTime<Utc> + Send + Sync>;

#[derive(Clone)]
pub(super) struct PeerCertificate {
    pub(super) fingerprint: String,
}

#[derive(Clone)]
pub(crate) struct NodeControlApi {
    inner: Arc<NodeControlApiInner>,
}

struct NodeControlApiInner {
    nodes: Arc<dyn INodeRepository>,
    lease: LeaseNodeCommandsHandler,
    acknowledge: AcknowledgeNodeCommandHandler,
    observations: RecordNodeObservationsHandler,
    logs: RecordNodeLogChunksHandler,
    gateway: RecordGatewayAcknowledgementHandler,
    sign_gateway_certificate: SignGatewayCertificateHandler,
    rotate_certificate: RotateNodeCertificateHandler,
    resolve_secret_material: ResolveSecretMaterialHandler,
    artifact_authorizer: NodeArtifactAuthorizer,
    artifacts: Arc<dyn INodeArtifactStore>,
    certificate_rotation_window: Duration,
    rotation_clock: RotationClock,
    maximum_body_bytes: usize,
    body_timeout: StdDuration,
    artifact_transfer_timeout: StdDuration,
}

impl NodeControlApi {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        nodes: Arc<dyn INodeRepository>,
        commands: Arc<dyn INodeControlRepository>,
        artifacts: Arc<dyn INodeArtifactStore>,
        gateway_projector: Arc<dyn IGatewayAcknowledgementProjector>,
        gateway_certificates: Arc<dyn IEdgeRepository>,
        gateway_certificate_authority: Arc<dyn IGatewayCertificateAuthority>,
        logs: Arc<dyn ILogChunkStore>,
        certificate_authority: Arc<dyn ICertificateAuthority>,
        workloads: Arc<dyn IWorkloadRepository>,
        secrets: Arc<dyn ISecretRepository>,
        secret_encryption: Arc<dyn ISecretEncryptionService>,
        gateway_certificate_ttl: Duration,
        certificate_ttl: Duration,
        certificate_rotation_window: Duration,
        lease_duration: Duration,
        maximum_wait: StdDuration,
        retry_interval: StdDuration,
        maximum_body_bytes: usize,
        body_timeout: StdDuration,
        artifact_transfer_timeout: StdDuration,
    ) -> Result<Self, String> {
        if maximum_body_bytes == 0
            || body_timeout.is_zero()
            || artifact_transfer_timeout.is_zero()
            || certificate_rotation_window <= Duration::zero()
            || certificate_rotation_window >= certificate_ttl
        {
            return Err("node-control request bounds must be positive".into());
        }
        let rotate_certificate = RotateNodeCertificateHandler::new(
            Arc::clone(&nodes),
            certificate_authority,
            certificate_ttl,
        )?;
        let artifact_authorizer = NodeArtifactAuthorizer::new(Arc::clone(&commands));
        Ok(Self {
            inner: Arc::new(NodeControlApiInner {
                nodes,
                lease: LeaseNodeCommandsHandler::new(
                    Arc::clone(&commands),
                    lease_duration,
                    maximum_wait,
                    retry_interval,
                )?,
                acknowledge: AcknowledgeNodeCommandHandler::new(Arc::clone(&commands)),
                observations: RecordNodeObservationsHandler::new(Arc::clone(&commands)),
                logs: RecordNodeLogChunksHandler::new(Arc::clone(&commands), logs),
                gateway: RecordGatewayAcknowledgementHandler::new(commands, gateway_projector),
                sign_gateway_certificate: SignGatewayCertificateHandler::new(
                    gateway_certificates,
                    gateway_certificate_authority,
                    gateway_certificate_ttl,
                )?,
                rotate_certificate,
                resolve_secret_material: ResolveSecretMaterialHandler::new(
                    workloads,
                    secrets,
                    secret_encryption,
                ),
                artifact_authorizer,
                artifacts,
                certificate_rotation_window,
                rotation_clock: Arc::new(Utc::now),
                maximum_body_bytes,
                body_timeout,
                artifact_transfer_timeout,
            }),
        })
    }

    #[cfg(test)]
    pub(super) fn with_rotation_clock(mut self, rotation_clock: RotationClock) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("rotation clock must be configured before the API is cloned")
            .rotation_clock = rotation_clock;
        self
    }

    fn rotation_now(&self) -> chrono::DateTime<Utc> {
        (self.inner.rotation_clock)()
    }

    pub(super) fn router(self) -> Router {
        Router::new()
            .route("/v1/node-control/commands:lease", post(lease_commands))
            .route(
                "/v1/node-control/commands/{command_action}",
                post(acknowledge_command),
            )
            .route("/v1/node-control/observations", post(record_observations))
            .route("/v1/node-control/log-chunks", post(record_log_chunks))
            .route(
                "/v1/node-control/secrets:materialize",
                post(materialize_secret),
            )
            .route(
                "/v1/node-control/artifacts:download",
                get(download_artifact),
            )
            .route("/v1/node-control/artifacts:upload", put(upload_artifact))
            .route("/v1/node-control/gateway-acks", post(record_gateway_ack))
            .route(
                "/v1/node-control/gateway-certificates:sign",
                post(sign_gateway_certificate),
            )
            .route(
                "/v1/node-control/certificate:rotate",
                post(rotate_certificate),
            )
            .with_state(self)
    }

    async fn authenticate_rotation(
        &self,
        request_id: Uuid,
        peer: &PeerCertificate,
        request: &NodeCertificateRotationRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<crate::modules::fleet::domain::entities::Node, NodeControlHttpError> {
        let replay_not_before = now - self.inner.certificate_rotation_window;
        let node = self
            .inner
            .nodes
            .authenticate_rotation_certificate(&peer.fingerprint, now, replay_not_before)
            .await
            .map_err(|error| repository_authentication_error(request_id, error))?;
        if request.node_id != node.id.as_uuid() {
            return Err(NodeControlHttpError::unauthenticated(request_id));
        }
        let certificate = self
            .inner
            .nodes
            .find_certificate(
                node.organization_id,
                node.id,
                NodeCertificateId::from_uuid(request.current_certificate_id),
            )
            .await
            .map_err(|error| repository_authentication_error(request_id, error))?;
        if certificate.fingerprint != peer.fingerprint {
            return Err(NodeControlHttpError::unauthenticated(request_id));
        }
        Ok(node)
    }

    async fn authenticate(
        &self,
        request_id: Uuid,
        peer: &PeerCertificate,
    ) -> Result<NodeId, NodeControlHttpError> {
        self.authenticate_node(request_id, peer)
            .await
            .map(|node| node.id)
    }

    async fn authenticate_node(
        &self,
        request_id: Uuid,
        peer: &PeerCertificate,
    ) -> Result<crate::modules::fleet::domain::entities::Node, NodeControlHttpError> {
        self.inner
            .nodes
            .authenticate_certificate(&peer.fingerprint, Utc::now())
            .await
            .map_err(|error| match error {
                RepositoryError::NotFound => NodeControlHttpError::unauthenticated(request_id),
                other => {
                    tracing::error!(%request_id, %other, "node certificate authentication failed");
                    NodeControlHttpError::unavailable(
                        request_id,
                        "node certificate authentication is unavailable",
                    )
                }
            })
    }

    async fn body<T>(&self, request_id: Uuid, request: Request) -> Result<T, NodeControlHttpError>
    where
        T: DeserializeOwned,
    {
        require_json(request_id, request.headers().get(CONTENT_TYPE))?;
        if content_length(request.headers().get(CONTENT_LENGTH))
            .is_some_and(|size| size > self.inner.maximum_body_bytes as u64)
        {
            return Err(NodeControlHttpError::payload_too_large(
                request_id,
                self.inner.maximum_body_bytes,
            ));
        }
        let bytes = tokio::time::timeout(
            self.inner.body_timeout,
            to_bytes(request.into_body(), self.inner.maximum_body_bytes),
        )
        .await
        .map_err(|_| NodeControlHttpError::request_timeout(request_id))?
        .map_err(|error| {
            if error
                .source()
                .is_some_and(|source| source.is::<LengthLimitError>())
            {
                NodeControlHttpError::payload_too_large(request_id, self.inner.maximum_body_bytes)
            } else {
                NodeControlHttpError::invalid(
                    request_id,
                    format!("could not read request body: {error}"),
                )
            }
        })?;
        serde_json::from_slice(&bytes).map_err(|error| {
            NodeControlHttpError::invalid(request_id, format!("invalid JSON body: {error}"))
        })
    }
}

async fn download_artifact(
    State(api): State<NodeControlApi>,
    Extension(peer): Extension<PeerCertificate>,
    RawQuery(raw_query): RawQuery,
) -> Result<Response, NodeControlHttpError> {
    let request_id = Uuid::now_v7();
    let node_id = api.authenticate(request_id, &peer).await?;
    let request: NodeArtifactDownloadRequest = decode_query(request_id, raw_query)?;
    let artifact = api
        .inner
        .artifact_authorizer
        .authorize_download(node_id, &request, Utc::now())
        .await
        .map_err(|error| NodeControlHttpError::internal(request_id, error.to_string()))?
        .map_err(|error| NodeControlHttpError::from_application(request_id, error))?;
    let deadline = tokio::time::Instant::now() + api.inner.artifact_transfer_timeout;
    let opened = tokio::time::timeout_at(deadline, api.inner.artifacts.open(&artifact))
        .await
        .map_err(|_| NodeControlHttpError::request_timeout(request_id))?
        .map_err(|error| artifact_read_error(request_id, error))?;
    if opened.descriptor.artifact != artifact {
        return Err(NodeControlHttpError::unavailable(
            request_id,
            "artifact store returned a different artifact identity",
        ));
    }
    let mut source = ReaderStream::new(opened.reader);
    let stream = async_stream::stream! {
        loop {
            match tokio::time::timeout_at(deadline, source.try_next()).await {
                Ok(Ok(Some(chunk))) => yield Ok::<_, std::io::Error>(chunk),
                Ok(Ok(None)) => break,
                Ok(Err(error)) => {
                    yield Err(error);
                    break;
                }
                Err(_) => {
                    yield Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "artifact download exceeded its total transfer timeout",
                    ));
                    break;
                }
            }
        }
    };
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&artifact.media_type)
            .map_err(|error| NodeControlHttpError::internal(request_id, error.to_string()))?,
    );
    headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&opened.descriptor.size_bytes.to_string())
            .map_err(|error| NodeControlHttpError::internal(request_id, error.to_string()))?,
    );
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        HeaderName::from_static("x-a3s-artifact-digest"),
        HeaderValue::from_str(&artifact.digest)
            .map_err(|error| NodeControlHttpError::internal(request_id, error.to_string()))?,
    );
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_str(&request_id.to_string())
            .map_err(|error| NodeControlHttpError::internal(request_id, error.to_string()))?,
    );
    Ok(response)
}

async fn upload_artifact(
    State(api): State<NodeControlApi>,
    Extension(peer): Extension<PeerCertificate>,
    RawQuery(raw_query): RawQuery,
    request: Request,
) -> Result<Response, NodeControlHttpError> {
    let request_id = Uuid::now_v7();
    let node_id = api.authenticate(request_id, &peer).await?;
    let transfer: NodeArtifactUploadRequest = decode_query(request_id, raw_query)?;
    require_media_type(
        request_id,
        request.headers().get(CONTENT_TYPE),
        &transfer.media_type,
    )?;
    let declared_size = content_length(request.headers().get(CONTENT_LENGTH)).ok_or_else(|| {
        NodeControlHttpError::invalid(request_id, "artifact upload requires content-length")
    })?;
    if declared_size != transfer.size_bytes {
        return Err(NodeControlHttpError::invalid(
            request_id,
            "artifact content-length does not match its typed request",
        ));
    }
    api.inner
        .artifact_authorizer
        .authorize_upload(node_id, &transfer, Utc::now())
        .await
        .map_err(|error| NodeControlHttpError::internal(request_id, error.to_string()))?
        .map_err(|error| NodeControlHttpError::from_application(request_id, error))?;
    let artifact = ArtifactRef {
        uri: artifact_uri(&transfer.digest)
            .map_err(|error| NodeControlHttpError::invalid(request_id, error))?,
        digest: transfer.digest.clone(),
        media_type: transfer.media_type.clone(),
    };
    let descriptor = NodeArtifactDescriptor::new(artifact, transfer.size_bytes)
        .map_err(|error| NodeControlHttpError::invalid(request_id, error))?;
    let stream = request
        .into_body()
        .into_data_stream()
        .map_err(|error| std::io::Error::other(error.to_string()));
    let reader = StreamReader::new(stream);
    let write = tokio::time::timeout(
        api.inner.artifact_transfer_timeout,
        api.inner.artifacts.put(&descriptor, Box::pin(reader)),
    )
    .await
    .map_err(|_| NodeControlHttpError::request_timeout(request_id))?
    .map_err(|error| artifact_write_error(request_id, error))?;
    let receipt = NodeArtifactUploadReceipt {
        schema: NodeArtifactUploadReceipt::SCHEMA.into(),
        node_id: transfer.node_id,
        command_id: transfer.command_id,
        spec_digest: transfer.spec_digest.clone(),
        artifact: RuntimeOutputArtifact {
            name: transfer.output_name.clone(),
            artifact: write.descriptor.artifact,
            size_bytes: write.descriptor.size_bytes,
        },
        replayed: write.replayed,
    };
    receipt
        .validate_against(&transfer)
        .map_err(|error| NodeControlHttpError::internal(request_id, error))?;
    let status = if receipt.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    let mut response = json_response(request_id, status, &receipt)?;
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn materialize_secret(
    State(api): State<NodeControlApi>,
    Extension(peer): Extension<PeerCertificate>,
    request: Request,
) -> Result<Response, NodeControlHttpError> {
    let request_id = Uuid::now_v7();
    let node = api.authenticate_node(request_id, &peer).await?;
    let request: NodeSecretMaterialRequest = api.body(request_id, request).await?;
    request
        .validate()
        .map_err(|error| NodeControlHttpError::invalid(request_id, error))?;
    if request.node_id != node.id.as_uuid() {
        return Err(NodeControlHttpError::from_application(
            request_id,
            crate::modules::shared_kernel::application::ApplicationError::Forbidden(
                "Secret material request does not belong to this node".into(),
            ),
        ));
    }
    let reference = request.reference;
    let plaintext = api
        .inner
        .resolve_secret_material
        .execute(
            ResolveSecretMaterial {
                organization_id: node.organization_id,
                authenticated_node_id: node.id,
                reference,
            },
            context(),
        )
        .await
        .map_err(|error| NodeControlHttpError::internal(request_id, error.to_string()))?
        .map_err(|error| NodeControlHttpError::from_application(request_id, error))?;
    let issued_at = Utc::now();
    let response = NodeSecretMaterialResponse::new(
        node.id.as_uuid(),
        reference,
        plaintext.as_bytes(),
        issued_at,
        issued_at + SECRET_MATERIAL_TTL,
    )
    .map_err(|error| NodeControlHttpError::internal(request_id, error))?;
    let mut response = json_response(request_id, StatusCode::OK, &response)?;
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(PRAGMA, HeaderValue::from_static("no-cache"));
    Ok(response)
}

async fn lease_commands(
    State(api): State<NodeControlApi>,
    Extension(peer): Extension<PeerCertificate>,
    request: Request,
) -> Result<Response, NodeControlHttpError> {
    let request_id = Uuid::now_v7();
    let node_id = api.authenticate(request_id, &peer).await?;
    let body: NodeCommandLeaseRequest = api.body(request_id, request).await?;
    let result = api
        .inner
        .lease
        .execute(
            LeaseNodeCommands {
                authenticated_node_id: node_id,
                request: body,
                received_at: Utc::now(),
            },
            context(),
        )
        .await
        .map_err(|error| NodeControlHttpError::internal(request_id, error.to_string()))?
        .map_err(|error| NodeControlHttpError::from_application(request_id, error))?;
    json_response(request_id, StatusCode::OK, &result)
}

async fn acknowledge_command(
    State(api): State<NodeControlApi>,
    Extension(peer): Extension<PeerCertificate>,
    Path(command_action): Path<String>,
    request: Request,
) -> Result<Response, NodeControlHttpError> {
    let request_id = Uuid::now_v7();
    let node_id = api.authenticate(request_id, &peer).await?;
    let command_id = command_action
        .strip_suffix(":ack")
        .ok_or_else(|| NodeControlHttpError::invalid(request_id, "unsupported command action"))
        .and_then(|value| {
            Uuid::parse_str(value).map_err(|error| {
                NodeControlHttpError::invalid(request_id, format!("invalid command ID: {error}"))
            })
        })?;
    let acknowledgement: NodeCommandAck = api.body(request_id, request).await?;
    if acknowledgement.command_id != command_id {
        return Err(NodeControlHttpError::invalid(
            request_id,
            "command path does not match the acknowledgement body",
        ));
    }
    let result = api
        .inner
        .acknowledge
        .execute(
            AcknowledgeNodeCommand {
                authenticated_node_id: node_id,
                acknowledgement,
                received_at: Utc::now(),
            },
            context(),
        )
        .await
        .map_err(|error| NodeControlHttpError::internal(request_id, error.to_string()))?
        .map_err(|error| NodeControlHttpError::from_application(request_id, error))?;
    let receipt = NodeCommandAckReceipt {
        schema: NodeCommandAckReceipt::SCHEMA.into(),
        command_id: result.acknowledgement.command_id,
        node_id: result.acknowledgement.node_id,
        replayed: result.replayed,
    };
    receipt
        .validate()
        .map_err(|error| NodeControlHttpError::internal(request_id, error))?;
    json_response(request_id, StatusCode::OK, &receipt)
}

async fn record_observations(
    State(api): State<NodeControlApi>,
    Extension(peer): Extension<PeerCertificate>,
    request: Request,
) -> Result<Response, NodeControlHttpError> {
    let request_id = Uuid::now_v7();
    let node_id = api.authenticate(request_id, &peer).await?;
    let batch: NodeObservationBatch = api.body(request_id, request).await?;
    let result = api
        .inner
        .observations
        .execute(
            RecordNodeObservations {
                authenticated_node_id: node_id,
                batch,
                received_at: Utc::now(),
            },
            context(),
        )
        .await
        .map_err(|error| NodeControlHttpError::internal(request_id, error.to_string()))?
        .map_err(|error| NodeControlHttpError::from_application(request_id, error))?;
    json_response(request_id, StatusCode::OK, &result)
}

async fn record_log_chunks(
    State(api): State<NodeControlApi>,
    Extension(peer): Extension<PeerCertificate>,
    request: Request,
) -> Result<Response, NodeControlHttpError> {
    let request_id = Uuid::now_v7();
    let node_id = api.authenticate(request_id, &peer).await?;
    let batch: NodeLogChunkBatch = api.body(request_id, request).await?;
    let result = api
        .inner
        .logs
        .execute(
            RecordNodeLogChunks {
                authenticated_node_id: node_id,
                batch,
                received_at: Utc::now(),
            },
            context(),
        )
        .await
        .map_err(|error| NodeControlHttpError::internal(request_id, error.to_string()))?
        .map_err(|error| NodeControlHttpError::from_application(request_id, error))?;
    json_response(request_id, StatusCode::OK, &result)
}

async fn record_gateway_ack(
    State(api): State<NodeControlApi>,
    Extension(peer): Extension<PeerCertificate>,
    request: Request,
) -> Result<Response, NodeControlHttpError> {
    let request_id = Uuid::now_v7();
    let node_id = api.authenticate(request_id, &peer).await?;
    let acknowledgement: NodeGatewayAck = api.body(request_id, request).await?;
    let result = api
        .inner
        .gateway
        .execute(
            RecordGatewayAcknowledgement {
                authenticated_node_id: node_id,
                acknowledgement,
                received_at: Utc::now(),
            },
            context(),
        )
        .await
        .map_err(|error| NodeControlHttpError::internal(request_id, error.to_string()))?
        .map_err(|error| NodeControlHttpError::from_application(request_id, error))?;
    json_response(request_id, StatusCode::OK, &result)
}

async fn sign_gateway_certificate(
    State(api): State<NodeControlApi>,
    Extension(peer): Extension<PeerCertificate>,
    request: Request,
) -> Result<Response, NodeControlHttpError> {
    let request_id = Uuid::now_v7();
    let node_id = api.authenticate(request_id, &peer).await?;
    let request: GatewayCertificateSigningRequest = api.body(request_id, request).await?;
    let result = api
        .inner
        .sign_gateway_certificate
        .execute(
            SignGatewayCertificate {
                authenticated_node_id: node_id,
                request,
                received_at: Utc::now(),
            },
            context(),
        )
        .await
        .map_err(|error| NodeControlHttpError::internal(request_id, error.to_string()))?
        .map_err(|error| NodeControlHttpError::from_application(request_id, error))?;
    json_response(request_id, StatusCode::OK, &result)
}

async fn rotate_certificate(
    State(api): State<NodeControlApi>,
    Extension(peer): Extension<PeerCertificate>,
    request: Request,
) -> Result<Response, NodeControlHttpError> {
    let request_id = Uuid::now_v7();
    let request: NodeCertificateRotationRequest = api.body(request_id, request).await?;
    request
        .validate()
        .map_err(|error| NodeControlHttpError::invalid(request_id, error))?;
    let now = api.rotation_now();
    let node = api
        .authenticate_rotation(request_id, &peer, &request, now)
        .await?;
    let idempotency_key = format!(
        "mtls-rotation-{:x}",
        Sha256::digest(
            [
                request.current_certificate_id.as_bytes().as_slice(),
                request.csr_pem.as_bytes(),
            ]
            .concat()
        )
    );
    let previous_certificate_id = request.current_certificate_id;
    let result = api
        .inner
        .rotate_certificate
        .execute(
            RotateNodeCertificate {
                organization_id: node.organization_id,
                node_id: node.id,
                current_certificate_id: NodeCertificateId::from_uuid(previous_certificate_id),
                csr_pem: request.csr_pem,
                idempotency_key,
                request_id,
                requested_at: now,
            },
            context(),
        )
        .await
        .map_err(|error| NodeControlHttpError::internal(request_id, error.to_string()))?
        .map_err(|error| NodeControlHttpError::from_application(request_id, error))?;
    let response = NodeCertificateRotationResponse {
        schema: NodeCertificateRotationResponse::SCHEMA.into(),
        node_id: node.id.as_uuid(),
        previous_certificate_id,
        certificate: NodeCertificateContract {
            certificate_id: result.certificate.id.as_uuid(),
            serial_number: result.certificate.serial_number,
            certificate_pem: result.certificate.certificate_pem,
            ca_bundle_pem: result.certificate.ca_bundle_pem,
            issued_at: result.certificate.issued_at,
            expires_at: result.certificate.expires_at,
        },
        replayed: result.replayed,
    };
    response
        .validate()
        .map_err(|error| NodeControlHttpError::internal(request_id, error))?;
    json_response(request_id, StatusCode::OK, &response)
}

fn repository_authentication_error(
    request_id: Uuid,
    error: RepositoryError,
) -> NodeControlHttpError {
    match error {
        RepositoryError::NotFound => NodeControlHttpError::unauthenticated(request_id),
        other => {
            tracing::error!(%request_id, %other, "node rotation certificate authentication failed");
            NodeControlHttpError::unavailable(
                request_id,
                "node certificate authentication is unavailable",
            )
        }
    }
}

fn require_json(
    request_id: Uuid,
    content_type: Option<&HeaderValue>,
) -> Result<(), NodeControlHttpError> {
    let is_json = content_type
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"));
    if is_json {
        Ok(())
    } else {
        Err(NodeControlHttpError::invalid(
            request_id,
            "content-type must be application/json",
        ))
    }
}

fn require_media_type(
    request_id: Uuid,
    content_type: Option<&HeaderValue>,
    expected: &str,
) -> Result<(), NodeControlHttpError> {
    let actual = content_type
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if actual == Some(expected) {
        Ok(())
    } else {
        Err(NodeControlHttpError::invalid(
            request_id,
            "content-type does not match the typed artifact request",
        ))
    }
}

fn decode_query<T>(request_id: Uuid, raw: Option<String>) -> Result<T, NodeControlHttpError>
where
    T: DeserializeOwned,
{
    let raw = raw.ok_or_else(|| NodeControlHttpError::invalid(request_id, "query is required"))?;
    serde_urlencoded::from_str(&raw).map_err(|error| {
        NodeControlHttpError::invalid(request_id, format!("invalid query: {error}"))
    })
}

fn artifact_read_error(request_id: Uuid, error: NodeArtifactStoreError) -> NodeControlHttpError {
    match error {
        NodeArtifactStoreError::Invalid(message) => {
            NodeControlHttpError::invalid(request_id, message)
        }
        NodeArtifactStoreError::NotFound => NodeControlHttpError::from_application(
            request_id,
            crate::modules::shared_kernel::application::ApplicationError::NotFound(
                "artifact was not found".into(),
            ),
        ),
        NodeArtifactStoreError::Conflict => NodeControlHttpError::from_application(
            request_id,
            crate::modules::shared_kernel::application::ApplicationError::Conflict(
                "artifact identity conflicts with stored content".into(),
            ),
        ),
        NodeArtifactStoreError::Integrity(_) | NodeArtifactStoreError::Storage(_) => {
            tracing::error!(%request_id, %error, "artifact download storage failed");
            NodeControlHttpError::unavailable(request_id, "artifact storage is unavailable")
        }
    }
}

fn artifact_write_error(request_id: Uuid, error: NodeArtifactStoreError) -> NodeControlHttpError {
    match error {
        NodeArtifactStoreError::Invalid(message) | NodeArtifactStoreError::Integrity(message) => {
            NodeControlHttpError::invalid(request_id, message)
        }
        NodeArtifactStoreError::Conflict => NodeControlHttpError::from_application(
            request_id,
            crate::modules::shared_kernel::application::ApplicationError::Conflict(
                "artifact identity conflicts with stored content".into(),
            ),
        ),
        NodeArtifactStoreError::NotFound | NodeArtifactStoreError::Storage(_) => {
            tracing::error!(%request_id, %error, "artifact upload storage failed");
            NodeControlHttpError::unavailable(request_id, "artifact storage is unavailable")
        }
    }
}

fn content_length(value: Option<&HeaderValue>) -> Option<u64> {
    value
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

fn json_response<T: Serialize>(
    request_id: Uuid,
    status: StatusCode,
    value: &T,
) -> Result<Response, NodeControlHttpError> {
    let mut response = (status, Json(value)).into_response();
    response.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_str(&request_id.to_string())
            .map_err(|error| NodeControlHttpError::internal(request_id, error.to_string()))?,
    );
    Ok(response)
}
