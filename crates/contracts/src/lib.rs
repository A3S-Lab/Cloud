//! Versioned public and node protocol contracts for A3S Cloud.

mod api;
mod event;
mod node;
mod registry_credential;

pub use api::{ApiErrorResponse, ApiSuccessResponse};
pub use event::DomainEventEnvelope;
pub use node::{
    CloudSecretReference, GatewayAckState, GatewayCertificateRequest,
    GatewayCertificateSigningRequest, GatewayCertificateSigningResponse, GatewaySnapshot,
    NodeCertificate, NodeCertificateRotationRequest, NodeCertificateRotationResponse,
    NodeCommandAck, NodeCommandAckReceipt, NodeCommandEnvelope, NodeCommandFailure,
    NodeCommandLeaseRequest, NodeCommandLeaseResponse, NodeCommandMetadata, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult, NodeEnrollmentRequest, NodeEnrollmentResponse,
    NodeGatewayAck, NodeGatewayAckReceipt, NodeHeartbeat, NodeLogChunkBatch, NodeLogChunkReceipt,
    NodeLogChunkReport, NodeLogGapReport, NodeObservationBatch, NodeObservationReceipt,
    NodeProtocolError, NodeProtocolErrorCode, NodeSecretMaterialRequest,
    NodeSecretMaterialResponse, RuntimeObservationReport, RuntimeServiceEndpoint,
};
pub use registry_credential::RegistryCredentialMaterial;
