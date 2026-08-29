use crate::modules::identity::domain::entities::{
    AcceptedPlatformRolePolicyRevision, AcceptedTrustDomainRevision,
    AcceptedWorkloadIdentityPolicyRevision, PlatformRoleBinding, TenantSupportGrant,
    TenantSupportGrantApprovalOutcome, TenantSupportGrantProposal,
};
use crate::modules::identity::domain::repositories::IIdentityBootstrapRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, InstallationId};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct PlatformRolePolicyMutationResult {
    pub policy: AcceptedPlatformRolePolicyRevision,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlatformRoleBindingMutationResult {
    pub binding: PlatformRoleBinding,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TenantSupportGrantProposalMutationResult {
    pub proposal: TenantSupportGrantProposal,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TenantSupportGrantApprovalMutationResult {
    pub outcome: TenantSupportGrantApprovalOutcome,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TenantSupportGrantMutationResult {
    pub grant: TenantSupportGrant,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrustDomainRevisionMutationResult {
    pub revision: AcceptedTrustDomainRevision,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkloadIdentityPolicyRevisionMutationResult {
    pub revision: AcceptedWorkloadIdentityPolicyRevision,
    pub replayed: bool,
}

pub(super) async fn installation_id(
    bootstrap: &Arc<dyn IIdentityBootstrapRepository>,
) -> ApplicationResult<InstallationId> {
    bootstrap.installation_id().await.map_err(Into::into)
}

pub(super) fn idempotency<T: Serialize>(
    scope: String,
    key: String,
    intent: &T,
) -> ApplicationResult<IdempotencyRequest> {
    let canonical = serde_json::to_vec(intent).map_err(|error| {
        ApplicationError::Internal(format!(
            "privileged management intent could not be encoded: {error}"
        ))
    })?;
    IdempotencyRequest::new(scope, key, &canonical).map_err(ApplicationError::Invalid)
}

pub(super) fn deterministic_id(
    installation_id: InstallationId,
    purpose: &'static str,
    idempotency: &IdempotencyRequest,
) -> Uuid {
    let mut identity = Vec::with_capacity(
        purpose.len()
            + idempotency.scope.len()
            + idempotency.key.len()
            + idempotency.request_digest.len()
            + 3,
    );
    for component in [
        purpose,
        idempotency.scope.as_str(),
        idempotency.key.as_str(),
        idempotency.request_digest.as_str(),
    ] {
        identity.extend_from_slice(component.as_bytes());
        identity.push(0);
    }
    Uuid::new_v5(&installation_id.as_uuid(), &identity)
}

pub(super) fn not_found(label: &'static str) -> ApplicationError {
    ApplicationError::NotFound(format!("{label} not found"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_identity_is_stable_and_intent_bound() {
        let installation_id = InstallationId::new();
        let first = idempotency(
            "installation/platform-role-bindings".into(),
            "stable-key".into(),
            &serde_json::json!({"principalId": Uuid::now_v7(), "role": "platform_operator"}),
        )
        .expect("idempotency");
        let replay = first.clone();
        let changed = idempotency(
            "installation/platform-role-bindings".into(),
            "stable-key".into(),
            &serde_json::json!({"principalId": Uuid::now_v7(), "role": "platform_operator"}),
        )
        .expect("changed");

        assert_eq!(
            deterministic_id(installation_id, "platform-role-binding", &first),
            deterministic_id(installation_id, "platform-role-binding", &replay)
        );
        assert_ne!(
            deterministic_id(installation_id, "platform-role-binding", &first),
            deterministic_id(installation_id, "platform-role-binding", &changed)
        );
    }
}
