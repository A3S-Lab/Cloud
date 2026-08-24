use super::AcceptedWorkloadProfileRevision;
use crate::modules::shared_kernel::domain::{
    BuildPlanId, EnvironmentId, OrganizationId, PrincipalId, ProjectId, SourceRevisionId,
    WorkloadProfileId, WorkloadProfileRevisionId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const WORKLOAD_PROFILE_REVISION_ACCEPTED_EVENT_KEY: &str =
    "developer.workload-profile.revision-accepted";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadProfileRevisionAccepted {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_profile_id: WorkloadProfileId,
    pub workload_profile_revision_id: WorkloadProfileRevisionId,
    pub revision_number: u64,
    pub build_plan_id: BuildPlanId,
    pub source_revision_id: SourceRevisionId,
    pub build_plan_digest: String,
    pub profile_digest: String,
    pub project_root: String,
    pub profile_name: String,
    pub profile_kind: String,
    pub accepted_by: PrincipalId,
}

impl WorkloadProfileRevisionAccepted {
    pub fn envelope(
        revision: &AcceptedWorkloadProfileRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        revision.validate()?;
        let spec = revision.contract.spec();
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: WORKLOAD_PROFILE_REVISION_ACCEPTED_EVENT_KEY.into(),
            schema_version: 1,
            organization_id: revision.organization_id.as_uuid(),
            aggregate_id: revision.profile_id.as_uuid(),
            aggregate_version: revision.revision_number,
            occurred_at: revision.accepted_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: revision.organization_id,
                project_id: revision.project_id,
                environment_id: revision.environment_id,
                workload_profile_id: revision.profile_id,
                workload_profile_revision_id: revision.id,
                revision_number: revision.revision_number,
                build_plan_id: revision.build_plan_id,
                source_revision_id: revision.source_revision_id,
                build_plan_digest: spec.build_plan_digest.to_string(),
                profile_digest: revision.contract.digest().to_string(),
                project_root: spec.project_root.clone(),
                profile_name: spec.profile.name.clone(),
                profile_kind: spec.profile.kind.as_str().into(),
                accepted_by: revision.accepted_by,
            })
            .map_err(|error| format!("workload profile revision event is invalid: {error}"))?,
        })
    }
}
