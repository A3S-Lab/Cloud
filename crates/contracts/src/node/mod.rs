mod agent_provider;
mod artifact;
mod box_build;
mod code_agent;
mod command;
mod command_lease;
mod durable_cell;
mod enrollment;
mod error;
mod gateway;
mod inventory;
mod observation;
mod plugin_host;
mod resource_claim;
mod secret;
mod session;
#[cfg(test)]
mod tests;

pub use agent_provider::{
    NodeAgentProviderEventBatchV1, NodeAgentProviderEventReceiptV1,
    NodeAgentProviderRuntimeBindingV1, NODE_AGENT_PROVIDER_COMMAND_SCHEMA_V1,
};
pub use artifact::{
    artifact_uri, validate_cloud_artifact, NodeArtifactDownloadRequest, NodeArtifactUploadReceipt,
    NodeArtifactUploadRequest, DURABLE_CELL_BUNDLE_MEDIA_TYPE, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
    SKILL_BUNDLE_MEDIA_TYPE,
};
pub use box_build::{
    NodeBoxBuildCacheInput, NodeBoxBuildCacheOutput, NodeBoxBuildCacheReceipt,
    NodeBoxBuildCancelResult, NodeBoxBuildCancellation, NodeBoxBuildDescriptor,
    NodeBoxBuildInspection, NodeBoxBuildOperationCancellation, NodeBoxBuildOperationRemoval,
    NodeBoxBuildOutput, NodeBoxBuildPhase, NodeBoxBuildPlan, NodeBoxBuildPlatform,
    NodeBoxBuildRemoveResult, NodeBoxBuildRequest, NodeBoxBuildStartResult, BOX_BUILD_OUTPUT_NAME,
    MAX_BOX_ARTIFACT_BYTES, OCI_IMAGE_INDEX_MEDIA_TYPE, OCI_IMAGE_MANIFEST_MEDIA_TYPE,
};
pub use code_agent::{
    NodeCodeAgentEventBatchV1, NodeCodeAgentEventReceiptV1, NodeCodeAgentRuntimeBindingV1,
    NODE_CODE_AGENT_COMMAND_SCHEMA_V1,
};
pub use command::{
    NodeCommandAck, NodeCommandAckReceipt, NodeCommandEnvelope, NodeCommandFailure,
    NodeCommandMetadata, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
};
pub use command_lease::{NodeCommandLeaseRequest, NodeCommandLeaseResponse};
pub use durable_cell::{
    NodeDurableCellOperatorBindingV1, NodeDurableCellOperatorObservationV1,
    NODE_DURABLE_CELL_OPERATOR_OBSERVE_SCHEMA_V1,
};
pub use enrollment::{
    NodeCertificate, NodeCertificateRotationRequest, NodeCertificateRotationResponse,
    NodeEnrollmentRequest, NodeEnrollmentResponse,
};
pub use error::{NodeProtocolError, NodeProtocolErrorCode};
pub use gateway::{
    AppliedGatewaySnapshot, GatewayCertificateRequest, GatewayCertificateSigningRequest,
    GatewayCertificateSigningResponse, GatewayManagementProtocol,
    GatewayManagementProtocolDiscovery, GatewaySnapshot, GatewaySnapshotObservationRequest,
    GatewaySnapshotObservationState, NodeGatewaySnapshotObservation,
};
pub use inventory::{
    NodeInventoryReference, NodeResourceInventory, NodeResourceInventoryReceipt, NodeResourceSlot,
};
pub use observation::{
    GatewayAckState, NodeGatewayAck, NodeGatewayAckReceipt, NodeHeartbeat, NodeHeartbeatV2,
    NodeLogChunkBatch, NodeLogChunkReceipt, NodeLogChunkReport, NodeLogGapReport,
    NodeObservationBatch, NodeObservationBatchEnvelope, NodeObservationBatchV2,
    NodeObservationReceipt, RuntimeObservationReport,
};
pub use plugin_host::NodePluginHostCapabilitiesRequest;
pub use resource_claim::{
    NodeResourceClaimBinding, NodeResourceClaimPrepare, NodeResourceClaimPrepared,
    NodeResourceClaimRelease, NodeResourceClaimReleased, RUNTIME_RESOURCE_BINDING_DIGEST_KEY,
    RUNTIME_RESOURCE_CLAIM_ID_KEY,
};
pub use secret::{CloudSecretReference, NodeSecretMaterialRequest, NodeSecretMaterialResponse};
pub use session::{
    NodeProtocolContractSet, NodeSessionHello, NodeSessionSelection, NodeSessionSelectionReference,
};

pub(crate) fn validate_single_line(label: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > max
        || value.contains('\0')
        || value.contains(['\r', '\n'])
    {
        return Err(format!(
            "{label} must be a bounded nonempty single-line value"
        ));
    }
    Ok(())
}

pub(crate) fn validate_sha256(label: &str, value: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{label} must use sha256"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "{label} must contain exactly 64 hexadecimal characters"
        ));
    }
    Ok(())
}

pub(crate) fn validate_lower_sha256(label: &str, value: &str) -> Result<(), String> {
    let valid = value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if valid {
        Ok(())
    } else {
        Err(format!("{label} must be a lowercase SHA-256 digest"))
    }
}

pub(crate) fn validate_uuid(label: &str, value: uuid::Uuid) -> Result<(), String> {
    if value.is_nil() {
        return Err(format!("{label} must not be nil"));
    }
    Ok(())
}
