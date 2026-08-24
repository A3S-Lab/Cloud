use super::{
    IScheduledTaskProfileAdmissionPort, IServiceProfileAdmissionPort, IWorkloadBuildOutcomePort,
    ScheduledTaskProfileAdmissionRequest, ServiceProfileAdmissionRequest,
    VerifiedWorkloadBuildOutcome, WorkloadProfileAdmissionReceipt, WorkloadProfileAdmissionTarget,
    WorkloadProfileTargetContext,
};
use crate::modules::developer_workflows::domain::{
    AcceptedBuildPlan, ScheduledTaskSchedule, WorkloadProfileContract, WorkloadProfileKind,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    BuildPlanId, BuildRunId, EnvironmentId, OrganizationId, ProjectId, Sha256Digest,
    SourceRevisionId,
};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledServiceProfile {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub build_plan_id: BuildPlanId,
    pub build_run_id: BuildRunId,
    pub source_revision_id: SourceRevisionId,
    pub profile_digest: Sha256Digest,
    pub name: String,
    pub kind: WorkloadProfileKind,
    pub admission: WorkloadProfileAdmissionReceipt,
    pub public_port: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledScheduledTaskProfile {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub build_plan_id: BuildPlanId,
    pub build_run_id: BuildRunId,
    pub source_revision_id: SourceRevisionId,
    pub profile_digest: Sha256Digest,
    pub name: String,
    pub admission: WorkloadProfileAdmissionReceipt,
    pub schedule: ScheduledTaskSchedule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompiledWorkloadProfile {
    Service(CompiledServiceProfile),
    ScheduledTask(CompiledScheduledTaskProfile),
}

pub struct WorkloadProfileCompilationService {
    builds: Arc<dyn IWorkloadBuildOutcomePort>,
    services: Arc<dyn IServiceProfileAdmissionPort>,
    scheduled_tasks: Arc<dyn IScheduledTaskProfileAdmissionPort>,
}

impl WorkloadProfileCompilationService {
    pub fn new(
        builds: Arc<dyn IWorkloadBuildOutcomePort>,
        services: Arc<dyn IServiceProfileAdmissionPort>,
        scheduled_tasks: Arc<dyn IScheduledTaskProfileAdmissionPort>,
    ) -> Self {
        Self {
            builds,
            services,
            scheduled_tasks,
        }
    }

    pub async fn compile(
        &self,
        build_plan: &AcceptedBuildPlan,
        profile: &WorkloadProfileContract,
        build_run_id: BuildRunId,
    ) -> ApplicationResult<CompiledWorkloadProfile> {
        profile
            .validate_for(build_plan)
            .map_err(ApplicationError::Invalid)?;
        let outcome = self
            .builds
            .verified_outcome(build_plan.organization_id, build_run_id)
            .await?
            .ok_or_else(|| {
                ApplicationError::NotFound("verified workload build outcome not found".into())
            })?;
        self.compile_verified(build_plan, profile, &outcome).await
    }

    async fn compile_verified(
        &self,
        build_plan: &AcceptedBuildPlan,
        profile: &WorkloadProfileContract,
        outcome: &VerifiedWorkloadBuildOutcome,
    ) -> ApplicationResult<CompiledWorkloadProfile> {
        validate_build_outcome(build_plan, outcome).map_err(ApplicationError::Invalid)?;

        let profile_spec = &profile.spec().profile;
        let context = WorkloadProfileTargetContext {
            organization_id: build_plan.organization_id,
            project_id: build_plan.project_id,
            environment_id: build_plan.environment_id,
            build_plan_id: build_plan.id,
            build_run_id: outcome.build_run_id,
            source_revision_id: build_plan.source_revision_id,
            profile_digest: profile.digest().clone(),
        };
        match profile_spec.kind {
            WorkloadProfileKind::Web | WorkloadProfileKind::Worker => {
                let request = ServiceProfileAdmissionRequest {
                    context: context.clone(),
                    profile: profile_spec.clone(),
                    artifact: outcome.artifact.clone(),
                };
                request.validate().map_err(ApplicationError::Invalid)?;
                let admission = self.services.admit_service_profile(request).await?;
                admission
                    .validate_for(
                        WorkloadProfileAdmissionTarget::Service,
                        &context,
                        &outcome.artifact.digest,
                    )
                    .map_err(ApplicationError::Invalid)?;
                Ok(CompiledWorkloadProfile::Service(CompiledServiceProfile {
                    organization_id: build_plan.organization_id,
                    project_id: build_plan.project_id,
                    environment_id: build_plan.environment_id,
                    build_plan_id: build_plan.id,
                    build_run_id: outcome.build_run_id,
                    source_revision_id: build_plan.source_revision_id,
                    profile_digest: profile.digest().clone(),
                    name: profile_spec.name.clone(),
                    kind: profile_spec.kind,
                    admission,
                    public_port: profile_spec.public_port.clone(),
                }))
            }
            WorkloadProfileKind::ScheduledTask => {
                let schedule = profile_spec.schedule.clone().ok_or_else(|| {
                    ApplicationError::Invalid("scheduled Task profile requires a schedule".into())
                })?;
                let request = ScheduledTaskProfileAdmissionRequest {
                    context: context.clone(),
                    profile: profile_spec.clone(),
                    artifact: outcome.artifact.clone(),
                };
                request.validate().map_err(ApplicationError::Invalid)?;
                let admission = self
                    .scheduled_tasks
                    .admit_scheduled_task_profile(request)
                    .await?;
                admission
                    .validate_for(
                        WorkloadProfileAdmissionTarget::ScheduledTask,
                        &context,
                        &outcome.artifact.digest,
                    )
                    .map_err(ApplicationError::Invalid)?;
                Ok(CompiledWorkloadProfile::ScheduledTask(
                    CompiledScheduledTaskProfile {
                        organization_id: build_plan.organization_id,
                        project_id: build_plan.project_id,
                        environment_id: build_plan.environment_id,
                        build_plan_id: build_plan.id,
                        build_run_id: outcome.build_run_id,
                        source_revision_id: build_plan.source_revision_id,
                        profile_digest: profile.digest().clone(),
                        name: profile_spec.name.clone(),
                        admission,
                        schedule,
                    },
                ))
            }
        }
    }
}

fn validate_build_outcome(
    build_plan: &AcceptedBuildPlan,
    outcome: &VerifiedWorkloadBuildOutcome,
) -> Result<(), String> {
    outcome.validate()?;
    let proposal = &build_plan.contract.spec().proposal;
    if outcome.organization_id != build_plan.organization_id
        || outcome.project_id != build_plan.project_id
        || outcome.environment_id != build_plan.environment_id
        || outcome.build_plan_id != build_plan.id
        || outcome.build_plan_digest != *build_plan.contract.digest()
        || outcome.source_revision_id != build_plan.source_revision_id
        || outcome.requested_at < build_plan.accepted_at
        || outcome.source_commit_sha != proposal.spec().source.commit_sha
        || outcome.source_content_digest != proposal.spec().source.content_digest
        || outcome.recipe != proposal.spec().recipe
    {
        return Err(
            "verified build outcome changed the accepted BuildPlan binding or scope".into(),
        );
    }
    Ok(())
}
