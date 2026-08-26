//! Versioned public and node protocol contracts for A3S Cloud.

mod agent_provider;
mod api;
mod app_platform_manifest;
mod event;
mod mcp;
mod node;
mod registry_credential;
mod resource;
mod workflow_node_profiles;

pub use a3s_code_core::{
    AgentEventTypeV1, AgentProtocolChangeSetRequestV1, AgentProtocolChangeSetV1,
    AgentProtocolCommandActionV1, AgentProtocolCommandReceiptV1, AgentProtocolCommandV1,
    AgentProtocolEventPageRequestV1, AgentProtocolEventPageV1, AgentProtocolEventRecordV1,
    AgentProtocolRunCancelV1, AgentProtocolRunIdentityV1, AgentProtocolRunRecoverV1,
    AgentProtocolRunStartV1, AgentProtocolRunStateV1, AGENT_PROTOCOL_CHANGE_SET_ENCODING_V1,
    AGENT_PROTOCOL_CHANGE_SET_FORMAT_V1, AGENT_PROTOCOL_CHANGE_SET_HTTP_PATH_V1,
    AGENT_PROTOCOL_COMMAND_HTTP_PATH_V1, AGENT_PROTOCOL_EVENT_PAGE_HTTP_PATH_V1,
    AGENT_PROTOCOL_MAX_CHANGE_SET_BYTES, AGENT_PROTOCOL_MAX_CHANGE_SET_RESPONSE_BYTES,
    AGENT_PROTOCOL_MAX_EVENTS_PER_PAGE, AGENT_PROTOCOL_MAX_EVENT_PAGE_BYTES,
    AGENT_PROTOCOL_MAX_EVENT_RECORD_BYTES, AGENT_PROTOCOL_V1,
};
pub use a3s_runtime::contract::RuntimeServiceEndpoint;
pub use agent_provider::{
    AgentProviderCapabilityNegotiationV1, AgentProviderCapabilityRequirementsV1,
    AgentProviderCapabilityV1, AgentProviderCommandActionV1, AgentProviderCommandReceiptV1,
    AgentProviderCommandV1, AgentProviderEventPageRequestV1, AgentProviderEventPageV1,
    AgentProviderEventReceiptV1, AgentProviderEventRecordV1, AgentProviderProfile,
    AgentProviderRunCancelV1, AgentProviderRunIdentityV1, AgentProviderRunRecoverV1,
    AgentProviderRunStartV1, AgentProviderRunStateV1, AgentProviderSemanticEventV1,
    AgentProviderToolPayloadIdentityV1, AgentProviderToolResultOutcomeV1,
    HarnessAgentReleaseBindingV1, HarnessInvocationProfileV1, HarnessMcpBindingV1,
    HarnessModelBindingV1, HarnessProviderBindingV1, HarnessSecretReferenceV1,
    HarnessSecretTargetV1, HarnessSkillBindingV1, HarnessToolBindingV1, HarnessWorkspaceBindingV1,
    AGENT_PROVIDER_COMMAND_HTTP_PATH_V1, AGENT_PROVIDER_EVENT_PAGE_HTTP_PATH_V1,
    AGENT_PROVIDER_MAX_COMMAND_RECEIPT_BYTES, AGENT_PROVIDER_MAX_EVENTS_PER_PAGE,
    AGENT_PROVIDER_MAX_EVENT_PAGE_BYTES, AGENT_PROVIDER_MAX_TOOL_PAYLOAD_BYTES,
    AGENT_PROVIDER_PROFILE_SCHEMA_V1, AGENT_PROVIDER_PROTOCOL_V1,
    HARNESS_INVOCATION_PROFILE_MAX_BYTES, HARNESS_INVOCATION_PROFILE_SCHEMA_V1,
    NATIVE_CODE_AGENT_PROVIDER_KIND, REFERENCE_ECHO_AGENT_PROVIDER_KIND,
    REFERENCE_ECHO_AGENT_PROVIDER_PROTOCOL_V1,
};
pub use api::{ApiErrorResponse, ApiSuccessResponse};
pub use app_platform_manifest::{
    AppPlatformCapability, AppPlatformCapabilityAvailability, AppPlatformCapabilityCategory,
    AppPlatformGate, AppPlatformGateState, AppPlatformParityManifest, AppPlatformReference,
    APP_PLATFORM_PARITY_MANIFEST_SCHEMA,
};
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
    GatewaySnapshotObservationState, NodeAgentProviderEventBatchV1,
    NodeAgentProviderEventReceiptV1, NodeAgentProviderRuntimeBindingV1,
    NodeArtifactDownloadRequest, NodeArtifactUploadReceipt, NodeArtifactUploadRequest,
    NodeBoxBuildCacheInput, NodeBoxBuildCacheOutput, NodeBoxBuildCacheReceipt,
    NodeBoxBuildCancelResult, NodeBoxBuildCancellation, NodeBoxBuildDescriptor,
    NodeBoxBuildInspection, NodeBoxBuildOperationCancellation, NodeBoxBuildOperationRemoval,
    NodeBoxBuildOutput, NodeBoxBuildPhase, NodeBoxBuildPlan, NodeBoxBuildPlatform,
    NodeBoxBuildRemoveResult, NodeBoxBuildRequest, NodeBoxBuildStartResult, NodeCertificate,
    NodeCertificateRotationRequest, NodeCertificateRotationResponse, NodeCodeAgentEventBatchV1,
    NodeCodeAgentEventReceiptV1, NodeCodeAgentRuntimeBindingV1, NodeCommandAck,
    NodeCommandAckReceipt, NodeCommandEnvelope, NodeCommandFailure, NodeCommandLeaseRequest,
    NodeCommandLeaseResponse, NodeCommandMetadata, NodeCommandOutcome, NodeCommandPayload,
    NodeCommandResult, NodeDurableCellOperatorBindingV1, NodeDurableCellOperatorObservationV1,
    NodeEnrollmentRequest, NodeEnrollmentResponse, NodeGatewayAck, NodeGatewayAckReceipt,
    NodeGatewaySnapshotObservation, NodeHeartbeat, NodeHeartbeatV2, NodeInventoryReference,
    NodeLogChunkBatch, NodeLogChunkReceipt, NodeLogChunkReport, NodeLogGapReport,
    NodeObservationBatch, NodeObservationBatchEnvelope, NodeObservationBatchV2,
    NodeObservationReceipt, NodePluginHostCapabilitiesRequest, NodeProtocolError,
    NodeProtocolErrorCode, NodeResourceClaimBinding, NodeResourceClaimPrepare,
    NodeResourceClaimPrepared, NodeResourceClaimRelease, NodeResourceClaimReleased,
    NodeResourceInventory, NodeResourceInventoryReceipt, NodeResourceSlot,
    NodeSecretMaterialRequest, NodeSecretMaterialResponse, RuntimeObservationReport,
    BOX_BUILD_OUTPUT_NAME, DURABLE_CELL_BUNDLE_MEDIA_TYPE, MAX_BOX_ARTIFACT_BYTES,
    NODE_AGENT_PROVIDER_COMMAND_SCHEMA_V1, NODE_CODE_AGENT_COMMAND_SCHEMA_V1,
    NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE, NODE_DURABLE_CELL_OPERATOR_OBSERVE_SCHEMA_V1,
    OCI_IMAGE_INDEX_MEDIA_TYPE, OCI_IMAGE_MANIFEST_MEDIA_TYPE, RUNTIME_RESOURCE_BINDING_DIGEST_KEY,
    RUNTIME_RESOURCE_CLAIM_ID_KEY, SKILL_BUNDLE_MEDIA_TYPE,
};
pub use registry_credential::RegistryCredentialMaterial;
pub use resource::{
    validate_slot_bindings, validate_slot_evidence, validate_slot_requests, ResourceAllocation,
    ResourceKind, ResourceSlotBinding, ResourceSlotEvidence, ResourceSlotRequest, ResourceUnit,
};
pub use workflow_node_profiles::{
    WorkflowNodeExecutionClass, WorkflowNodeKind, WorkflowNodeProfile, WorkflowNodeProfiles,
    WORKFLOW_NODE_PROFILES_REVISION, WORKFLOW_NODE_PROFILES_SCHEMA,
};
