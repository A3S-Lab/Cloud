use super::{AcceptedBuildPlan, BuildPlanAccepted, BUILD_PLAN_ACCEPTED_EVENT_KEY};
use crate::modules::shared_kernel::domain::{
    BuildPlanId, EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId,
    ProjectId, RepositoryError, SourceRevisionId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AcceptBuildPlanWrite {
    pub plan: AcceptedBuildPlan,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl AcceptBuildPlanWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.plan.validate()?;
        self.idempotency.validate()?;
        if self.actor_principal_id.as_uuid().is_nil()
            || self.actor_principal_id != self.plan.accepted_by
            || self.request_id.is_nil()
            || self.event.event_id.is_nil()
            || self.event.event_key != BUILD_PLAN_ACCEPTED_EVENT_KEY
            || self.event.schema_version != 1
            || self.event.organization_id != self.plan.organization_id.as_uuid()
            || self.event.aggregate_id != self.plan.id.as_uuid()
            || self.event.aggregate_version != 1
            || self.event.occurred_at != self.plan.accepted_at
            || self.event.correlation_id != self.request_id
            || self.event.causation_id.is_some()
        {
            return Err("accepted BuildPlan write and event identity are inconsistent".into());
        }
        let payload: BuildPlanAccepted = serde_json::from_value(self.event.payload.clone())
            .map_err(|error| format!("accepted BuildPlan event payload is invalid: {error}"))?;
        let proposal = &self.plan.contract.spec().proposal;
        let spec = proposal.spec();
        if payload.organization_id != self.plan.organization_id
            || payload.project_id != self.plan.project_id
            || payload.environment_id != self.plan.environment_id
            || payload.build_plan_id != self.plan.id
            || payload.source_revision_id != self.plan.source_revision_id
            || payload.plan_digest != self.plan.contract.digest().as_str()
            || payload.proposal_digest != proposal.digest().as_str()
            || payload.source_identity_digest != spec.source.source_identity_digest.as_str()
            || payload.source_content_digest != spec.source.content_digest.as_str()
            || payload.commit_sha != spec.source.commit_sha.as_str()
            || payload.project_root != spec.project_root
            || payload.detector != spec.detector.as_str()
            || payload.detector_revision != spec.detector_revision
            || payload.recipe_digest != spec.recipe.digest()?
            || payload.accepted_by != self.plan.accepted_by
        {
            return Err("accepted BuildPlan event payload is inconsistent".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BuildPlanWriteReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub build_plan_id: BuildPlanId,
}

impl From<&AcceptedBuildPlan> for BuildPlanWriteReference {
    fn from(plan: &AcceptedBuildPlan) -> Self {
        Self {
            organization_id: plan.organization_id,
            project_id: plan.project_id,
            environment_id: plan.environment_id,
            build_plan_id: plan.id,
        }
    }
}

#[async_trait]
pub trait IBuildPlanRepository: Send + Sync {
    async fn replay_acceptance(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AcceptedBuildPlan>, RepositoryError>;

    async fn accept(
        &self,
        write: AcceptBuildPlanWrite,
    ) -> Result<IdempotentWrite<AcceptedBuildPlan>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        build_plan_id: BuildPlanId,
    ) -> Result<Option<AcceptedBuildPlan>, RepositoryError>;

    async fn find_for_source_root(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        source_revision_id: SourceRevisionId,
        project_root: &str,
    ) -> Result<Option<AcceptedBuildPlan>, RepositoryError>;

    async fn list_for_source(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        source_revision_id: SourceRevisionId,
        limit: usize,
    ) -> Result<Vec<AcceptedBuildPlan>, RepositoryError>;
}
