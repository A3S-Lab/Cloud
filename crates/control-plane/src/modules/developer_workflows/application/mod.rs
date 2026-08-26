mod acceptance;
mod accepted_profile_compilation;
mod authorization;
mod build_outcome;
mod build_plan_detection_query;
mod detection;
mod preview_environment;
mod preview_policy_acceptance;
mod preview_source_subscription;
mod profile_compilation;
mod pull_request_preview_projection;
mod source_revision;
mod target_admission;
mod workload_profile_acceptance;

pub use acceptance::{AcceptBuildPlan, AcceptBuildPlanHandler, AcceptBuildPlanResult};
pub use accepted_profile_compilation::{
    CompileAcceptedWorkloadProfile, CompileAcceptedWorkloadProfileHandler,
    CompiledAcceptedWorkloadProfile,
};
pub use authorization::{
    DeveloperWorkflowAction, DeveloperWorkflowEnvironmentAccess,
    IDeveloperWorkflowAuthorizationPort,
};
pub use build_outcome::{
    IWorkloadBuildOutcomePort, VerifiedOciArtifact, VerifiedWorkloadBuildOutcome,
    WORKLOAD_BUILD_OUTCOME_SCHEMA,
};
pub use build_plan_detection_query::{DetectBuildPlanProposals, DetectBuildPlanProposalsHandler};
pub use detection::BuildPlanDetectionService;
pub use preview_environment::{
    EnsurePreviewEnvironment, IPreviewEnvironmentPort, PreviewEnvironmentBinding,
    PreviewEnvironmentReceipt,
};
pub use preview_policy_acceptance::{
    AcceptPullRequestPreviewPolicy, AcceptPullRequestPreviewPolicyHandler,
    AcceptPullRequestPreviewPolicyResult,
};
pub use preview_source_subscription::{
    IPreviewSourceSubscriptionQueryPort, PreviewSourceSubscriptionBinding,
};
pub use profile_compilation::{
    CompiledScheduledTaskProfile, CompiledServiceProfile, CompiledWorkloadProfile,
    WorkloadProfileCompilationService,
};
pub use pull_request_preview_projection::{
    IPullRequestPreviewProjectionPort, ProjectCommittedPullRequestChange,
    PullRequestPreviewProjectionService,
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
mod accepted_profile_compilation_tests;

#[cfg(test)]
mod build_plan_detection_query_tests;

#[cfg(test)]
mod workload_profile_acceptance_tests;

#[cfg(test)]
mod preview_policy_acceptance_tests;

#[cfg(test)]
mod pull_request_preview_projection_tests;
