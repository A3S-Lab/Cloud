use super::AcceptedBuildPlan;
use crate::modules::shared_kernel::domain::{
    BuildPlanId, EnvironmentId, OrganizationId, PrincipalId, ProjectId, SourceRevisionId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const BUILD_PLAN_ACCEPTED_EVENT_KEY: &str = "developer.build-plan.accepted";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildPlanAccepted {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub build_plan_id: BuildPlanId,
    pub source_revision_id: SourceRevisionId,
    pub plan_digest: String,
    pub proposal_digest: String,
    pub source_identity_digest: String,
    pub source_content_digest: String,
    pub commit_sha: String,
    pub project_root: String,
    pub detector: String,
    pub detector_revision: String,
    pub recipe_digest: String,
    pub accepted_by: PrincipalId,
}

impl BuildPlanAccepted {
    pub fn envelope(
        plan: &AcceptedBuildPlan,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        plan.validate()?;
        let proposal = &plan.contract.spec().proposal;
        let proposal_spec = proposal.spec();
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: BUILD_PLAN_ACCEPTED_EVENT_KEY.into(),
            schema_version: 1,
            organization_id: plan.organization_id.as_uuid(),
            aggregate_id: plan.id.as_uuid(),
            aggregate_version: plan.aggregate_version,
            occurred_at: plan.accepted_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: plan.organization_id,
                project_id: plan.project_id,
                environment_id: plan.environment_id,
                build_plan_id: plan.id,
                source_revision_id: plan.source_revision_id,
                plan_digest: plan.contract.digest().to_string(),
                proposal_digest: proposal.digest().to_string(),
                source_identity_digest: proposal_spec.source.source_identity_digest.to_string(),
                source_content_digest: proposal_spec.source.content_digest.to_string(),
                commit_sha: proposal_spec.source.commit_sha.to_string(),
                project_root: proposal_spec.project_root.clone(),
                detector: proposal_spec.detector.as_str().into(),
                detector_revision: proposal_spec.detector_revision.clone(),
                recipe_digest: proposal_spec.recipe.digest()?,
                accepted_by: plan.accepted_by,
            })
            .map_err(|error| format!("accepted BuildPlan event is invalid: {error}"))?,
        })
    }
}
