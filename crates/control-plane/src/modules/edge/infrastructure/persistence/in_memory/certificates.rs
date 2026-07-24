use crate::modules::edge::domain::{GatewayCertificate, GatewayCertificateState};
use crate::modules::shared_kernel::domain::RepositoryError;

pub(super) fn validate_transition(
    existing: &GatewayCertificate,
    next: &GatewayCertificate,
    expected_version: u64,
) -> Result<(), RepositoryError> {
    let transition_is_valid = match (existing.state, next.state) {
        (GatewayCertificateState::Provisioning, GatewayCertificateState::Issued) => {
            next.csr_digest.is_some()
                && next.material.is_some()
                && next.failure.is_none()
                && next.ready_at.is_none()
                && next.revoked_at.is_none()
        }
        (GatewayCertificateState::Provisioning, GatewayCertificateState::Failed) => {
            next.csr_digest.is_some()
                && next.material.is_none()
                && next.failure.is_some()
                && next.ready_at.is_none()
                && next.revoked_at.is_none()
        }
        (GatewayCertificateState::Ready, GatewayCertificateState::Revoked) => {
            next.csr_digest == existing.csr_digest
                && next.material == existing.material
                && next.failure.is_some()
                && next.ready_at == existing.ready_at
                && next.revoked_at.is_some()
        }
        _ => false,
    };
    if existing.aggregate_version != expected_version
        || next.aggregate_version != expected_version.saturating_add(1)
        || !transition_is_valid
        || existing.id != next.id
        || existing.organization_id != next.organization_id
        || existing.node_id != next.node_id
        || existing.domain_claim_ids != next.domain_claim_ids
        || existing.gateway_revision != next.gateway_revision
        || existing.gateway_command_id != next.gateway_command_id
        || existing.snapshot_digest != next.snapshot_digest
        || existing.request != next.request
        || existing.created_at != next.created_at
        || next.updated_at < existing.updated_at
    {
        return Err(RepositoryError::Conflict(
            "Gateway certificate changed while applying its transition".into(),
        ));
    }
    Ok(())
}
