mod acceptance;
mod detection;
mod profile_compilation;
mod source_revision;

pub use acceptance::{AcceptBuildPlan, AcceptBuildPlanHandler, AcceptBuildPlanResult};
pub use detection::BuildPlanDetectionService;
pub use profile_compilation::{
    CompiledScheduledTaskProfile, CompiledServiceProfile, CompiledWorkloadProfile,
    WorkloadProfileCompilationService,
};
pub use source_revision::{BuildPlanSourceRevisionEvidence, IBuildPlanSourceRevisionPort};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod profile_compilation_tests;
