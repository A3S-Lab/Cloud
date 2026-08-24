use super::{AcceptedBuildPlan, WorkloadProfileContract};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, BuildPlanId, EnvironmentId, OrganizationId, PrincipalId, ProjectId,
    SourceRevisionId, WorkloadProfileId, WorkloadProfileRevisionId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const WORKLOAD_PROFILE_NAMESPACE: Uuid = Uuid::from_bytes([
    0x6d, 0x5d, 0x0e, 0x43, 0x9d, 0x60, 0x4e, 0x3e, 0xb1, 0x68, 0x6b, 0x9e, 0xd8, 0xe7, 0x85, 0xb4,
]);
const WORKLOAD_PROFILE_REVISION_NAMESPACE: Uuid = Uuid::from_bytes([
    0x5d, 0xf8, 0x71, 0x29, 0x8e, 0x53, 0x47, 0x10, 0x9f, 0xc6, 0x5d, 0x39, 0x7f, 0x8c, 0xd6, 0x52,
]);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedWorkloadProfileRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: WorkloadProfileId,
    pub id: WorkloadProfileRevisionId,
    pub revision_number: u64,
    pub build_plan_id: BuildPlanId,
    pub source_revision_id: SourceRevisionId,
    pub contract: WorkloadProfileContract,
    pub accepted_by: PrincipalId,
    pub accepted_at: DateTime<Utc>,
}

impl AcceptedWorkloadProfileRevision {
    pub fn profile_id_for(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        contract: &WorkloadProfileContract,
    ) -> Result<WorkloadProfileId, String> {
        contract.validate()?;
        let spec = contract.spec();
        let mut identity =
            Vec::with_capacity(60 + spec.project_root.len() + spec.profile.name.len());
        identity.extend_from_slice(organization_id.as_uuid().as_bytes());
        identity.extend_from_slice(project_id.as_uuid().as_bytes());
        identity.extend_from_slice(environment_id.as_uuid().as_bytes());
        push_identity_part(&mut identity, &spec.project_root)?;
        push_identity_part(&mut identity, &spec.profile.name)?;
        Ok(WorkloadProfileId::from_uuid(Uuid::new_v5(
            &WORKLOAD_PROFILE_NAMESPACE,
            &identity,
        )))
    }

    pub fn revision_id_for(
        profile_id: WorkloadProfileId,
        revision_number: u64,
        contract: &WorkloadProfileContract,
    ) -> Result<WorkloadProfileRevisionId, String> {
        contract.validate()?;
        if revision_number == 0 || revision_number > i64::MAX as u64 {
            return Err("workload profile revision number is outside the persistence bound".into());
        }
        let mut identity = Vec::with_capacity(24 + contract.digest().as_str().len());
        identity.extend_from_slice(profile_id.as_uuid().as_bytes());
        identity.extend_from_slice(&revision_number.to_be_bytes());
        identity.extend_from_slice(contract.digest().as_str().as_bytes());
        Ok(WorkloadProfileRevisionId::from_uuid(Uuid::new_v5(
            &WORKLOAD_PROFILE_REVISION_NAMESPACE,
            &identity,
        )))
    }

    pub fn accept(
        build_plan: &AcceptedBuildPlan,
        contract: WorkloadProfileContract,
        revision_number: u64,
        accepted_by: PrincipalId,
        accepted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        contract.validate_for(build_plan)?;
        let profile_id = Self::profile_id_for(
            build_plan.organization_id,
            build_plan.project_id,
            build_plan.environment_id,
            &contract,
        )?;
        let id = Self::revision_id_for(profile_id, revision_number, &contract)?;
        let value = Self {
            organization_id: build_plan.organization_id,
            project_id: build_plan.project_id,
            environment_id: build_plan.environment_id,
            profile_id,
            id,
            revision_number,
            build_plan_id: build_plan.id,
            source_revision_id: build_plan.source_revision_id,
            contract,
            accepted_by,
            accepted_at: canonical_timestamp(accepted_at),
        };
        value.validate_for(build_plan)?;
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: WorkloadProfileId,
        id: WorkloadProfileRevisionId,
        revision_number: u64,
        build_plan_id: BuildPlanId,
        source_revision_id: SourceRevisionId,
        canonical_acl: &str,
        stored_digest: &str,
        accepted_by: PrincipalId,
        accepted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            project_id,
            environment_id,
            profile_id,
            id,
            revision_number,
            build_plan_id,
            source_revision_id,
            contract: WorkloadProfileContract::restore(canonical_acl, stored_digest)?,
            accepted_by,
            accepted_at: canonical_timestamp(accepted_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.contract.validate()?;
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.profile_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.build_plan_id.as_uuid().is_nil()
            || self.source_revision_id.as_uuid().is_nil()
            || self.accepted_by.as_uuid().is_nil()
            || self.revision_number == 0
            || self.revision_number > i64::MAX as u64
            || self.accepted_at != canonical_timestamp(self.accepted_at)
            || self.build_plan_id != self.contract.spec().build_plan_id
            || self.source_revision_id != self.contract.spec().source_revision_id
            || self.profile_id
                != Self::profile_id_for(
                    self.organization_id,
                    self.project_id,
                    self.environment_id,
                    &self.contract,
                )?
            || self.id
                != Self::revision_id_for(self.profile_id, self.revision_number, &self.contract)?
        {
            return Err("accepted workload profile revision identity or state is invalid".into());
        }
        Ok(())
    }

    pub fn validate_for(&self, build_plan: &AcceptedBuildPlan) -> Result<(), String> {
        self.validate()?;
        self.contract.validate_for(build_plan)?;
        if self.organization_id != build_plan.organization_id
            || self.project_id != build_plan.project_id
            || self.environment_id != build_plan.environment_id
            || self.build_plan_id != build_plan.id
            || self.source_revision_id != build_plan.source_revision_id
            || self.accepted_at < build_plan.accepted_at
        {
            return Err(
                "accepted workload profile revision changed its BuildPlan authority".into(),
            );
        }
        Ok(())
    }
}

fn push_identity_part(identity: &mut Vec<u8>, value: &str) -> Result<(), String> {
    let length = u32::try_from(value.len())
        .map_err(|_| "workload profile identity field exceeds u32".to_owned())?;
    identity.extend_from_slice(&length.to_be_bytes());
    identity.extend_from_slice(value.as_bytes());
    Ok(())
}
