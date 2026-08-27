use super::AcceptedBuildPlanContract;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, BuildPlanId, EnvironmentId, OrganizationId, PrincipalId, ProjectId,
    SourceRevisionId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use uuid::Uuid;

const BUILD_PLAN_NAMESPACE: Uuid = Uuid::from_bytes([
    0x49, 0x33, 0x69, 0xa1, 0x8e, 0xec, 0x4c, 0x90, 0x82, 0x50, 0x75, 0x16, 0x48, 0x42, 0x37, 0x5c,
]);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedBuildPlan {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub id: BuildPlanId,
    pub source_revision_id: SourceRevisionId,
    pub contract: AcceptedBuildPlanContract,
    pub aggregate_version: u64,
    pub accepted_by: PrincipalId,
    pub accepted_at: DateTime<Utc>,
}

impl AcceptedBuildPlan {
    pub fn id_for(
        organization_id: OrganizationId,
        source_revision_id: SourceRevisionId,
        project_root: &str,
    ) -> BuildPlanId {
        let mut identity = Vec::with_capacity(33 + project_root.len());
        identity.extend_from_slice(organization_id.as_uuid().as_bytes());
        identity.extend_from_slice(source_revision_id.as_uuid().as_bytes());
        identity.push(0);
        identity.extend_from_slice(project_root.as_bytes());
        BuildPlanId::from_uuid(Uuid::new_v5(&BUILD_PLAN_NAMESPACE, &identity))
    }

    pub fn accept(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        contract: AcceptedBuildPlanContract,
        accepted_by: PrincipalId,
        accepted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let source_revision_id = contract.spec().source_revision_id;
        let id = Self::id_for(
            organization_id,
            source_revision_id,
            &contract.spec().proposal.spec().project_root,
        );
        let value = Self {
            organization_id,
            project_id,
            environment_id,
            id,
            source_revision_id,
            contract,
            aggregate_version: 1,
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
        environment_id: EnvironmentId,
        id: BuildPlanId,
        source_revision_id: SourceRevisionId,
        canonical_acl: &str,
        stored_digest: &str,
        aggregate_version: u64,
        accepted_by: PrincipalId,
        accepted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            project_id,
            environment_id,
            id,
            source_revision_id,
            contract: AcceptedBuildPlanContract::restore(canonical_acl, stored_digest)?,
            aggregate_version,
            accepted_by,
            accepted_at: canonical_timestamp(accepted_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.contract.validate()?;
        let proposal = &self.contract.spec().proposal;
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.source_revision_id.as_uuid().is_nil()
            || self.accepted_by.as_uuid().is_nil()
            || self.source_revision_id != self.contract.spec().source_revision_id
            || self.aggregate_version != 1
            || self.accepted_at != canonical_timestamp(self.accepted_at)
            || self.id
                != Self::id_for(
                    self.organization_id,
                    self.source_revision_id,
                    &proposal.spec().project_root,
                )
        {
            return Err("accepted BuildPlan identity or immutable state is invalid".into());
        }
        Ok(())
    }

    /// Canonical order for a page of accepted BuildPlans within one Source revision.
    pub(crate) fn canonical_cmp(&self, other: &Self) -> Ordering {
        self.contract
            .spec()
            .proposal
            .spec()
            .project_root
            .cmp(&other.contract.spec().proposal.spec().project_root)
            .then_with(|| self.id.cmp(&other.id))
    }
}
