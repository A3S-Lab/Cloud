use crate::modules::identity::domain::entities::{
    AcceptedWorkloadIdentityPolicyRevision, WorkloadRuntimeEvidenceCandidate,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, InstallationId, NodePoolId, OrganizationId, ProjectId,
    RepositoryError, ResourceClaimId, Sha256Digest, WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{RuntimeIsolationLevel, RuntimeUnitClass};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Identity-owned request for one normalized owner-evidence candidate. It is
/// derived from an already accepted policy revision and contains no foreign
/// aggregate, provider document, credential, or mutable lifecycle state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadRuntimeEvidenceRequest {
    installation_id: InstallationId,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    workload_id: WorkloadId,
    workload_revision_id: WorkloadRevisionId,
    resource_claim_id: ResourceClaimId,
    node_pool_id: NodePoolId,
    runtime_class: RuntimeUnitClass,
    isolation_level: RuntimeIsolationLevel,
    semantics_profile_digest: Sha256Digest,
    identity_attachment_digest: Sha256Digest,
    evaluated_at: DateTime<Utc>,
}

impl WorkloadRuntimeEvidenceRequest {
    pub fn for_policy(
        policy: &AcceptedWorkloadIdentityPolicyRevision,
        resource_claim_id: ResourceClaimId,
        evaluated_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        policy.validate()?;
        let spec = policy.contract.spec();
        let value = Self {
            installation_id: spec.installation_id,
            organization_id: spec.organization_id,
            project_id: spec.project_id,
            environment_id: spec.environment_id,
            workload_id: spec.workload_id,
            workload_revision_id: spec.workload_revision_id,
            resource_claim_id,
            node_pool_id: spec.node_pool_id,
            runtime_class: spec.runtime_class,
            isolation_level: spec.isolation_level,
            semantics_profile_digest: spec.semantics_profile_digest.clone(),
            identity_attachment_digest: policy.contract.digest().clone(),
            evaluated_at: canonical_timestamp(evaluated_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.installation_id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.resource_claim_id.as_uuid().is_nil()
            || self.node_pool_id.as_uuid().is_nil()
            || self.evaluated_at != canonical_timestamp(self.evaluated_at)
            || Sha256Digest::parse(self.semantics_profile_digest.as_str())?
                != self.semantics_profile_digest
            || Sha256Digest::parse(self.identity_attachment_digest.as_str())?
                != self.identity_attachment_digest
        {
            return Err("workload Runtime evidence request identity or digest is invalid".into());
        }
        Ok(())
    }

    pub fn validate_candidate(
        &self,
        candidate: &WorkloadRuntimeEvidenceCandidate,
    ) -> Result<(), String> {
        self.validate()?;
        candidate.validate()?;
        if candidate.installation_id != self.installation_id
            || candidate.organization_id != self.organization_id
            || candidate.project_id != self.project_id
            || candidate.environment_id != self.environment_id
            || candidate.workload_id != self.workload_id
            || candidate.workload_revision_id != self.workload_revision_id
            || candidate.resource_claim_id != self.resource_claim_id
            || candidate.node_pool_id != self.node_pool_id
            || candidate.runtime_class != self.runtime_class
            || candidate.isolation_level != self.isolation_level
            || candidate.semantics_profile_digest != self.semantics_profile_digest
            || candidate.identity_attachment_digest != self.identity_attachment_digest
        {
            return Err(
                "workload Runtime evidence candidate changed the requested owner or execution binding"
                    .into(),
            );
        }
        Ok(())
    }

    pub const fn installation_id(&self) -> InstallationId {
        self.installation_id
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn environment_id(&self) -> EnvironmentId {
        self.environment_id
    }

    pub const fn workload_id(&self) -> WorkloadId {
        self.workload_id
    }

    pub const fn workload_revision_id(&self) -> WorkloadRevisionId {
        self.workload_revision_id
    }

    pub const fn resource_claim_id(&self) -> ResourceClaimId {
        self.resource_claim_id
    }

    pub const fn node_pool_id(&self) -> NodePoolId {
        self.node_pool_id
    }

    pub const fn runtime_class(&self) -> RuntimeUnitClass {
        self.runtime_class
    }

    pub const fn isolation_level(&self) -> RuntimeIsolationLevel {
        self.isolation_level
    }

    pub const fn semantics_profile_digest(&self) -> &Sha256Digest {
        &self.semantics_profile_digest
    }

    pub const fn identity_attachment_digest(&self) -> &Sha256Digest {
        &self.identity_attachment_digest
    }

    pub const fn evaluated_at(&self) -> DateTime<Utc> {
        self.evaluated_at
    }
}

#[async_trait]
pub trait IWorkloadRuntimeEvidenceCandidatePort: Send + Sync {
    async fn read_candidate(
        &self,
        request: WorkloadRuntimeEvidenceRequest,
    ) -> Result<WorkloadRuntimeEvidenceCandidate, RepositoryError>;
}
