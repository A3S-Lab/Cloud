//! Versioned public and node protocol contracts for A3S Cloud.

mod api;
mod event;
mod node;
mod registry_credential;

pub use api::{ApiErrorResponse, ApiSuccessResponse};
pub use event::DomainEventEnvelope;
pub use node::{
    artifact_uri, validate_cloud_artifact, CloudSecretReference, GatewayAckState,
    GatewayCertificateRequest, GatewayCertificateSigningRequest, GatewayCertificateSigningResponse,
    GatewaySnapshot, NodeArtifactDownloadRequest, NodeArtifactUploadReceipt,
    NodeArtifactUploadRequest, NodeCertificate, NodeCertificateRotationRequest,
    NodeCertificateRotationResponse, NodeCommandAck, NodeCommandAckReceipt, NodeCommandEnvelope,
    NodeCommandFailure, NodeCommandLeaseRequest, NodeCommandLeaseResponse, NodeCommandMetadata,
    NodeCommandOutcome, NodeCommandPayload, NodeCommandResult, NodeEnrollmentRequest,
    NodeEnrollmentResponse, NodeGatewayAck, NodeGatewayAckReceipt, NodeHeartbeat,
    NodeLogChunkBatch, NodeLogChunkReceipt, NodeLogChunkReport, NodeLogGapReport,
    NodeObservationBatch, NodeObservationReceipt, NodeProtocolError, NodeProtocolErrorCode,
    NodeSecretMaterialRequest, NodeSecretMaterialResponse, RuntimeObservationReport,
    RuntimeServiceEndpoint, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
pub use registry_credential::RegistryCredentialMaterial;
