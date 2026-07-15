use super::{RotateNodeCertificate, RotateNodeCertificateResult};
use crate::modules::fleet::application::certificate;
use crate::modules::fleet::domain::events::NodeCertificateRotated;
use crate::modules::fleet::domain::repositories::{
    INodeRepository, NodeCertificateRotationCompletion, NodeCertificateRotationDraft,
};
use crate::modules::fleet::domain::services::{ICertificateAuthority, NodeCertificateRequest};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, NodeCertificateId};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use a3s_cloud_contracts::NodeCertificateRotationRequest;
use chrono::Duration;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

pub struct RotateNodeCertificateHandler {
    nodes: Arc<dyn INodeRepository>,
    certificate_authority: Arc<dyn ICertificateAuthority>,
    certificate_ttl: Duration,
}

impl RotateNodeCertificateHandler {
    pub fn new(
        nodes: Arc<dyn INodeRepository>,
        certificate_authority: Arc<dyn ICertificateAuthority>,
        certificate_ttl: Duration,
    ) -> Result<Self, String> {
        if certificate_ttl <= Duration::zero() {
            return Err("node certificate TTL must be positive".into());
        }
        Ok(Self {
            nodes,
            certificate_authority,
            certificate_ttl,
        })
    }
}

impl CommandHandler<RotateNodeCertificate> for RotateNodeCertificateHandler {
    fn execute(
        &self,
        command: RotateNodeCertificate,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<RotateNodeCertificateResult>>,
    > {
        let nodes = Arc::clone(&self.nodes);
        let certificate_authority = Arc::clone(&self.certificate_authority);
        let certificate_ttl = self.certificate_ttl;
        Box::pin(async move {
            let request = NodeCertificateRotationRequest {
                schema: NodeCertificateRotationRequest::SCHEMA.into(),
                node_id: command.node_id.as_uuid(),
                current_certificate_id: command.current_certificate_id.as_uuid(),
                csr_pem: command.csr_pem.clone(),
            };
            if let Err(error) = request.validate() {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "nodeId": command.node_id,
                "currentCertificateId": command.current_certificate_id,
                "csrPem": command.csr_pem,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/nodes/{}/certificate-rotations",
                    command.organization_id, command.node_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let replacement_certificate_id = deterministic_certificate_id(
                command.node_id.as_uuid(),
                command.current_certificate_id.as_uuid(),
                &idempotency.request_digest,
            );
            let reservation = match nodes
                .reserve_certificate_rotation(
                    command.organization_id,
                    command.node_id,
                    command.current_certificate_id,
                    NodeCertificateRotationDraft {
                        replacement_certificate_id,
                        requested_at: command.requested_at,
                    },
                    idempotency.clone(),
                )
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };

            let (replacement, replayed) = if let Some(existing) = reservation.replacement {
                (existing, true)
            } else {
                let replacement = match certificate_authority
                    .issue(NodeCertificateRequest {
                        certificate_id: reservation.replacement_certificate_id,
                        node_id: reservation.node.id,
                        csr_pem: command.csr_pem,
                        issued_at: reservation.requested_at,
                        expires_at: reservation.requested_at + certificate_ttl,
                    })
                    .await
                {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(certificate::application_error(error))),
                };
                let mut projected_node = reservation.node.clone();
                projected_node.aggregate_version += 1;
                let event = NodeCertificateRotated::envelope(
                    &projected_node,
                    &reservation.current_certificate,
                    &replacement,
                    command.requested_at,
                    command.request_id,
                )
                .map_err(|error| BootError::Internal(error.to_string()))?;
                let completed = match nodes
                    .complete_certificate_rotation(NodeCertificateRotationCompletion {
                        organization_id: command.organization_id,
                        node_id: command.node_id,
                        current_certificate_id: command.current_certificate_id,
                        replacement,
                        rotated_at: command.requested_at,
                        event,
                        idempotency,
                    })
                    .await
                {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(error.into())),
                };
                let replacement = completed.replacement.ok_or_else(|| {
                    BootError::Internal("completed certificate rotation has no replacement".into())
                })?;
                (replacement, completed.replayed)
            };

            if let Err(error) = certificate_authority
                .revoke(&reservation.current_certificate)
                .await
            {
                return Ok(Err(certificate::application_error(error)));
            }
            Ok(Ok(RotateNodeCertificateResult {
                certificate: replacement,
                replayed,
            }))
        })
    }
}

fn deterministic_certificate_id(
    node_id: Uuid,
    current_certificate_id: Uuid,
    request_digest: &str,
) -> NodeCertificateId {
    let digest = Sha256::digest(
        [
            node_id.as_bytes().as_slice(),
            current_certificate_id.as_bytes().as_slice(),
            request_digest.as_bytes(),
        ]
        .concat(),
    );
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    NodeCertificateId::from_uuid(Uuid::from_bytes(bytes))
}
