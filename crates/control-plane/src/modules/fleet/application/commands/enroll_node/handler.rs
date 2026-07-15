use super::{EnrollNode, EnrollNodeResult};
use crate::modules::fleet::application::certificate;
use crate::modules::fleet::domain::events::NodeEnrolled;
use crate::modules::fleet::domain::repositories::{INodeRepository, NodeEnrollmentDraft};
use crate::modules::fleet::domain::services::{ICertificateAuthority, NodeCertificateRequest};
use crate::modules::fleet::domain::value_objects::{
    EnrollmentTokenCredential, NodeCapabilities, NodeName,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{NodeCertificateId, NodeId};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use a3s_cloud_contracts::NodeEnrollmentResponse;
use chrono::Duration;
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub struct EnrollNodeHandler {
    nodes: Arc<dyn INodeRepository>,
    certificate_authority: Arc<dyn ICertificateAuthority>,
    certificate_ttl: Duration,
    certificate_rotation_window_ms: u64,
    heartbeat_interval_ms: u64,
    command_long_poll_ms: u64,
}

impl EnrollNodeHandler {
    pub fn new(
        nodes: Arc<dyn INodeRepository>,
        certificate_authority: Arc<dyn ICertificateAuthority>,
        certificate_ttl: Duration,
        certificate_rotation_window_ms: u64,
        heartbeat_interval_ms: u64,
        command_long_poll_ms: u64,
    ) -> Result<Self, String> {
        let certificate_ttl_ms = u64::try_from(certificate_ttl.num_milliseconds())
            .map_err(|_| "node certificate TTL exceeds the supported range")?;
        if certificate_ttl <= Duration::zero()
            || certificate_rotation_window_ms == 0
            || certificate_rotation_window_ms >= certificate_ttl_ms
            || heartbeat_interval_ms == 0
            || command_long_poll_ms == 0
            || command_long_poll_ms > 60_000
        {
            return Err("node enrollment timing policy is invalid".into());
        }
        Ok(Self {
            nodes,
            certificate_authority,
            certificate_ttl,
            certificate_rotation_window_ms,
            heartbeat_interval_ms,
            command_long_poll_ms,
        })
    }
}

impl CommandHandler<EnrollNode> for EnrollNodeHandler {
    fn execute(
        &self,
        command: EnrollNode,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<EnrollNodeResult>>> {
        let nodes = Arc::clone(&self.nodes);
        let certificate_authority = Arc::clone(&self.certificate_authority);
        let certificate_ttl = self.certificate_ttl;
        let certificate_rotation_window_ms = self.certificate_rotation_window_ms;
        let heartbeat_interval_ms = self.heartbeat_interval_ms;
        let command_long_poll_ms = self.command_long_poll_ms;
        Box::pin(async move {
            if let Err(error) = command.request.validate() {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            let credential =
                match EnrollmentTokenCredential::from_secret(&command.request.enrollment_token) {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                };
            let name = match NodeName::new(command.request.node_name.clone()) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let capabilities_document = serde_json::to_value(&command.request.runtime_capabilities)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let capabilities = match NodeCapabilities::new(
                command.request.runtime_capabilities.provider_id.clone(),
                command.request.runtime_capabilities.provider_build.clone(),
                capabilities_document,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "schema": command.request.schema,
                "enrollmentTokenDigest": credential.digest(),
                "nodeName": name.value(),
                "agentInstanceId": command.request.agent_instance_id,
                "agentVersion": command.request.agent_version,
                "csrPem": command.request.csr_pem,
                "runtimeCapabilities": command.request.runtime_capabilities,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let request_digest = format!("sha256:{:x}", Sha256::digest(canonical));
            let reservation = match nodes
                .reserve_enrollment(
                    &credential,
                    NodeEnrollmentDraft {
                        proposed_node_id: NodeId::new(),
                        name,
                        agent_instance_id: command.request.agent_instance_id,
                        agent_version: command.request.agent_version.clone(),
                        capabilities,
                        request_digest: request_digest.clone(),
                        requested_at: command.received_at,
                    },
                )
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            if let Some(existing) = reservation.certificate {
                return response(
                    reservation.node.id,
                    &existing,
                    heartbeat_interval_ms,
                    command_long_poll_ms,
                    certificate_rotation_window_ms,
                    true,
                );
            }
            let certificate = match certificate_authority
                .issue(NodeCertificateRequest {
                    certificate_id: NodeCertificateId::from_uuid(reservation.node.id.as_uuid()),
                    node_id: reservation.node.id,
                    csr_pem: command.request.csr_pem,
                    issued_at: reservation.node.enrolled_at,
                    expires_at: reservation.node.enrolled_at + certificate_ttl,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(certificate::application_error(error))),
            };
            let event = NodeEnrolled::envelope(&reservation.node, &certificate, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let completed = match nodes
                .complete_enrollment(
                    reservation.enrollment_token.id,
                    reservation.node.id,
                    &request_digest,
                    certificate,
                    event,
                )
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            let certificate = completed.certificate.ok_or_else(|| {
                BootError::Internal("completed node enrollment has no certificate".into())
            })?;
            response(
                completed.node.id,
                &certificate,
                heartbeat_interval_ms,
                command_long_poll_ms,
                certificate_rotation_window_ms,
                completed.replayed,
            )
        })
    }
}

fn response(
    node_id: NodeId,
    certificate_value: &crate::modules::fleet::domain::entities::NodeCertificate,
    heartbeat_interval_ms: u64,
    command_long_poll_ms: u64,
    certificate_rotation_window_ms: u64,
    replayed: bool,
) -> a3s_boot::Result<ApplicationResult<EnrollNodeResult>> {
    let response = NodeEnrollmentResponse {
        schema: NodeEnrollmentResponse::SCHEMA.into(),
        node_id: node_id.as_uuid(),
        certificate: certificate::contract(certificate_value),
        heartbeat_interval_ms,
        command_long_poll_ms,
        certificate_rotation_window_ms,
    };
    response
        .validate()
        .map_err(|error| BootError::Internal(format!("invalid enrollment response: {error}")))?;
    Ok(Ok(EnrollNodeResult { response, replayed }))
}
