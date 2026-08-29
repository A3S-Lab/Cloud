use super::{AcceptTrustDomainRevision, AcceptWorkloadIdentityPolicyRevision};
use crate::modules::identity::application::privileged_management::{idempotency, installation_id};
use crate::modules::identity::application::{
    TrustDomainRevisionMutationResult, WorkloadIdentityPolicyRevisionMutationResult,
};
use crate::modules::identity::domain::entities::{
    AcceptedTrustDomainRevision, AcceptedWorkloadIdentityPolicyRevision,
};
use crate::modules::identity::domain::repositories::{
    AcceptTrustDomainRevisionWrite, AcceptWorkloadIdentityPolicyRevisionWrite,
    IIdentityBootstrapRepository, ITrustDomainRepository, IWorkloadIdentityPolicyRepository,
};
use crate::modules::identity::domain::value_objects::{
    TrustDomainContract, WorkloadIdentityPolicyContract,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct AcceptTrustDomainRevisionHandler {
    bootstrap: Arc<dyn IIdentityBootstrapRepository>,
    repository: Arc<dyn ITrustDomainRepository>,
}

impl AcceptTrustDomainRevisionHandler {
    pub fn new(
        bootstrap: Arc<dyn IIdentityBootstrapRepository>,
        repository: Arc<dyn ITrustDomainRepository>,
    ) -> Self {
        Self {
            bootstrap,
            repository,
        }
    }
}

impl CommandHandler<AcceptTrustDomainRevision> for AcceptTrustDomainRevisionHandler {
    fn execute(
        &self,
        command: AcceptTrustDomainRevision,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<TrustDomainRevisionMutationResult>>,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let contract = match TrustDomainContract::parse_acl(&command.canonical_acl) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if contract.spec().installation_id != installation_id
                || contract.spec().trust_domain_id != command.trust_domain_id
            {
                return Ok(Err(ApplicationError::Invalid(
                    "trust-domain ACL crossed its canonical Installation or path identity".into(),
                )));
            }
            let revision = match AcceptedTrustDomainRevision::accept(
                contract,
                command.revision_number,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let idempotency = match idempotency(
                format!(
                    "installation/trust-domains/{}/revisions",
                    command.trust_domain_id
                ),
                command.idempotency_key,
                &serde_json::json!({
                    "canonicalAcl": revision.contract.canonical_acl(),
                    "revisionNumber": revision.revision_number,
                    "expectedPreviousRevisionId": command.expected_previous_revision_id,
                }),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            match repository
                .accept(AcceptTrustDomainRevisionWrite {
                    revision,
                    expected_previous_revision_id: command.expected_previous_revision_id,
                    actor_principal_id: command.actor_principal_id,
                    credential_id: command.credential_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(TrustDomainRevisionMutationResult {
                    revision: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

pub struct AcceptWorkloadIdentityPolicyRevisionHandler {
    bootstrap: Arc<dyn IIdentityBootstrapRepository>,
    repository: Arc<dyn IWorkloadIdentityPolicyRepository>,
}

impl AcceptWorkloadIdentityPolicyRevisionHandler {
    pub fn new(
        bootstrap: Arc<dyn IIdentityBootstrapRepository>,
        repository: Arc<dyn IWorkloadIdentityPolicyRepository>,
    ) -> Self {
        Self {
            bootstrap,
            repository,
        }
    }
}

impl CommandHandler<AcceptWorkloadIdentityPolicyRevision>
    for AcceptWorkloadIdentityPolicyRevisionHandler
{
    fn execute(
        &self,
        command: AcceptWorkloadIdentityPolicyRevision,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<WorkloadIdentityPolicyRevisionMutationResult>>,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let contract = match WorkloadIdentityPolicyContract::parse_acl(&command.canonical_acl) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let spec = contract.spec();
            if spec.installation_id != installation_id
                || spec.organization_id != command.organization_id
                || spec.policy_id != command.policy_id
            {
                return Ok(Err(ApplicationError::Invalid(
                    "workload identity policy ACL crossed its canonical Installation, Organization, or path identity".into(),
                )));
            }
            let revision = match AcceptedWorkloadIdentityPolicyRevision::accept(
                contract,
                command.revision_number,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let idempotency = match idempotency(
                format!(
                    "organizations/{}/workload-identity-policies/{}/revisions",
                    command.organization_id, command.policy_id
                ),
                command.idempotency_key,
                &serde_json::json!({
                    "canonicalAcl": revision.contract.canonical_acl(),
                    "revisionNumber": revision.revision_number,
                    "expectedPreviousRevisionId": command.expected_previous_revision_id,
                }),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            match repository
                .accept(AcceptWorkloadIdentityPolicyRevisionWrite {
                    revision,
                    expected_previous_revision_id: command.expected_previous_revision_id,
                    actor_principal_id: command.actor_principal_id,
                    credential_id: command.credential_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(WorkloadIdentityPolicyRevisionMutationResult {
                    revision: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
