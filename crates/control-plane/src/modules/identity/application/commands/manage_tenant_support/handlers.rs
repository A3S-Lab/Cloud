use super::{ApproveTenantSupportGrant, ProposeTenantSupportGrant, RevokeTenantSupportGrant};
use crate::modules::identity::application::privileged_management::{idempotency, installation_id};
use crate::modules::identity::application::{
    TenantSupportGrantApprovalMutationResult, TenantSupportGrantMutationResult,
    TenantSupportGrantProposalMutationResult,
};
use crate::modules::identity::domain::repositories::{
    ApproveTenantSupportGrantWrite, IIdentityBootstrapRepository, ITenantSupportGrantRepository,
    ProposeTenantSupportGrantWrite, RevokeTenantSupportGrantWrite,
};
use crate::modules::identity::domain::value_objects::TenantSupportGrantContract;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_boot::{CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct ProposeTenantSupportGrantHandler {
    bootstrap: Arc<dyn IIdentityBootstrapRepository>,
    repository: Arc<dyn ITenantSupportGrantRepository>,
}

impl ProposeTenantSupportGrantHandler {
    pub fn new(
        bootstrap: Arc<dyn IIdentityBootstrapRepository>,
        repository: Arc<dyn ITenantSupportGrantRepository>,
    ) -> Self {
        Self {
            bootstrap,
            repository,
        }
    }
}

impl CommandHandler<ProposeTenantSupportGrant> for ProposeTenantSupportGrantHandler {
    fn execute(
        &self,
        command: ProposeTenantSupportGrant,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<TenantSupportGrantProposalMutationResult>>,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let contract = match TenantSupportGrantContract::parse_acl(&command.canonical_acl) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            if contract.spec().installation_id() != installation_id {
                return Ok(Err(ApplicationError::Invalid(
                    "tenant support grant crossed the canonical Installation boundary".into(),
                )));
            }
            let idempotency = match idempotency(
                "installation/tenant-support-grants".into(),
                command.idempotency_key,
                &serde_json::json!({"canonicalAcl": contract.canonical_acl()}),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let result = match repository
                .propose_tenant_support_grant(ProposeTenantSupportGrantWrite {
                    contract,
                    actor_principal_id: command.actor_principal_id,
                    credential_id: command.credential_id,
                    requested_at: Utc::now(),
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(TenantSupportGrantProposalMutationResult {
                proposal: result.value,
                replayed: result.replayed,
            }))
        })
    }
}

pub struct ApproveTenantSupportGrantHandler {
    bootstrap: Arc<dyn IIdentityBootstrapRepository>,
    repository: Arc<dyn ITenantSupportGrantRepository>,
}

impl ApproveTenantSupportGrantHandler {
    pub fn new(
        bootstrap: Arc<dyn IIdentityBootstrapRepository>,
        repository: Arc<dyn ITenantSupportGrantRepository>,
    ) -> Self {
        Self {
            bootstrap,
            repository,
        }
    }
}

impl CommandHandler<ApproveTenantSupportGrant> for ApproveTenantSupportGrantHandler {
    fn execute(
        &self,
        command: ApproveTenantSupportGrant,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<TenantSupportGrantApprovalMutationResult>>,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let expected_contract_digest =
                match Sha256Digest::parse(command.expected_contract_digest) {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                };
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let idempotency = match idempotency(
                format!(
                    "installation/tenant-support-grants/{}/approvals",
                    command.grant_id
                ),
                command.idempotency_key,
                &serde_json::json!({"expectedContractDigest": expected_contract_digest}),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let result = match repository
                .approve_tenant_support_grant(ApproveTenantSupportGrantWrite {
                    installation_id,
                    grant_id: command.grant_id,
                    expected_contract_digest,
                    actor_principal_id: command.actor_principal_id,
                    credential_id: command.credential_id,
                    approved_at: Utc::now(),
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(TenantSupportGrantApprovalMutationResult {
                outcome: result.value,
                replayed: result.replayed,
            }))
        })
    }
}

pub struct RevokeTenantSupportGrantHandler {
    bootstrap: Arc<dyn IIdentityBootstrapRepository>,
    repository: Arc<dyn ITenantSupportGrantRepository>,
}

impl RevokeTenantSupportGrantHandler {
    pub fn new(
        bootstrap: Arc<dyn IIdentityBootstrapRepository>,
        repository: Arc<dyn ITenantSupportGrantRepository>,
    ) -> Self {
        Self {
            bootstrap,
            repository,
        }
    }
}

impl CommandHandler<RevokeTenantSupportGrant> for RevokeTenantSupportGrantHandler {
    fn execute(
        &self,
        command: RevokeTenantSupportGrant,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<TenantSupportGrantMutationResult>>,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            if command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected tenant support grant version must be positive".into(),
                )));
            }
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let idempotency = match idempotency(
                format!(
                    "installation/tenant-support-grants/{}/revoke",
                    command.grant_id
                ),
                command.idempotency_key,
                &serde_json::json!({"expectedVersion": command.expected_version}),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let result = match repository
                .revoke_tenant_support_grant(RevokeTenantSupportGrantWrite {
                    installation_id,
                    grant_id: command.grant_id,
                    expected_version: command.expected_version,
                    actor_principal_id: command.actor_principal_id,
                    credential_id: command.credential_id,
                    revoked_at: Utc::now(),
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(TenantSupportGrantMutationResult {
                grant: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
