use super::VerifiedOciArtifact;
use crate::modules::developer_workflows::domain::{WorkloadProfileKind, WorkloadProfileSpec};
use crate::modules::shared_kernel::domain::{
    BuildPlanId, BuildRunId, EnvironmentId, OrganizationId, ProjectId, RepositoryError,
    Sha256Digest, SourceRevisionId,
};
use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadProfileTargetContext {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub build_plan_id: BuildPlanId,
    pub build_run_id: BuildRunId,
    pub source_revision_id: SourceRevisionId,
    pub profile_digest: Sha256Digest,
}

impl WorkloadProfileTargetContext {
    fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.build_plan_id.as_uuid().is_nil()
            || self.build_run_id.as_uuid().is_nil()
            || self.source_revision_id.as_uuid().is_nil()
            || Sha256Digest::parse(self.profile_digest.as_str())? != self.profile_digest
        {
            return Err("workload profile target context is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceProfileAdmissionRequest {
    pub context: WorkloadProfileTargetContext,
    pub profile: WorkloadProfileSpec,
    pub artifact: VerifiedOciArtifact,
}

impl ServiceProfileAdmissionRequest {
    pub fn validate(&self) -> Result<(), String> {
        self.context.validate()?;
        self.profile.validate()?;
        self.artifact.validate()?;
        if !matches!(
            self.profile.kind,
            WorkloadProfileKind::Web | WorkloadProfileKind::Worker
        ) {
            return Err("Service admission requires a web or worker profile".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledTaskProfileAdmissionRequest {
    pub context: WorkloadProfileTargetContext,
    pub profile: WorkloadProfileSpec,
    pub artifact: VerifiedOciArtifact,
}

impl ScheduledTaskProfileAdmissionRequest {
    pub fn validate(&self) -> Result<(), String> {
        self.context.validate()?;
        self.profile.validate()?;
        self.artifact.validate()?;
        if self.profile.kind != WorkloadProfileKind::ScheduledTask {
            return Err("scheduled Task admission requires a scheduled profile".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadProfileAdmissionTarget {
    Service,
    ScheduledTask,
}

/// Exact receipt returned by either target owner.
///
/// The receipt repeats only immutable correlation evidence. It does not expose
/// a Workloads or Executions aggregate, template, lifecycle state, or retry
/// mechanism. The consumer rejects a valid-looking receipt from another
/// target, profile, BuildRun, or artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadProfileAdmissionReceipt {
    pub target: WorkloadProfileAdmissionTarget,
    pub context: WorkloadProfileTargetContext,
    pub artifact_digest: Sha256Digest,
    pub owner_contract_digest: Sha256Digest,
}

impl WorkloadProfileAdmissionReceipt {
    pub fn validate_for(
        &self,
        target: WorkloadProfileAdmissionTarget,
        context: &WorkloadProfileTargetContext,
        artifact_digest: &Sha256Digest,
    ) -> Result<(), String> {
        self.context.validate()?;
        if self.target != target
            || self.context != *context
            || self.artifact_digest != *artifact_digest
            || Sha256Digest::parse(self.artifact_digest.as_str())? != self.artifact_digest
            || Sha256Digest::parse(self.owner_contract_digest.as_str())?
                != self.owner_contract_digest
        {
            return Err("workload profile admission receipt changed its request binding".into());
        }
        Ok(())
    }
}

/// Consumer-owned port into Workloads. The implementation translates the
/// local review intent, applies Workloads admission rules, and returns only an
/// immutable receipt; Workload lifecycle never enters this context.
#[async_trait]
pub trait IServiceProfileAdmissionPort: Send + Sync {
    async fn admit_service_profile(
        &self,
        request: ServiceProfileAdmissionRequest,
    ) -> Result<WorkloadProfileAdmissionReceipt, RepositoryError>;
}

/// Consumer-owned port into Executions. The implementation owns conversion to
/// an ExecutionTemplate and returns an immutable receipt; Task scheduling and
/// retry authority remain in Executions/Flow.
#[async_trait]
pub trait IScheduledTaskProfileAdmissionPort: Send + Sync {
    async fn admit_scheduled_task_profile(
        &self,
        request: ScheduledTaskProfileAdmissionRequest,
    ) -> Result<WorkloadProfileAdmissionReceipt, RepositoryError>;
}
