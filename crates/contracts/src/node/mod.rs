mod command;
mod enrollment;
mod error;
mod gateway;
mod observation;
mod runtime_endpoint;
#[cfg(test)]
mod tests;

pub use command::{
    NodeCommandAck, NodeCommandAckReceipt, NodeCommandEnvelope, NodeCommandFailure,
    NodeCommandLeaseRequest, NodeCommandLeaseResponse, NodeCommandMetadata, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult,
};
pub use enrollment::{
    NodeCertificate, NodeCertificateRotationRequest, NodeCertificateRotationResponse,
    NodeEnrollmentRequest, NodeEnrollmentResponse,
};
pub use error::{NodeProtocolError, NodeProtocolErrorCode};
pub use gateway::{
    GatewayCertificateRequest, GatewayCertificateSigningRequest, GatewayCertificateSigningResponse,
    GatewaySnapshot,
};
pub use observation::{
    GatewayAckState, NodeGatewayAck, NodeGatewayAckReceipt, NodeHeartbeat, NodeLogChunkBatch,
    NodeLogChunkReceipt, NodeLogChunkReport, NodeObservationBatch, NodeObservationReceipt,
    RuntimeObservationReport,
};
pub use runtime_endpoint::RuntimeServiceEndpoint;

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

pub(crate) fn validate_uuid(label: &str, value: uuid::Uuid) -> Result<(), String> {
    if value.is_nil() {
        return Err(format!("{label} must not be nil"));
    }
    Ok(())
}
