//! Versioned public and node protocol contracts for A3S Cloud.

mod api;
mod event;
mod node;

pub use api::{ApiErrorResponse, ApiSuccessResponse};
pub use event::DomainEventEnvelope;
pub use node::{
    GatewayAckState, NodeCertificate, NodeCertificateRotationRequest,
    NodeCertificateRotationResponse, NodeCommandAck, NodeCommandAckReceipt, NodeCommandEnvelope,
    NodeCommandFailure, NodeCommandLeaseRequest, NodeCommandLeaseResponse, NodeCommandMetadata,
    NodeCommandOutcome, NodeCommandPayload, NodeCommandResult, NodeEnrollmentRequest,
    NodeEnrollmentResponse, NodeGatewayAck, NodeGatewayAckReceipt, NodeHeartbeat,
    NodeLogChunkBatch, NodeLogChunkReceipt, NodeLogChunkReport, NodeObservationBatch,
    NodeObservationReceipt, NodeProtocolError, NodeProtocolErrorCode, RuntimeObservationReport,
};
