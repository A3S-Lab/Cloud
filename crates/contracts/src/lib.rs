//! Versioned public and node protocol contracts for A3S Cloud.

mod api;
mod event;
mod mcp;
mod node;
mod registry_credential;
mod resource;

pub use a3s_runtime::contract::RuntimeServiceEndpoint;
pub use api::{ApiErrorResponse, ApiSuccessResponse};
pub use event::DomainEventEnvelope;
pub use mcp::{
    validate_mcp_allowed_origins, validate_mcp_telemetry_names, McpCredentialProjection,
    McpGatewayProjection, McpGrantProjection, McpLimitsProjection, McpRoutePolicyProjection,
    McpServiceProfileProjection, McpTargetProjection, MCP_CREDENTIAL_AUDIENCE,
    MCP_GATEWAY_PROJECTION_SCHEMA, MCP_PROTOCOL_VERSION,
};
pub use node::{
    artifact_uri, validate_cloud_artifact, AppliedGatewaySnapshot, CloudSecretReference,
    GatewayAckState, GatewayCertificateRequest, GatewayCertificateSigningRequest,
    GatewayCertificateSigningResponse, GatewayManagementProtocol,
    GatewayManagementProtocolDiscovery, GatewaySnapshot, GatewaySnapshotObservationRequest,
    GatewaySnapshotObservationState, NodeArtifactDownloadRequest, NodeArtifactUploadReceipt,
    NodeArtifactUploadRequest, NodeBoxBuildCacheInput, NodeBoxBuildCacheOutput,
    NodeBoxBuildCacheReceipt, NodeBoxBuildCancelResult, NodeBoxBuildCancellation,
    NodeBoxBuildDescriptor, NodeBoxBuildInspection, NodeBoxBuildOperationCancellation,
    NodeBoxBuildOperationRemoval, NodeBoxBuildOutput, NodeBoxBuildPhase, NodeBoxBuildPlan,
    NodeBoxBuildPlatform, NodeBoxBuildRemoveResult, NodeBoxBuildRequest, NodeBoxBuildStartResult,
    NodeCertificate, NodeCertificateRotationRequest, NodeCertificateRotationResponse,
    NodeCommandAck, NodeCommandAckReceipt, NodeCommandEnvelope, NodeCommandFailure,
    NodeCommandLeaseRequest, NodeCommandLeaseResponse, NodeCommandMetadata, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult, NodeEnrollmentRequest, NodeEnrollmentResponse,
    NodeGatewayAck, NodeGatewayAckReceipt, NodeGatewaySnapshotObservation, NodeHeartbeat,
    NodeHeartbeatV2, NodeInventoryReference, NodeLogChunkBatch, NodeLogChunkReceipt,
    NodeLogChunkReport, NodeLogGapReport, NodeObservationBatch, NodeObservationBatchEnvelope,
    NodeObservationBatchV2, NodeObservationReceipt, NodeProtocolError, NodeProtocolErrorCode,
    NodeResourceClaimBinding, NodeResourceClaimPrepare, NodeResourceClaimPrepared,
    NodeResourceClaimRelease, NodeResourceClaimReleased, NodeResourceInventory,
    NodeResourceInventoryReceipt, NodeResourceSlot, NodeSecretMaterialRequest,
    NodeSecretMaterialResponse, RuntimeObservationReport, BOX_BUILD_OUTPUT_NAME,
    NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE, RUNTIME_RESOURCE_BINDING_DIGEST_KEY,
    RUNTIME_RESOURCE_CLAIM_ID_KEY,
};
pub use registry_credential::RegistryCredentialMaterial;
pub use resource::{
    validate_slot_bindings, validate_slot_evidence, validate_slot_requests, ResourceAllocation,
    ResourceKind, ResourceSlotBinding, ResourceSlotEvidence, ResourceSlotRequest, ResourceUnit,
};
