use super::super::domain::{
    AcceptedBuildPlan, ScheduledTaskSchedule, WorkloadProfileContract, WorkloadProfileKind,
};
use crate::modules::artifacts::domain::{BuildRun, BuildRunStatus, BuildSubject};
use crate::modules::executions::domain::ExecutionTemplate;
use crate::modules::shared_kernel::domain::{
    BuildPlanId, BuildRunId, EnvironmentId, OrganizationId, ProjectId, Sha256Digest,
    SourceRevisionId,
};
use crate::modules::workloads::domain::entities::{OciArtifact, ServiceTemplate};

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
    pub template: ServiceTemplate,
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
    pub template: ExecutionTemplate,
    pub schedule: ScheduledTaskSchedule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompiledWorkloadProfile {
    Service(CompiledServiceProfile),
    ScheduledTask(CompiledScheduledTaskProfile),
}

pub struct WorkloadProfileCompilationService;

impl WorkloadProfileCompilationService {
    pub fn compile(
        build_plan: &AcceptedBuildPlan,
        profile: &WorkloadProfileContract,
        build_run: &BuildRun,
    ) -> Result<CompiledWorkloadProfile, String> {
        profile.validate_for(build_plan)?;
        validate_build_evidence(build_plan, build_run)?;

        let artifact = compiled_artifact(build_run)?;
        let profile_spec = &profile.spec().profile;
        match profile_spec.kind {
            WorkloadProfileKind::Web | WorkloadProfileKind::Worker => {
                let template = profile_spec.project_service_template(artifact);
                template.validate()?;
                Ok(CompiledWorkloadProfile::Service(CompiledServiceProfile {
                    organization_id: build_plan.organization_id,
                    project_id: build_plan.project_id,
                    environment_id: build_plan.environment_id,
                    build_plan_id: build_plan.id,
                    build_run_id: build_run.id,
                    source_revision_id: build_plan.source_revision_id,
                    profile_digest: profile.digest().clone(),
                    name: profile_spec.name.clone(),
                    kind: profile_spec.kind,
                    template,
                    public_port: profile_spec.public_port.clone(),
                }))
            }
            WorkloadProfileKind::ScheduledTask => {
                let schedule = profile_spec
                    .schedule
                    .clone()
                    .ok_or_else(|| "scheduled Task profile requires a schedule".to_owned())?;
                let template = profile_spec.project_execution_template(artifact)?;
                template.validate()?;
                Ok(CompiledWorkloadProfile::ScheduledTask(
                    CompiledScheduledTaskProfile {
                        organization_id: build_plan.organization_id,
                        project_id: build_plan.project_id,
                        environment_id: build_plan.environment_id,
                        build_plan_id: build_plan.id,
                        build_run_id: build_run.id,
                        source_revision_id: build_plan.source_revision_id,
                        profile_digest: profile.digest().clone(),
                        name: profile_spec.name.clone(),
                        template,
                        schedule,
                    },
                ))
            }
        }
    }
}

fn validate_build_evidence(
    build_plan: &AcceptedBuildPlan,
    build_run: &BuildRun,
) -> Result<(), String> {
    let proposal = &build_plan.contract.spec().proposal;
    let expected_subject = BuildSubject::external_source_revision(
        build_plan.project_id,
        build_plan.environment_id,
        build_plan.source_revision_id,
    );
    let expected_build_run_id =
        BuildRun::id_for_subject_attempt(expected_subject, build_run.attempt)?;
    let expected_retry_id = if build_run.attempt == 1 {
        None
    } else {
        Some(BuildRun::id_for_subject_attempt(
            expected_subject,
            build_run.attempt - 1,
        )?)
    };
    if build_run.organization_id != build_plan.organization_id
        || build_run.subject != expected_subject
        || build_run.id != expected_build_run_id
        || build_run.retry_of_build_run_id != expected_retry_id
        || build_run.operation_id.as_uuid() != build_run.id.as_uuid()
        || build_run.aggregate_version == 0
        || build_run.status != BuildRunStatus::Succeeded
        || build_run.requested_at < build_plan.accepted_at
        || build_run.started_at.is_none()
        || build_run.finished_at.is_none()
        || build_run.failure.is_some()
        || build_run.cancellation_requested_at.is_some()
        || build_run.cleanup_command_id.is_none()
        || !build_run.evidence_required
    {
        return Err(
            "workload profile requires one successful post-acceptance BuildRun in the exact scope"
                .into(),
        );
    }
    let finished_at = build_run
        .finished_at
        .ok_or_else(|| "successful BuildRun is missing its finish time".to_owned())?;
    if finished_at < build_run.requested_at || build_run.updated_at < finished_at {
        return Err("workload profile BuildRun timestamps are inconsistent".into());
    }
    let evidence = build_run
        .evidence
        .as_deref()
        .ok_or_else(|| "workload profile requires verified BuildEvidence".to_owned())?;
    evidence.validate()?;
    if evidence.build_run_id != build_run.id
        || evidence.operation_id != build_run.operation_id
        || !evidence.subject.matches(build_run.subject)
        || evidence.attempt != build_run.attempt
        || evidence.attested_at < build_run.requested_at
        || evidence.attested_at > finished_at
        || evidence.commit_sha != proposal.spec().source.commit_sha.as_str()
        || evidence.source_content_digest != proposal.spec().source.content_digest.as_str()
        || evidence.recipe != proposal.spec().recipe
        || build_run.source_content_digest.as_deref()
            != Some(proposal.spec().source.content_digest.as_str())
        || build_run.build_request_digest.as_deref() != Some(evidence.build_request_digest.as_str())
        || build_run.published_artifact.as_ref() != Some(&evidence.artifact)
    {
        return Err(
            "BuildEvidence changed the accepted BuildPlan or durable BuildRun binding".into(),
        );
    }
    Ok(())
}

fn compiled_artifact(build_run: &BuildRun) -> Result<OciArtifact, String> {
    let published = build_run
        .published_artifact
        .as_ref()
        .ok_or_else(|| "successful BuildRun is missing its published OCI artifact".to_owned())?;
    published.validate()?;
    let artifact = OciArtifact {
        uri: published.uri.clone(),
        digest: published.digest.clone(),
        media_type: published.media_type.clone(),
    };
    artifact.validate()?;
    Ok(artifact)
}
