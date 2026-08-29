use crate::modules::identity::domain::entities::{
    AcceptedTrustDomainRevision, AcceptedWorkloadIdentityPolicyRevision,
};
use crate::modules::shared_kernel::domain::{
    ApiTokenId, IdempotencyRequest, IdempotentWrite, InstallationId, OrganizationId, PrincipalId,
    RepositoryError, TrustDomainId, TrustDomainRevisionId, WorkloadId, WorkloadIdentityPolicyId,
    WorkloadIdentityPolicyRevisionId,
};
use async_trait::async_trait;
use uuid::Uuid;

pub const MAX_WORKLOAD_IDENTITY_REVISIONS_PAGE: usize = 100;

#[derive(Debug, Clone)]
pub struct AcceptTrustDomainRevisionWrite {
    pub revision: AcceptedTrustDomainRevision,
    pub expected_previous_revision_id: Option<TrustDomainRevisionId>,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl AcceptTrustDomainRevisionWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.revision.validate()?;
        self.idempotency.validate()?;
        if self.revision.accepted_by != self.actor_principal_id
            || self.credential_id.as_uuid().is_nil()
            || self.request_id.is_nil()
        {
            return Err("trust-domain acceptance actor or request identity is invalid".into());
        }
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
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl AcceptWorkloadIdentityPolicyRevisionWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.revision.validate()?;
        self.idempotency.validate()?;
        if self.revision.accepted_by != self.actor_principal_id
            || self.credential_id.as_uuid().is_nil()
            || self.request_id.is_nil()
        {
            return Err(
                "workload identity policy acceptance actor or request identity is invalid".into(),
            );
        }
        validate_previous_revision(
            self.revision.revision_number,
            self.revision.id.as_uuid(),
            self.expected_previous_revision_id
                .map(WorkloadIdentityPolicyRevisionId::as_uuid),
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ReadTrustDomainRevision {
    pub installation_id: InstallationId,
    pub trust_domain_id: TrustDomainId,
    pub revision_id: TrustDomainRevisionId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl ReadTrustDomainRevision {
    pub fn validate(&self) -> Result<(), String> {
        validate_read_context(
            &[
                self.installation_id.as_uuid(),
                self.trust_domain_id.as_uuid(),
                self.revision_id.as_uuid(),
            ],
            self.actor_principal_id,
            self.credential_id,
            self.request_id,
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ReadCurrentTrustDomain {
    pub installation_id: InstallationId,
    pub trust_domain_id: TrustDomainId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl ReadCurrentTrustDomain {
    pub fn validate(&self) -> Result<(), String> {
        validate_read_context(
            &[
                self.installation_id.as_uuid(),
                self.trust_domain_id.as_uuid(),
            ],
            self.actor_principal_id,
            self.credential_id,
            self.request_id,
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ListTrustDomainRevisions {
    pub installation_id: InstallationId,
    pub trust_domain_id: TrustDomainId,
    pub limit: usize,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl ListTrustDomainRevisions {
    pub fn validate(&self) -> Result<(), String> {
        validate_read_context(
            &[
                self.installation_id.as_uuid(),
                self.trust_domain_id.as_uuid(),
            ],
            self.actor_principal_id,
            self.credential_id,
            self.request_id,
        )?;
        validate_limit(self.limit)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ReadWorkloadIdentityPolicyRevision {
    pub installation_id: InstallationId,
    pub organization_id: OrganizationId,
    pub policy_id: WorkloadIdentityPolicyId,
    pub revision_id: WorkloadIdentityPolicyRevisionId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl ReadWorkloadIdentityPolicyRevision {
    pub fn validate(&self) -> Result<(), String> {
        validate_read_context(
            &[
                self.installation_id.as_uuid(),
                self.organization_id.as_uuid(),
                self.policy_id.as_uuid(),
                self.revision_id.as_uuid(),
            ],
            self.actor_principal_id,
            self.credential_id,
            self.request_id,
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ReadCurrentWorkloadIdentityPolicy {
    pub installation_id: InstallationId,
    pub organization_id: OrganizationId,
    pub policy_id: WorkloadIdentityPolicyId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl ReadCurrentWorkloadIdentityPolicy {
    pub fn validate(&self) -> Result<(), String> {
        validate_read_context(
            &[
                self.installation_id.as_uuid(),
                self.organization_id.as_uuid(),
                self.policy_id.as_uuid(),
            ],
            self.actor_principal_id,
            self.credential_id,
            self.request_id,
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ReadCurrentWorkloadIdentityPolicyForWorkload {
    pub installation_id: InstallationId,
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl ReadCurrentWorkloadIdentityPolicyForWorkload {
    pub fn validate(&self) -> Result<(), String> {
        validate_read_context(
            &[
                self.installation_id.as_uuid(),
                self.organization_id.as_uuid(),
                self.workload_id.as_uuid(),
            ],
            self.actor_principal_id,
            self.credential_id,
            self.request_id,
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ListWorkloadIdentityPolicyRevisions {
    pub installation_id: InstallationId,
    pub organization_id: OrganizationId,
    pub policy_id: WorkloadIdentityPolicyId,
    pub limit: usize,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl ListWorkloadIdentityPolicyRevisions {
    pub fn validate(&self) -> Result<(), String> {
        validate_read_context(
            &[
                self.installation_id.as_uuid(),
                self.organization_id.as_uuid(),
                self.policy_id.as_uuid(),
            ],
            self.actor_principal_id,
            self.credential_id,
            self.request_id,
        )?;
        validate_limit(self.limit)
    }
}

#[async_trait]
pub trait ITrustDomainRepository: Send + Sync {
    async fn accept(
        &self,
        write: AcceptTrustDomainRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedTrustDomainRevision>, RepositoryError>;

    async fn read_revision(
        &self,
        read: ReadTrustDomainRevision,
    ) -> Result<Option<AcceptedTrustDomainRevision>, RepositoryError>;

    async fn read_current(
        &self,
        read: ReadCurrentTrustDomain,
    ) -> Result<Option<AcceptedTrustDomainRevision>, RepositoryError>;

    async fn list_revisions(
        &self,
        read: ListTrustDomainRevisions,
    ) -> Result<Vec<AcceptedTrustDomainRevision>, RepositoryError>;
}

#[async_trait]
pub trait IWorkloadIdentityPolicyRepository: Send + Sync {
    async fn accept(
        &self,
        write: AcceptWorkloadIdentityPolicyRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError>;

    async fn read_revision(
        &self,
        read: ReadWorkloadIdentityPolicyRevision,
    ) -> Result<Option<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError>;

    async fn read_current(
        &self,
        read: ReadCurrentWorkloadIdentityPolicy,
    ) -> Result<Option<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError>;

    async fn read_current_for_workload(
        &self,
        read: ReadCurrentWorkloadIdentityPolicyForWorkload,
    ) -> Result<Option<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError>;

    async fn list_revisions(
        &self,
        read: ListWorkloadIdentityPolicyRevisions,
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

fn validate_read_context(
    resource_ids: &[Uuid],
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    request_id: Uuid,
) -> Result<(), String> {
    if resource_ids.iter().any(Uuid::is_nil)
        || actor_principal_id.as_uuid().is_nil()
        || credential_id.as_uuid().is_nil()
        || request_id.is_nil()
    {
        return Err("workload trust read authority context is invalid".into());
    }
    Ok(())
}

fn validate_limit(limit: usize) -> Result<(), String> {
    if !(1..=MAX_WORKLOAD_IDENTITY_REVISIONS_PAGE).contains(&limit) {
        return Err("workload identity revision page limit is outside bounds".into());
    }
    Ok(())
}
