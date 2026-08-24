mod accepted_build_plan;
mod accepted_build_plan_contract;
mod build_plan;
mod detection;
mod events;
mod repository;
pub(crate) mod source_layout;
mod workload_profile;

pub use accepted_build_plan::AcceptedBuildPlan;
pub use accepted_build_plan_contract::{
    AcceptedBuildPlanContract, AcceptedBuildPlanContractSpec, BUILD_PLAN_MAX_ACL_BYTES,
    BUILD_PLAN_SCHEMA,
};
pub use build_plan::{
    BuildPlanDetectorKind, BuildPlanProposal, BuildPlanProposalSpec, BUILD_PLAN_DETECTOR_REVISION,
    BUILD_PLAN_PROPOSAL_MAX_ACL_BYTES, BUILD_PLAN_PROPOSAL_SCHEMA,
};
pub use detection::{
    BuildPlanDetection, BuildPlanDetectionDiagnostic, BuildPlanDetectionDiagnosticCode,
    BuildPlanDetectorMatch, BuildPlanDetectorOutput, IBuildPlanDetector, MAX_BUILD_PLAN_DETECTORS,
    MAX_BUILD_PLAN_DIAGNOSTICS, MAX_BUILD_PLAN_PROPOSALS,
};
pub use events::{BuildPlanAccepted, BUILD_PLAN_ACCEPTED_EVENT_KEY};
pub(crate) use repository::BuildPlanWriteReference;
pub use repository::{AcceptBuildPlanWrite, IBuildPlanRepository};
pub use source_layout::{
    SourceLayoutEntry, SourceLayoutEntryKind, SourceLayoutIdentity, SourceLayoutSnapshot,
    MAX_SOURCE_LAYOUT_CONTENT_BYTES, MAX_SOURCE_LAYOUT_ENTRIES,
    MAX_SOURCE_LAYOUT_INSPECTED_FILE_BYTES,
};
pub use workload_profile::{
    ScheduledTaskCatchUpPolicy, ScheduledTaskHistoryPolicy, ScheduledTaskRetryPolicy,
    ScheduledTaskSchedule, WorkloadProfileContract, WorkloadProfileContractSpec,
    WorkloadProfileKind, WorkloadProfileResources, WorkloadProfileSpec,
    WORKLOAD_PROFILE_MAX_ACL_BYTES, WORKLOAD_PROFILE_SCHEMA,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod workload_profile_tests;
