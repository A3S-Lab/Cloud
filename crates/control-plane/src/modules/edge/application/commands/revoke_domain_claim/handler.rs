use super::{RevokeDomainClaim, RevokeDomainClaimResult};
use crate::modules::edge::domain::events::DomainClaimChanged;
use crate::modules::edge::domain::repositories::{IEdgeRepository, TransitionDomainClaim};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct RevokeDomainClaimHandler {
    edge: Arc<dyn IEdgeRepository>,
}

impl RevokeDomainClaimHandler {
    pub fn new(edge: Arc<dyn IEdgeRepository>) -> Self {
        Self { edge }
    }
}

impl CommandHandler<RevokeDomainClaim> for RevokeDomainClaimHandler {
    fn execute(
        &self,
        command: RevokeDomainClaim,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<RevokeDomainClaimResult>>>
    {
        let edge = Arc::clone(&self.edge);
        Box::pin(async move {
            let reason = match bounded_reason(command.reason) {
                Ok(reason) => reason,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organization_id": command.organization_id,
                "claim_id": command.claim_id,
                "reason": reason,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/domain-claims/{}/revoke",
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
                    return Ok(Ok(RevokeDomainClaimResult {
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
            if let Err(error) = claim.revoke(reason, command.requested_at) {
                return Ok(Err(ApplicationError::Conflict(error)));
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
            Ok(Ok(RevokeDomainClaimResult {
                claim: write.value,
                replayed: write.replayed,
            }))
        })
    }
}

fn bounded_reason(reason: String) -> Result<String, String> {
    let reason = reason.replace(['\0', '\r', '\n'], " ");
    let reason = reason.trim();
    if reason.is_empty() || reason.len() > 4096 {
        return Err("domain claim revocation reason must be a bounded single-line value".into());
    }
    Ok(reason.into())
}
