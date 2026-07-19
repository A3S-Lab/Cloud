use super::{VerifyDomainClaim, VerifyDomainClaimResult};
use crate::modules::edge::domain::events::DomainClaimChanged;
use crate::modules::edge::domain::repositories::{IEdgeRepository, TransitionDomainClaim};
use crate::modules::edge::domain::services::{
    DomainOwnershipVerificationError, DomainOwnershipVerificationRequest, IDomainOwnershipVerifier,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct VerifyDomainClaimHandler {
    edge: Arc<dyn IEdgeRepository>,
    verifier: Arc<dyn IDomainOwnershipVerifier>,
}

impl VerifyDomainClaimHandler {
    pub fn new(
        edge: Arc<dyn IEdgeRepository>,
        verifier: Arc<dyn IDomainOwnershipVerifier>,
    ) -> Self {
        Self { edge, verifier }
    }
}

impl CommandHandler<VerifyDomainClaim> for VerifyDomainClaimHandler {
    fn execute(
        &self,
        command: VerifyDomainClaim,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<VerifyDomainClaimResult>>>
    {
        let edge = Arc::clone(&self.edge);
        let verifier = Arc::clone(&self.verifier);
        Box::pin(async move {
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organization_id": command.organization_id,
                "claim_id": command.claim_id,
                "proof": command.proof,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/domain-claims/{}/verify",
                    command.organization_id, command.claim_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match edge.replay_domain_claim_write(&idempotency).await {
                Ok(Some(claim)) => {
                    return Ok(Ok(VerifyDomainClaimResult {
                        claim,
                        replayed: true,
                    }))
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let mut claim = match edge
                .find_domain_claim(command.organization_id, command.claim_id)
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            let expected_version = claim.aggregate_version;
            let verification = verifier
                .verify(DomainOwnershipVerificationRequest {
                    claim_id: claim.id,
                    pattern: claim.pattern.clone(),
                    challenge_dns_name: claim.challenge_dns_name.clone(),
                    expected_value: claim.challenge_value.clone(),
                    presented_proof: command.proof,
                })
                .await;
            match verification {
                Ok(()) => {
                    if let Err(error) = claim.verify(command.requested_at) {
                        return Ok(Err(ApplicationError::Conflict(error)));
                    }
                }
                Err(DomainOwnershipVerificationError::Rejected(error)) => {
                    if let Err(error) = claim.reject(error, command.requested_at) {
                        return Ok(Err(ApplicationError::Conflict(error)));
                    }
                }
                Err(DomainOwnershipVerificationError::Invalid(error)) => {
                    return Ok(Err(ApplicationError::Invalid(error)))
                }
                Err(DomainOwnershipVerificationError::Unavailable(error)) => {
                    return Ok(Err(ApplicationError::Internal(error)))
                }
            }
            let event = DomainClaimChanged::envelope(&claim, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let write = match edge
                .transition_domain_claim(TransitionDomainClaim {
                    claim,
                    expected_version,
                    idempotency,
                    event,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(VerifyDomainClaimResult {
                claim: write.value,
                replayed: write.replayed,
            }))
        })
    }
}
