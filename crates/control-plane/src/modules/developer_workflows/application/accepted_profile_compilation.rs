use super::{CompiledWorkloadProfile, WorkloadProfileCompilationService};
use crate::modules::developer_workflows::domain::{
    AcceptedBuildPlan, AcceptedWorkloadProfileRevision, IBuildPlanRepository,
    IWorkloadProfileRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    BuildPlanId, BuildRunId, EnvironmentId, OrganizationId, ProjectId, RepositoryError,
    WorkloadProfileId, WorkloadProfileRevisionId,
};
use a3s_boot::{BootError, CqrsContext, Query, QueryHandler};
use std::sync::Arc;

/// Resolve one exact accepted Developer Workflows revision and compile it
/// against one exact successful Artifacts BuildRun.
///
/// This query is an internal production-composition boundary. It creates no
/// Workload, Execution, schedule, Route, Operation, or delivery state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompileAcceptedWorkloadProfile {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub build_plan_id: BuildPlanId,
    pub workload_profile_id: WorkloadProfileId,
    pub workload_profile_revision_id: WorkloadProfileRevisionId,
    pub build_run_id: BuildRunId,
}

impl CompileAcceptedWorkloadProfile {
    fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.build_plan_id.as_uuid().is_nil()
            || self.workload_profile_id.as_uuid().is_nil()
            || self.workload_profile_revision_id.as_uuid().is_nil()
            || self.build_run_id.as_uuid().is_nil()
        {
            return Err("accepted workload profile compilation identity is invalid".into());
        }
        Ok(())
    }
}

impl Query for CompileAcceptedWorkloadProfile {
    type Output = ApplicationResult<CompiledAcceptedWorkloadProfile>;
}

/// An exact accepted-revision reference plus the owner-admitted compilation.
///
/// Keeping the local revision identity outside the owner-neutral compiled
/// value lets a later handoff retain its causation without copying any target
/// bounded-context lifecycle into Developer Workflows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledAcceptedWorkloadProfile {
    pub workload_profile_id: WorkloadProfileId,
    pub workload_profile_revision_id: WorkloadProfileRevisionId,
    pub revision_number: u64,
    pub compiled: CompiledWorkloadProfile,
}

pub struct CompileAcceptedWorkloadProfileHandler {
    build_plans: Arc<dyn IBuildPlanRepository>,
    workload_profiles: Arc<dyn IWorkloadProfileRepository>,
    compiler: Arc<WorkloadProfileCompilationService>,
}

impl CompileAcceptedWorkloadProfileHandler {
    pub fn new(
        build_plans: Arc<dyn IBuildPlanRepository>,
        workload_profiles: Arc<dyn IWorkloadProfileRepository>,
        compiler: Arc<WorkloadProfileCompilationService>,
    ) -> Self {
        Self {
            build_plans,
            workload_profiles,
            compiler,
        }
    }
}

impl QueryHandler<CompileAcceptedWorkloadProfile> for CompileAcceptedWorkloadProfileHandler {
    fn execute(
        &self,
        query: CompileAcceptedWorkloadProfile,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<CompiledAcceptedWorkloadProfile>>,
    > {
        let build_plans = Arc::clone(&self.build_plans);
        let workload_profiles = Arc::clone(&self.workload_profiles);
        let compiler = Arc::clone(&self.compiler);
        Box::pin(async move {
            if let Err(error) = query.validate() {
                return Ok(Err(ApplicationError::Invalid(error)));
            }

            let build_plan = match build_plans
                .find(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                    query.build_plan_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(compilation_authority_not_found()))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            validate_returned_build_plan(&query, &build_plan)?;

            let revision = match workload_profiles
                .find_revision(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                    query.workload_profile_id,
                    query.workload_profile_revision_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(compilation_authority_not_found()))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            validate_returned_revision(&query, &revision)?;
            if revision.build_plan_id != query.build_plan_id {
                return Ok(Err(compilation_authority_not_found()));
            }
            revision.validate_for(&build_plan).map_err(|error| {
                BootError::Internal(format!(
                    "persisted workload profile revision changed its accepted BuildPlan authority: {error}"
                ))
            })?;

            let compiled = match compiler
                .compile(&build_plan, &revision.contract, query.build_run_id)
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            validate_compiled_binding(&query, &build_plan, &revision, &compiled)?;

            Ok(Ok(CompiledAcceptedWorkloadProfile {
                workload_profile_id: revision.profile_id,
                workload_profile_revision_id: revision.id,
                revision_number: revision.revision_number,
                compiled,
            }))
        })
    }
}

fn validate_returned_build_plan(
    query: &CompileAcceptedWorkloadProfile,
    build_plan: &AcceptedBuildPlan,
) -> a3s_boot::Result<()> {
    build_plan.validate().map_err(|error| {
        BootError::Internal(format!("persisted accepted BuildPlan is invalid: {error}"))
    })?;
    if build_plan.organization_id != query.organization_id
        || build_plan.project_id != query.project_id
        || build_plan.environment_id != query.environment_id
        || build_plan.id != query.build_plan_id
    {
        return Err(BootError::Internal(
            "BuildPlan repository changed the requested compilation identity".into(),
        ));
    }
    Ok(())
}

fn validate_returned_revision(
    query: &CompileAcceptedWorkloadProfile,
    revision: &AcceptedWorkloadProfileRevision,
) -> a3s_boot::Result<()> {
    revision.validate().map_err(|error| {
        BootError::Internal(format!(
            "persisted accepted workload profile revision is invalid: {error}"
        ))
    })?;
    if revision.organization_id != query.organization_id
        || revision.project_id != query.project_id
        || revision.environment_id != query.environment_id
        || revision.profile_id != query.workload_profile_id
        || revision.id != query.workload_profile_revision_id
    {
        return Err(BootError::Internal(
            "workload profile repository changed the requested compilation identity".into(),
        ));
    }
    Ok(())
}

fn validate_compiled_binding(
    query: &CompileAcceptedWorkloadProfile,
    build_plan: &AcceptedBuildPlan,
    revision: &AcceptedWorkloadProfileRevision,
    compiled: &CompiledWorkloadProfile,
) -> a3s_boot::Result<()> {
    let binding = match compiled {
        CompiledWorkloadProfile::Service(value) => (
            value.organization_id,
            value.project_id,
            value.environment_id,
            value.build_plan_id,
            value.build_run_id,
            value.source_revision_id,
            &value.profile_digest,
        ),
        CompiledWorkloadProfile::ScheduledTask(value) => (
            value.organization_id,
            value.project_id,
            value.environment_id,
            value.build_plan_id,
            value.build_run_id,
            value.source_revision_id,
            &value.profile_digest,
        ),
    };
    if binding.0 != query.organization_id
        || binding.1 != query.project_id
        || binding.2 != query.environment_id
        || binding.3 != build_plan.id
        || binding.4 != query.build_run_id
        || binding.5 != build_plan.source_revision_id
        || binding.6 != revision.contract.digest()
    {
        return Err(BootError::Internal(
            "workload profile compiler changed the accepted revision binding".into(),
        ));
    }
    Ok(())
}

fn compilation_authority_not_found() -> ApplicationError {
    ApplicationError::NotFound("accepted workload profile compilation authority not found".into())
}
