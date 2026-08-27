mod accepted_build_plan;
mod accepted_build_plan_contract;
mod accepted_preview_policy;
mod accepted_workload_profile;
mod build_plan;
mod detection;
mod events;
mod preview_policy_contract;
mod preview_policy_events;
mod preview_policy_repository;
mod pull_request_preview;
mod pull_request_preview_events;
mod pull_request_preview_projection;
mod repository;
pub(crate) mod source_layout;
mod workload_profile;
mod workload_profile_events;
mod workload_profile_repository;
mod workload_profile_values;

pub use accepted_build_plan::AcceptedBuildPlan;
pub use accepted_build_plan_contract::{
    AcceptedBuildPlanContract, AcceptedBuildPlanContractSpec, BUILD_PLAN_MAX_ACL_BYTES,
    BUILD_PLAN_SCHEMA,
};
pub use accepted_preview_policy::AcceptedPullRequestPreviewPolicyRevision;
pub use accepted_workload_profile::AcceptedWorkloadProfileRevision;
pub use build_plan::{
    BuildPlanDetectorKind, BuildPlanProposal, BuildPlanProposalSpec, ASSET_ACL_EVIDENCE_PATH,
    BUILD_PLAN_DETECTOR_REVISION, BUILD_PLAN_PROPOSAL_MAX_ACL_BYTES, BUILD_PLAN_PROPOSAL_SCHEMA,
};
pub use detection::{
    BuildPlanDetection, BuildPlanDetectionDiagnostic, BuildPlanDetectionDiagnosticCode,
    BuildPlanDetectorMatch, BuildPlanDetectorOutput, IBuildPlanDetector, MAX_BUILD_PLAN_DETECTORS,
    MAX_BUILD_PLAN_DIAGNOSTICS, MAX_BUILD_PLAN_PROPOSALS,
};
pub use events::{BuildPlanAccepted, BUILD_PLAN_ACCEPTED_EVENT_KEY};
pub use preview_policy_contract::{
    PullRequestPreviewPolicyContract, PULL_REQUEST_PREVIEW_POLICY_MAX_ACL_BYTES,
    PULL_REQUEST_PREVIEW_POLICY_SCHEMA,
};
pub use preview_policy_events::{
    PullRequestPreviewPolicyRevisionAccepted,
    PULL_REQUEST_PREVIEW_POLICY_REVISION_ACCEPTED_EVENT_KEY,
};
pub(crate) use preview_policy_repository::PreviewPolicyRevisionWriteReference;
pub use preview_policy_repository::{
    AcceptPullRequestPreviewPolicyRevisionWrite, IPullRequestPreviewPolicyRepository,
    MAX_PULL_REQUEST_PREVIEW_POLICY_REVISIONS_PAGE,
};
pub use pull_request_preview::{
    reconcile_pull_request_preview, GitBranch, GithubInstallationRef, PreviewCleanupReason,
    PreviewForkPolicy, PreviewQuota, PreviewReconcileOutcome, PreviewReconciliation,
    PullRequestChange, PullRequestChangeKind, PullRequestPreview, PullRequestPreviewPolicy,
    PullRequestPreviewPolicyAuthority, PullRequestPreviewStatus, MAX_ACTIVE_PREVIEWS_PER_POLICY,
    MAX_PREVIEW_LIFETIME_SECONDS, MIN_PREVIEW_LIFETIME_SECONDS,
};
pub use pull_request_preview_events::PullRequestPreviewLifecycleEvent;
pub use pull_request_preview_projection::{
    CommitPullRequestPreviewProjection, IPullRequestPreviewProjectionRepository,
    PullRequestPreviewFactFingerprint, PullRequestPreviewProjectionOutcome,
    PullRequestPreviewProjectionReceipt, PullRequestPreviewVersion,
};
pub(crate) use repository::BuildPlanWriteReference;
pub use repository::{AcceptBuildPlanWrite, IBuildPlanRepository};
pub use source_layout::{
    SourceLayoutEntry, SourceLayoutEntryKind, SourceLayoutIdentity, SourceLayoutSnapshot,
    MAX_SOURCE_LAYOUT_CONTENT_BYTES, MAX_SOURCE_LAYOUT_ENTRIES,
    MAX_SOURCE_LAYOUT_INSPECTED_FILE_BYTES, MAX_SOURCE_LAYOUT_PATH_BYTES,
};
pub use workload_profile::{
    ScheduledTaskCatchUpPolicy, ScheduledTaskHistoryPolicy, ScheduledTaskRetryPolicy,
    ScheduledTaskSchedule, WorkloadProfileContract, WorkloadProfileContractSpec,
    WorkloadProfileKind, WorkloadProfileSpec, MAX_WORKLOAD_PROFILE_NAME_BYTES,
    MAX_WORKLOAD_PROFILE_SAFE_INTEGER, MAX_WORKLOAD_SCHEDULE_ATTEMPTS,
    MAX_WORKLOAD_SCHEDULE_BACKOFF_MS, MAX_WORKLOAD_SCHEDULE_CONCURRENCY,
    MAX_WORKLOAD_SCHEDULE_EXPRESSION_BYTES, MAX_WORKLOAD_SCHEDULE_HISTORY_COUNT,
    MAX_WORKLOAD_SCHEDULE_HISTORY_DAYS, MAX_WORKLOAD_SCHEDULE_MISFIRE_GRACE_MS,
    MAX_WORKLOAD_SCHEDULE_TIMEZONE_NAME_BYTES, WORKLOAD_PROFILE_MAX_ACL_BYTES,
    WORKLOAD_PROFILE_SCHEMA,
};
pub use workload_profile_events::{
    WorkloadProfileRevisionAccepted, WORKLOAD_PROFILE_REVISION_ACCEPTED_EVENT_KEY,
};
pub(crate) use workload_profile_repository::WorkloadProfileRevisionWriteReference;
pub use workload_profile_repository::{
    AcceptWorkloadProfileRevisionWrite, IWorkloadProfileRepository,
    MAX_WORKLOAD_PROFILE_REVISIONS_PAGE,
};
pub use workload_profile_values::{
    WorkloadHttpHealthCheck, WorkloadProcess, WorkloadProfileResources, WorkloadSecretBinding,
    WorkloadSecretTarget, WorkloadServicePort, MAX_WORKLOAD_ENVIRONMENT_VARIABLES,
    MAX_WORKLOAD_ENVIRONMENT_VARIABLE_NAME_BYTES, MAX_WORKLOAD_EXECUTION_ENVIRONMENT_VALUE_BYTES,
    MAX_WORKLOAD_HEALTH_PATH_BYTES, MAX_WORKLOAD_PROCESS_ARGUMENTS, MAX_WORKLOAD_PROCESS_COMMANDS,
    MAX_WORKLOAD_PROCESS_VALUE_BYTES, MAX_WORKLOAD_PROFILE_EXECUTION_TIMEOUT_MS,
    MAX_WORKLOAD_RESOURCE_CPU_MILLIS, MAX_WORKLOAD_RESOURCE_EPHEMERAL_STORAGE_BYTES,
    MAX_WORKLOAD_RESOURCE_MEMORY_BYTES, MAX_WORKLOAD_RESOURCE_PIDS, MAX_WORKLOAD_SECRET_BINDINGS,
    MAX_WORKLOAD_SECRET_NAME_BYTES, MAX_WORKLOAD_SERVICE_ENVIRONMENT_VALUE_BYTES,
    MAX_WORKLOAD_SERVICE_PORTS, MAX_WORKLOAD_SERVICE_PORT_NAME_BYTES,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod workload_profile_tests;

#[cfg(test)]
mod pull_request_preview_tests;

#[cfg(test)]
mod preview_policy_tests;
