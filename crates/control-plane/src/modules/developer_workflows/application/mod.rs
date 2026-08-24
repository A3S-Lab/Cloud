mod acceptance;
mod detection;
mod profile_compilation;
mod source_revision;
mod workload_profile_acceptance;

pub use acceptance::{AcceptBuildPlan, AcceptBuildPlanHandler, AcceptBuildPlanResult};
pub use detection::BuildPlanDetectionService;
pub use profile_compilation::{
    CompiledScheduledTaskProfile, CompiledServiceProfile, CompiledWorkloadProfile,
    WorkloadProfileCompilationService,
};
pub use source_revision::{BuildPlanSourceRevisionEvidence, IBuildPlanSourceRevisionPort};
pub use workload_profile_acceptance::{
    AcceptWorkloadProfile, AcceptWorkloadProfileHandler, AcceptWorkloadProfileResult,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod profile_compilation_tests;

#[cfg(test)]
mod workload_profile_acceptance_tests;
