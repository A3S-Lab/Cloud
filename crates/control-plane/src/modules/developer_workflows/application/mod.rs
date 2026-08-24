mod acceptance;
mod authorization;
mod build_outcome;
mod detection;
mod profile_compilation;
mod source_revision;
mod target_admission;
mod workload_profile_acceptance;

pub use acceptance::{AcceptBuildPlan, AcceptBuildPlanHandler, AcceptBuildPlanResult};
pub use authorization::{
    DeveloperWorkflowAction, DeveloperWorkflowEnvironmentAccess,
    IDeveloperWorkflowAuthorizationPort,
};
pub use build_outcome::{
    IWorkloadBuildOutcomePort, VerifiedOciArtifact, VerifiedWorkloadBuildOutcome,
    WORKLOAD_BUILD_OUTCOME_SCHEMA,
};
pub use detection::BuildPlanDetectionService;
pub use profile_compilation::{
    CompiledScheduledTaskProfile, CompiledServiceProfile, CompiledWorkloadProfile,
    WorkloadProfileCompilationService,
};
pub use source_revision::{BuildPlanSourceRevisionEvidence, IBuildPlanSourceRevisionPort};
pub use target_admission::{
    IScheduledTaskProfileAdmissionPort, IServiceProfileAdmissionPort,
    ScheduledTaskProfileAdmissionRequest, ServiceProfileAdmissionRequest,
    WorkloadProfileAdmissionReceipt, WorkloadProfileAdmissionTarget, WorkloadProfileTargetContext,
};
pub use workload_profile_acceptance::{
    AcceptWorkloadProfile, AcceptWorkloadProfileHandler, AcceptWorkloadProfileResult,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod profile_compilation_tests;

#[cfg(test)]
mod workload_profile_acceptance_tests;
