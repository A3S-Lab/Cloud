//! Versioned public and node protocol contracts for A3S Cloud.

mod api;
mod event;
mod node;
mod registry_credential;
mod resource;

pub use api::{ApiErrorResponse, ApiSuccessResponse};
pub use event::DomainEventEnvelope;
pub use node::{
    artifact_uri, validate_cloud_artifact, CloudSecretReference, GatewayAckState,
    GatewayCertificateRequest, GatewayCertificateSigningRequest, GatewayCertificateSigningResponse,
    GatewayManagementProtocol, GatewayManagementProtocolDiscovery, GatewaySnapshot,
    NodeArtifactDownloadRequest, NodeArtifactUploadReceipt, NodeArtifactUploadRequest,
    NodeCertificate, NodeCertificateRotationRequest, NodeCertificateRotationResponse,
    NodeCommandAck, NodeCommandAckReceipt, NodeCommandEnvelope, NodeCommandFailure,
    NodeCommandLeaseRequest, NodeCommandLeaseResponse, NodeCommandMetadata, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult, NodeEnrollmentRequest, NodeEnrollmentResponse,
    NodeGatewayAck, NodeGatewayAckReceipt, NodeHeartbeat, NodeHeartbeatV2, NodeInventoryReference,
    NodeLogChunkBatch, NodeLogChunkReceipt, NodeLogChunkReport, NodeLogGapReport,
    NodeObservationBatch, NodeObservationBatchEnvelope, NodeObservationBatchV2,
    NodeObservationReceipt, NodeProtocolError, NodeProtocolErrorCode, NodeResourceClaimBinding,
    NodeResourceClaimPrepare, NodeResourceClaimPrepared, NodeResourceClaimRelease,
    NodeResourceClaimReleased, NodeResourceInventory, NodeResourceInventoryReceipt,
    NodeResourceSlot, NodeSecretMaterialRequest, NodeSecretMaterialResponse,
    RuntimeObservationReport, RuntimeServiceEndpoint, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
    RUNTIME_RESOURCE_BINDING_DIGEST_KEY, RUNTIME_RESOURCE_CLAIM_ID_KEY,
};
pub use registry_credential::RegistryCredentialMaterial;
pub use resource::{
    validate_slot_bindings, validate_slot_evidence, validate_slot_requests, ResourceAllocation,
    ResourceKind, ResourceSlotBinding, ResourceSlotEvidence, ResourceSlotRequest, ResourceUnit,
};
