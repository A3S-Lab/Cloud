use super::{
    AcceptedBuildPlan, AcceptedWorkloadProfileRevision, WorkloadProfileRevisionAccepted,
    WORKLOAD_PROFILE_REVISION_ACCEPTED_EVENT_KEY,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId, ProjectId,
    RepositoryError, WorkloadProfileId, WorkloadProfileRevisionId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_WORKLOAD_PROFILE_REVISIONS_PAGE: usize = 100;

#[derive(Debug, Clone)]
pub struct AcceptWorkloadProfileRevisionWrite {
    pub revision: AcceptedWorkloadProfileRevision,
    pub build_plan: AcceptedBuildPlan,
    pub expected_previous_revision_id: Option<WorkloadProfileRevisionId>,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl AcceptWorkloadProfileRevisionWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.revision.validate_for(&self.build_plan)?;
        self.idempotency.validate()?;
        let expected_previous_shape = match self.revision.revision_number {
            1 => self.expected_previous_revision_id.is_none(),
            _ => self
                .expected_previous_revision_id
                .is_some_and(|id| !id.as_uuid().is_nil() && id != self.revision.id),
        };
        if !expected_previous_shape
            || self.actor_principal_id.as_uuid().is_nil()
            || self.actor_principal_id != self.revision.accepted_by
            || self.request_id.is_nil()
            || self.event.event_id.is_nil()
            || self.event.event_key != WORKLOAD_PROFILE_REVISION_ACCEPTED_EVENT_KEY
            || self.event.schema_version != 1
            || self.event.organization_id != self.revision.organization_id.as_uuid()
            || self.event.aggregate_id != self.revision.profile_id.as_uuid()
            || self.event.aggregate_version != self.revision.revision_number
            || self.event.occurred_at != self.revision.accepted_at
            || self.event.correlation_id != self.request_id
            || self.event.causation_id.is_some()
        {
            return Err(
                "workload profile revision write and event identity are inconsistent".into(),
            );
        }
        let payload: WorkloadProfileRevisionAccepted =
            serde_json::from_value(self.event.payload.clone()).map_err(|error| {
                format!("workload profile revision event payload is invalid: {error}")
            })?;
        let spec = self.revision.contract.spec();
        if payload.organization_id != self.revision.organization_id
            || payload.project_id != self.revision.project_id
            || payload.environment_id != self.revision.environment_id
            || payload.workload_profile_id != self.revision.profile_id
            || payload.workload_profile_revision_id != self.revision.id
            || payload.revision_number != self.revision.revision_number
            || payload.build_plan_id != self.revision.build_plan_id
            || payload.source_revision_id != self.revision.source_revision_id
            || payload.build_plan_digest != spec.build_plan_digest.as_str()
            || payload.profile_digest != self.revision.contract.digest().as_str()
            || payload.project_root != spec.project_root
            || payload.profile_name != spec.profile.name
            || payload.profile_kind != spec.profile.kind.as_str()
            || payload.accepted_by != self.revision.accepted_by
        {
            return Err("workload profile revision event payload is inconsistent".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkloadProfileRevisionWriteReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_profile_id: WorkloadProfileId,
    pub workload_profile_revision_id: WorkloadProfileRevisionId,
}

impl From<&AcceptedWorkloadProfileRevision> for WorkloadProfileRevisionWriteReference {
    fn from(revision: &AcceptedWorkloadProfileRevision) -> Self {
        Self {
            organization_id: revision.organization_id,
            project_id: revision.project_id,
            environment_id: revision.environment_id,
            workload_profile_id: revision.profile_id,
            workload_profile_revision_id: revision.id,
        }
    }
}

#[async_trait]
pub trait IWorkloadProfileRepository: Send + Sync {
    async fn replay_acceptance(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AcceptedWorkloadProfileRevision>, RepositoryError>;

    async fn accept(
        &self,
        write: AcceptWorkloadProfileRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedWorkloadProfileRevision>, RepositoryError>;

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        workload_profile_id: WorkloadProfileId,
        workload_profile_revision_id: WorkloadProfileRevisionId,
    ) -> Result<Option<AcceptedWorkloadProfileRevision>, RepositoryError>;

    async fn find_current(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        workload_profile_id: WorkloadProfileId,
    ) -> Result<Option<AcceptedWorkloadProfileRevision>, RepositoryError>;

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        workload_profile_id: WorkloadProfileId,
        limit: usize,
    ) -> Result<Vec<AcceptedWorkloadProfileRevision>, RepositoryError>;
}
