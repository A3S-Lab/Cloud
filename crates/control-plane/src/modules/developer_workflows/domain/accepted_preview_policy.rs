use super::{PullRequestPreviewPolicyAuthority, PullRequestPreviewPolicyContract};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, OrganizationId, PrincipalId, ProjectId,
    PullRequestPreviewPolicyRevisionId, SourceSubscriptionId,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

const PREVIEW_POLICY_REVISION_NAMESPACE: Uuid = Uuid::from_bytes([
    0x0d, 0xd2, 0x47, 0x8b, 0xf8, 0x21, 0x44, 0x6c, 0xa1, 0x4b, 0x2f, 0xa4, 0x31, 0xb4, 0x76, 0x0e,
]);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedPullRequestPreviewPolicyRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    /// Existing source-subscription Environment used only as authorization and
    /// owner-binding evidence. Preview Environments are separate deterministic
    /// identities and remain Projects-owned.
    pub source_environment_id: EnvironmentId,
    pub source_subscription_id: SourceSubscriptionId,
    pub id: PullRequestPreviewPolicyRevisionId,
    pub revision_number: u64,
    pub contract: PullRequestPreviewPolicyContract,
    pub accepted_by: PrincipalId,
    pub accepted_at: DateTime<Utc>,
}

impl AcceptedPullRequestPreviewPolicyRevision {
    pub fn preview_authority(&self) -> Result<PullRequestPreviewPolicyAuthority, String> {
        self.validate()?;
        let authority = PullRequestPreviewPolicyAuthority {
            source_environment_id: self.source_environment_id,
            revision_id: self.id,
            revision_number: self.revision_number,
            accepted_at: self.accepted_at,
            policy: self.contract.policy().clone(),
        };
        authority.validate()?;
        Ok(authority)
    }

    pub fn revision_id_for(
        source_subscription_id: SourceSubscriptionId,
        revision_number: u64,
        contract: &PullRequestPreviewPolicyContract,
    ) -> Result<PullRequestPreviewPolicyRevisionId, String> {
        contract.validate()?;
        if source_subscription_id.as_uuid().is_nil()
            || revision_number == 0
            || revision_number > i64::MAX as u64
        {
            return Err("Preview policy revision identity is outside persistence bounds".into());
        }
        let mut identity = Vec::with_capacity(24 + contract.digest().as_str().len());
        identity.extend_from_slice(source_subscription_id.as_uuid().as_bytes());
        identity.extend_from_slice(&revision_number.to_be_bytes());
        identity.extend_from_slice(contract.digest().as_str().as_bytes());
        Ok(PullRequestPreviewPolicyRevisionId::from_uuid(Uuid::new_v5(
            &PREVIEW_POLICY_REVISION_NAMESPACE,
            &identity,
        )))
    }

    pub fn accept(
        source_environment_id: EnvironmentId,
        contract: PullRequestPreviewPolicyContract,
        revision_number: u64,
        accepted_by: PrincipalId,
        accepted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        contract.validate()?;
        let policy = contract.policy();
        let id = Self::revision_id_for(policy.source_subscription_id, revision_number, &contract)?;
        let value = Self {
            organization_id: policy.organization_id,
            project_id: policy.project_id,
            source_environment_id,
            source_subscription_id: policy.source_subscription_id,
            id,
            revision_number,
            contract,
            accepted_by,
            accepted_at: canonical_timestamp(accepted_at),
        };
        value.validate()?;
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
        id: PullRequestPreviewPolicyRevisionId,
        revision_number: u64,
        canonical_acl: &str,
        stored_digest: &str,
        accepted_by: PrincipalId,
        accepted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            project_id,
            source_environment_id,
            source_subscription_id,
            id,
            revision_number,
            contract: PullRequestPreviewPolicyContract::restore(canonical_acl, stored_digest)?,
            accepted_by,
            accepted_at: canonical_timestamp(accepted_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.contract.validate()?;
        let policy = self.contract.policy();
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.source_environment_id.as_uuid().is_nil()
            || self.source_subscription_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.accepted_by.as_uuid().is_nil()
            || self.revision_number == 0
            || self.revision_number > i64::MAX as u64
            || self.accepted_at != canonical_timestamp(self.accepted_at)
            || self.organization_id != policy.organization_id
            || self.project_id != policy.project_id
            || self.source_subscription_id != policy.source_subscription_id
            || self.id
                != Self::revision_id_for(
                    self.source_subscription_id,
                    self.revision_number,
                    &self.contract,
                )?
        {
            return Err("accepted Preview policy revision identity or state is invalid".into());
        }
        Ok(())
    }
}
