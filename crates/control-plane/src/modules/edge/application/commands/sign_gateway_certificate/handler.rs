use super::SignGatewayCertificate;
use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::domain::services::{
    GatewayCertificateAuthorityError, GatewayCertificateIssueRequest, IGatewayCertificateAuthority,
};
use crate::modules::edge::domain::{GatewayCertificate, GatewayCertificateState};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{canonical_timestamp, GatewayCertificateId};
use a3s_boot::{CommandHandler, CqrsContext};
use a3s_cloud_contracts::GatewayCertificateSigningResponse;
use chrono::Duration;
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub struct SignGatewayCertificateHandler {
    certificates: Arc<dyn IEdgeRepository>,
    certificate_authority: Arc<dyn IGatewayCertificateAuthority>,
    certificate_ttl: Duration,
}

impl SignGatewayCertificateHandler {
    pub fn new(
        certificates: Arc<dyn IEdgeRepository>,
        certificate_authority: Arc<dyn IGatewayCertificateAuthority>,
        certificate_ttl: Duration,
    ) -> Result<Self, String> {
        if certificate_ttl <= Duration::zero() {
            return Err("Gateway certificate TTL must be positive".into());
        }
        Ok(Self {
            certificates,
            certificate_authority,
            certificate_ttl,
        })
    }
}

impl CommandHandler<SignGatewayCertificate> for SignGatewayCertificateHandler {
    fn execute(
        &self,
        command: SignGatewayCertificate,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<GatewayCertificateSigningResponse>>,
    > {
        let certificates = Arc::clone(&self.certificates);
        let certificate_authority = Arc::clone(&self.certificate_authority);
        let certificate_ttl = self.certificate_ttl;
        Box::pin(async move {
            if let Err(error) = command.request.validate() {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            if command.request.node_id != command.authenticated_node_id.as_uuid() {
                return Ok(Err(ApplicationError::Forbidden(
                    "authenticated certificate does not belong to the Gateway certificate request"
                        .into(),
                )));
            }
            let received_at = canonical_timestamp(command.received_at);
            let certificate_id = GatewayCertificateId::from_uuid(command.request.certificate_id);
            let certificate = match certificates
                .find_gateway_certificate(command.authenticated_node_id, certificate_id)
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            if received_at < certificate.created_at {
                return Ok(Err(ApplicationError::Conflict(
                    "Gateway certificate request predates its desired state".into(),
                )));
            }
            let csr_digest = format!(
                "sha256:{:x}",
                Sha256::digest(command.request.csr_pem.as_bytes())
            );
            match replay_response(&certificate, &csr_digest) {
                Ok(Some(response)) => return Ok(Ok(response)),
                Ok(None) => {}
                Err(error) => return Ok(Err(error)),
            }

            let material = match certificate_authority
                .issue(GatewayCertificateIssueRequest {
                    certificate_id: certificate.id,
                    node_id: certificate.node_id,
                    dns_names: certificate.request.dns_names.clone(),
                    csr_pem: command.request.csr_pem,
                    issued_at: received_at,
                    expires_at: received_at + certificate_ttl,
                })
                .await
            {
                Ok(material) => material,
                Err(error) => {
                    let application_error = authority_error(&error);
                    let mut failed = certificate;
                    let expected_version = failed.aggregate_version;
                    let failure = authority_failure(&error);
                    if let Err(transition_error) =
                        failed.fail_provisioning(csr_digest, failure, received_at)
                    {
                        return Ok(Err(ApplicationError::Conflict(transition_error)));
                    }
                    if let Err(repository_error) = certificates
                        .transition_gateway_certificate(failed, expected_version)
                        .await
                    {
                        return Ok(Err(repository_error.into()));
                    }
                    return Ok(Err(application_error));
                }
            };
            let mut issued = certificate;
            let expected_version = issued.aggregate_version;
            if let Err(error) = issued.record_issued(csr_digest.clone(), material, received_at) {
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            let issued = match certificates
                .transition_gateway_certificate(issued, expected_version)
                .await
            {
                Ok(value) => value,
                Err(crate::modules::shared_kernel::domain::RepositoryError::Conflict(_)) => {
                    let current = match certificates
                        .find_gateway_certificate(command.authenticated_node_id, certificate_id)
                        .await
                    {
                        Ok(value) => value,
                        Err(error) => return Ok(Err(error.into())),
                    };
                    match replay_response(&current, &csr_digest) {
                        Ok(Some(response)) => return Ok(Ok(response)),
                        Ok(None) => {
                            return Ok(Err(ApplicationError::Conflict(
                                "Gateway certificate signing raced with another transition".into(),
                            )))
                        }
                        Err(error) => return Ok(Err(error)),
                    }
                }
                Err(error) => return Ok(Err(error.into())),
            };
            match signing_response(&issued) {
                Ok(response) => Ok(Ok(response)),
                Err(error) => Ok(Err(error)),
            }
        })
    }
}

fn replay_response(
    certificate: &GatewayCertificate,
    csr_digest: &str,
) -> Result<Option<GatewayCertificateSigningResponse>, ApplicationError> {
    match certificate.state {
        GatewayCertificateState::Provisioning => {
            if certificate
                .csr_digest
                .as_deref()
                .is_some_and(|stored| stored != csr_digest)
            {
                Err(ApplicationError::Conflict(
                    "Gateway certificate is already bound to another CSR".into(),
                ))
            } else {
                Ok(None)
            }
        }
        GatewayCertificateState::Issued | GatewayCertificateState::Ready => {
            if certificate.csr_digest.as_deref() != Some(csr_digest) {
                return Err(ApplicationError::Conflict(
                    "Gateway certificate was issued for another CSR".into(),
                ));
            }
            signing_response(certificate).map(Some)
        }
        GatewayCertificateState::Failed => {
            let message = if certificate.csr_digest.as_deref() == Some(csr_digest) {
                "Gateway certificate provisioning previously failed"
            } else {
                "Gateway certificate is bound to another CSR"
            };
            Err(ApplicationError::Conflict(message.into()))
        }
        GatewayCertificateState::Revoked => Err(ApplicationError::Forbidden(
            "Gateway certificate has been revoked".into(),
        )),
    }
}

fn signing_response(
    certificate: &GatewayCertificate,
) -> Result<GatewayCertificateSigningResponse, ApplicationError> {
    let material = certificate.material.as_ref().ok_or_else(|| {
        ApplicationError::Internal("issued Gateway certificate has no public material".into())
    })?;
    let response = GatewayCertificateSigningResponse {
        schema: GatewayCertificateSigningResponse::SCHEMA.into(),
        certificate_id: certificate.id.as_uuid(),
        node_id: certificate.node_id.as_uuid(),
        dns_names: certificate.request.dns_names.clone(),
        serial_number: material.serial_number.clone(),
        fingerprint: material.fingerprint.clone(),
        certificate_pem: material.certificate_pem.clone(),
        ca_bundle_pem: material.ca_bundle_pem.clone(),
        issued_at: material.issued_at,
        expires_at: material.expires_at,
    };
    response.validate().map_err(|error| {
        ApplicationError::Internal(format!(
            "stored Gateway certificate response is invalid: {error}"
        ))
    })?;
    Ok(response)
}

fn authority_error(error: &GatewayCertificateAuthorityError) -> ApplicationError {
    match error {
        GatewayCertificateAuthorityError::InvalidRequest(_) => {
            ApplicationError::Invalid("Gateway certificate CSR was rejected as invalid".into())
        }
        GatewayCertificateAuthorityError::Rejected(_) => {
            ApplicationError::Forbidden("Gateway certificate CSR was rejected".into())
        }
        GatewayCertificateAuthorityError::Unavailable(_) => {
            ApplicationError::Internal("Gateway certificate authority is unavailable".into())
        }
    }
}

fn authority_failure(error: &GatewayCertificateAuthorityError) -> &'static str {
    match error {
        GatewayCertificateAuthorityError::InvalidRequest(_) => {
            "Gateway certificate CSR was invalid"
        }
        GatewayCertificateAuthorityError::Rejected(_) => {
            "Gateway certificate authority rejected the CSR"
        }
        GatewayCertificateAuthorityError::Unavailable(_) => {
            "Gateway certificate authority was unavailable"
        }
    }
}
