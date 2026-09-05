//! Versioned public and node protocol contracts for A3S Cloud.

mod agent_provider;
mod agent_release;
mod api;
mod app_platform_manifest;
mod automation;
mod cloud_scope_ref;
mod event;
mod function;
mod inference;
mod mcp;
mod node;
mod registry_credential;
mod resource;
mod workflow_node_profiles;

pub use a3s_code_core::release::{
    agent_harness_compatibility_v1, AgentReleaseCacheMode, AgentReleaseManifest,
    AgentReleasePersistentDataMode, AgentReleaseProvenance, AgentReleaseSecretRequirement,
    AgentReleaseSecretTarget, AgentReleaseWorkspaceMode, AGENT_RELEASE_CONTRACT_V1,
    AGENT_RELEASE_ENTRYPOINT_ARGS_V1, AGENT_RELEASE_ENTRYPOINT_COMMAND_V1, AGENT_RELEASE_LIMITS,
    AGENT_RELEASE_OCI_MEDIA_TYPE,
};
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
pub use a3s_runtime::contract::{
    IsolationLevel as RuntimeIsolationLevel, RuntimeServiceEndpoint, RuntimeUnitClass,
    RuntimeUnitState,
};
pub use agent_provider::{
    AgentProviderApprovalDecisionV1, AgentProviderApprovalOutcomeV1,
    AgentProviderCapabilityNegotiationV1, AgentProviderCapabilityRequirementsV1,
    AgentProviderCapabilityV1, AgentProviderCommandActionV1, AgentProviderCommandReceiptV1,
    AgentProviderCommandV1, AgentProviderEventPageRequestV1, AgentProviderEventPageV1,
    AgentProviderEventReceiptV1, AgentProviderEventRecordV1, AgentProviderProfile,
    AgentProviderRunCancelV1, AgentProviderRunIdentityV1, AgentProviderRunRecoverV1,
    AgentProviderRunResumeV1, AgentProviderRunStartV1, AgentProviderRunStateV1,
    AgentProviderSemanticEventV1, AgentProviderToolPayloadIdentityV1,
    AgentProviderToolResultOutcomeV1, HarnessAgentReleaseBindingV1, HarnessInvocationProfileV1,
    HarnessMcpBindingV1, HarnessModelBindingV1, HarnessProviderBindingV1, HarnessSecretReferenceV1,
    HarnessSecretTargetV1, HarnessSkillBindingV1, HarnessToolBindingV1, HarnessWorkspaceBindingV1,
    AGENT_PROVIDER_COMMAND_HTTP_PATH_V1, AGENT_PROVIDER_EVENT_PAGE_HTTP_PATH_V1,
    AGENT_PROVIDER_MAX_COMMAND_RECEIPT_BYTES, AGENT_PROVIDER_MAX_EVENTS_PER_PAGE,
    AGENT_PROVIDER_MAX_EVENT_PAGE_BYTES, AGENT_PROVIDER_MAX_PROMPT_BYTES,
    AGENT_PROVIDER_MAX_TOOL_PAYLOAD_BYTES, AGENT_PROVIDER_PROFILE_SCHEMA_V1,
    AGENT_PROVIDER_PROTOCOL_V1, AGENT_PROVIDER_TOOL_APPROVAL_TTL_MS_V1,
    HARNESS_INVOCATION_PROFILE_MAX_BYTES, HARNESS_INVOCATION_PROFILE_SCHEMA_V1,
    NATIVE_CODE_AGENT_PROVIDER_KIND, REFERENCE_ECHO_AGENT_PROVIDER_KIND,
    REFERENCE_ECHO_AGENT_PROVIDER_PROTOCOL_V1,
};
pub use agent_release::{
    agent_release_builder_uri, agent_release_manifest_archive, agent_release_source_uri,
    AGENT_RELEASE_MANIFEST_ARCHIVE_PATH,
};
pub use api::{ApiErrorResponse, ApiSuccessResponse};
pub use app_platform_manifest::{
    AppPlatformCapability, AppPlatformCapabilityAvailability, AppPlatformCapabilityCategory,
    AppPlatformGate, AppPlatformGateState, AppPlatformParityManifest, AppPlatformReference,
    APP_PLATFORM_PARITY_MANIFEST_SCHEMA,
};
pub use automation::{
    AutomationApplicationTargetV1, AutomationAuditActionV1, AutomationAuditRecordV1,
    AutomationAuthorizationPolicyV1, AutomationConcurrencyModeV1, AutomationConcurrencyPolicyV1,
    AutomationDeduplicationPolicyV1, AutomationDeduplicationScopeV1, AutomationDefinition,
    AutomationDefinitionSpecV1, AutomationDefinitionV1, AutomationEventTriggerV1,
    AutomationInvocationAuthorizationV1, AutomationInvocationEnvelope,
    AutomationInvocationEnvelopeV1, AutomationInvocationInputV1, AutomationInvocationOriginV1,
    AutomationMisfireModeV1, AutomationMisfirePolicyV1, AutomationOutboxEventKindV1,
    AutomationOutboxMessageV1, AutomationRevision, AutomationRevisionSpecV1, AutomationRevisionV1,
    AutomationScheduleTriggerV1, AutomationSubscriptionReferenceV1, AutomationTargetKindV1,
    AutomationTargetV1, AutomationTaskTargetV1, AutomationTriggerPolicyV1, AutomationTriggerV1,
    AutomationWebhookTriggerV1, AutomationWorkflowTargetV1, AUTOMATION_AUDIT_SCHEMA_V1,
    AUTOMATION_DEFINITION_MAX_ACL_BYTES, AUTOMATION_DEFINITION_SCHEMA_V1,
    AUTOMATION_INVOCATION_ENVELOPE_MAX_BYTES, AUTOMATION_INVOCATION_INLINE_MAX_BYTES,
    AUTOMATION_INVOCATION_SCHEMA_V1, AUTOMATION_MAX_CONCURRENCY,
    AUTOMATION_MAX_DEDUPLICATION_TEMPLATE_BYTES, AUTOMATION_MAX_DEDUPLICATION_WINDOW_MS,
    AUTOMATION_MAX_MISFIRE_GRACE_MS, AUTOMATION_MAX_NAME_BYTES, AUTOMATION_OUTBOX_SCHEMA_V1,
    AUTOMATION_REVISION_SCHEMA_V1,
};
pub use cloud_scope_ref::CloudScopeRef;
pub use event::DomainEventEnvelope;
pub use function::{
    ExternalFunctionTargetV1, FunctionEgressClassV1, FunctionFailureDispositionV1,
    FunctionInvocationAuthorityV1, FunctionInvocationFailureCodeV1, FunctionInvocationFailureV1,
    FunctionInvocationInputV1, FunctionInvocationParentKindV1, FunctionInvocationParentV1,
    FunctionInvocationPolicyV1, FunctionInvocationSlotV1, FunctionInvocationTargetV1,
    FunctionIoContractV1, FunctionModeV1, FunctionOwnerV1, FunctionPolicyV1, FunctionProfileSpecV1,
    FunctionProfileV1, FunctionSecretReferenceV1, FunctionSecurityV1, FunctionTargetV1,
    FunctionTrafficProtocolV1, FunctionTrafficV1, FunctionTrafficVisibilityV1,
    HostedServiceFunctionTargetV1, HostedTaskFunctionTargetV1, FUNCTION_EXTERNAL_MAX_INPUT_BYTES,
    FUNCTION_EXTERNAL_MAX_OUTPUT_BYTES, FUNCTION_EXTERNAL_MAX_TIMEOUT_MS,
    FUNCTION_HOSTED_SERVICE_MAX_INPUT_BYTES, FUNCTION_HOSTED_SERVICE_MAX_OUTPUT_BYTES,
    FUNCTION_HOSTED_SERVICE_MAX_TIMEOUT_MS, FUNCTION_HOSTED_TASK_MAX_INPUT_BYTES,
    FUNCTION_HOSTED_TASK_MAX_OUTPUT_BYTES, FUNCTION_HOSTED_TASK_MAX_TIMEOUT_MS,
    FUNCTION_INVOCATION_ENVELOPE_MAX_BYTES, FUNCTION_INVOCATION_FAILURE_SCHEMA_V1,
    FUNCTION_INVOCATION_INLINE_MAX_BYTES, FUNCTION_INVOCATION_SCHEMA_V1, FUNCTION_MAX_CONCURRENCY,
    FUNCTION_PROFILE_MAX_ACL_BYTES, FUNCTION_PROFILE_SCHEMA_V1,
};
pub use inference::{
    InferenceServingPhase, PowerAdmissionObservation, PowerPromptCacheObservation,
    PowerTransferHealth, PowerWorkerCapabilities, PowerWorkerObservation,
    POWER_WORKER_OBSERVATION_SCHEMA,
};
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
    NodeObservationReceipt, NodePluginHostCapabilitiesRequest, NodeProtocolContractSet,
    NodeProtocolError, NodeProtocolErrorCode, NodeResourceClaimBinding, NodeResourceClaimPrepare,
    NodeResourceClaimPrepared, NodeResourceClaimRelease, NodeResourceClaimReleased,
    NodeResourceInventory, NodeResourceInventoryReceipt, NodeResourceSlot,
    NodeSecretMaterialRequest, NodeSecretMaterialResponse, NodeSessionHello, NodeSessionSelection,
    NodeSessionSelectionReference, RuntimeObservationReport, BOX_BUILD_OUTPUT_NAME,
    DURABLE_CELL_BUNDLE_MEDIA_TYPE, MAX_BOX_ARTIFACT_BYTES, NODE_AGENT_PROVIDER_COMMAND_SCHEMA_V1,
    NODE_CODE_AGENT_COMMAND_SCHEMA_V1, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
    NODE_DURABLE_CELL_OPERATOR_OBSERVE_SCHEMA_V1, OCI_IMAGE_INDEX_MEDIA_TYPE,
    OCI_IMAGE_MANIFEST_MEDIA_TYPE, RUNTIME_RESOURCE_BINDING_DIGEST_KEY,
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
