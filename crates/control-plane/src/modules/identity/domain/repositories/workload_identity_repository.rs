use crate::modules::identity::domain::entities::{
    AcceptedTrustDomainRevision, AcceptedWorkloadIdentityPolicyRevision,
};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, InstallationId, OrganizationId, RepositoryError,
    TrustDomainId, TrustDomainRevisionId, WorkloadId, WorkloadIdentityPolicyId,
    WorkloadIdentityPolicyRevisionId,
};
use async_trait::async_trait;

pub const MAX_WORKLOAD_IDENTITY_REVISIONS_PAGE: usize = 100;

#[derive(Debug, Clone)]
pub struct AcceptTrustDomainRevisionWrite {
    pub revision: AcceptedTrustDomainRevision,
    pub expected_previous_revision_id: Option<TrustDomainRevisionId>,
    pub idempotency: IdempotencyRequest,
}

impl AcceptTrustDomainRevisionWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.revision.validate()?;
        self.idempotency.validate()?;
        validate_previous_revision(
            self.revision.revision_number,
            self.revision.id.as_uuid(),
            self.expected_previous_revision_id
                .map(TrustDomainRevisionId::as_uuid),
        )
    }
}

#[derive(Debug, Clone)]
pub struct AcceptWorkloadIdentityPolicyRevisionWrite {
    pub revision: AcceptedWorkloadIdentityPolicyRevision,
    pub expected_previous_revision_id: Option<WorkloadIdentityPolicyRevisionId>,
    pub idempotency: IdempotencyRequest,
}

impl AcceptWorkloadIdentityPolicyRevisionWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.revision.validate()?;
        self.idempotency.validate()?;
        validate_previous_revision(
            self.revision.revision_number,
            self.revision.id.as_uuid(),
            self.expected_previous_revision_id
                .map(WorkloadIdentityPolicyRevisionId::as_uuid),
        )
    }
}

#[async_trait]
pub trait ITrustDomainRepository: Send + Sync {
    async fn accept(
        &self,
        write: AcceptTrustDomainRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedTrustDomainRevision>, RepositoryError>;

    async fn find_revision(
        &self,
        installation_id: InstallationId,
        trust_domain_id: TrustDomainId,
        revision_id: TrustDomainRevisionId,
    ) -> Result<Option<AcceptedTrustDomainRevision>, RepositoryError>;

    async fn find_current(
        &self,
        installation_id: InstallationId,
        trust_domain_id: TrustDomainId,
    ) -> Result<Option<AcceptedTrustDomainRevision>, RepositoryError>;

    async fn list_revisions(
        &self,
        installation_id: InstallationId,
        trust_domain_id: TrustDomainId,
        limit: usize,
    ) -> Result<Vec<AcceptedTrustDomainRevision>, RepositoryError>;
}

#[async_trait]
pub trait IWorkloadIdentityPolicyRepository: Send + Sync {
    async fn accept(
        &self,
        write: AcceptWorkloadIdentityPolicyRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError>;

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        policy_id: WorkloadIdentityPolicyId,
        revision_id: WorkloadIdentityPolicyRevisionId,
    ) -> Result<Option<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError>;

    async fn find_current(
        &self,
        organization_id: OrganizationId,
        policy_id: WorkloadIdentityPolicyId,
    ) -> Result<Option<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError>;

    async fn find_current_for_workload(
        &self,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
    ) -> Result<Option<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError>;

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        policy_id: WorkloadIdentityPolicyId,
        limit: usize,
    ) -> Result<Vec<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError>;
}

fn validate_previous_revision(
    revision_number: u64,
    revision_id: uuid::Uuid,
    expected_previous_revision_id: Option<uuid::Uuid>,
) -> Result<(), String> {
    let valid = match revision_number {
        1 => expected_previous_revision_id.is_none(),
        _ => expected_previous_revision_id
            .is_some_and(|previous| !previous.is_nil() && previous != revision_id),
    };
    if !valid {
        return Err("identity revision predecessor fence is invalid".into());
    }
    Ok(())
}
