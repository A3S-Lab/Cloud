use crate::modules::shared_kernel::domain::{
    NodeCommandId, NodeId, OperationId, OrganizationId, PluginAssignmentId, PluginPlanProjectionId,
    PluginRegistryId,
};
use a3s_use_core::PluginOperationConfirmation;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PluginAssignmentFlowInput {
    pub organization_id: OrganizationId,
    pub assignment_id: PluginAssignmentId,
    pub operation_id: OperationId,
    pub assignment_generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct LockedPluginAssignment {
    pub organization_id: OrganizationId,
    pub assignment_id: PluginAssignmentId,
    pub operation_id: OperationId,
    pub assignment_generation: u64,
    pub registry_id: PluginRegistryId,
    pub target_host_id: NodeId,
    pub package_id: String,
    pub desired_state: String,
    pub locked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum LockOutput {
    Ready {
        locked: Box<LockedPluginAssignment>,
    },
    Terminal {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ResolveHostInput {
    pub locked: Box<LockedPluginAssignment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ResolvedHost {
    pub locked: Box<LockedPluginAssignment>,
    pub command_id: NodeCommandId,
    pub host_id: String,
    pub manager_version: String,
    pub manager_build_id: String,
    pub capabilities_digest: String,
    pub resolved_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ResolveHostOutput {
    Ready {
        resolved: Box<ResolvedHost>,
    },
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Terminal {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AuthorizeTrustInput {
    pub resolved: Box<ResolvedHost>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AuthorizedTrust {
    pub resolved: Box<ResolvedHost>,
    pub command_id: NodeCommandId,
    pub trust_root_digest: String,
    pub policy_digest: String,
    pub authorized_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum AuthorizeTrustOutput {
    Ready {
        authorized: Box<AuthorizedTrust>,
    },
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Terminal {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct EnqueuePlanInput {
    pub authorized: Box<AuthorizedTrust>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PlannedAssignment {
    pub authorized: Box<AuthorizedTrust>,
    pub command_id: NodeCommandId,
    pub request_id: String,
    pub plan_digest: String,
    pub use_operation_id: String,
    /// `package` for PluginHostPlan, `enablement` for PluginHostPlanEnablement.
    pub plan_kind: String,
    pub action: String,
    pub planned_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AlreadyConvergedAssignment {
    pub authorized: Box<AuthorizedTrust>,
    pub observed_desired: String,
    pub observed_state: String,
    pub capability_generation: u64,
    pub package_generation: Option<u64>,
    pub converged_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum EnqueuePlanOutput {
    Ready {
        planned: Box<PlannedAssignment>,
    },
    AlreadyConverged {
        converged: Box<AlreadyConvergedAssignment>,
    },
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Terminal {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StorePlanInput {
    pub planned: Box<PlannedAssignment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StoredPlan {
    pub planned: Box<PlannedAssignment>,
    pub projection_id: PluginPlanProjectionId,
    pub plan_digest: String,
    pub awaits_confirmation: bool,
    pub stored_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum StorePlanOutput {
    Ready {
        stored: Box<StoredPlan>,
    },
    Terminal {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AwaitConfirmationInput {
    pub stored: Box<StoredPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ConfirmedPlan {
    pub stored: Box<StoredPlan>,
    pub confirmation_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<PluginOperationConfirmation>,
    pub confirmed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum AwaitConfirmationOutput {
    Ready {
        confirmed: Box<ConfirmedPlan>,
    },
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Terminal {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct EnqueueApplyInput {
    pub confirmed: Box<ConfirmedPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AppliedAssignment {
    pub confirmed: Box<ConfirmedPlan>,
    pub command_id: NodeCommandId,
    pub request_id: String,
    pub operation_result_digest: String,
    pub capability_generation: u64,
    pub package_generation: Option<u64>,
    pub applied_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum EnqueueApplyOutput {
    Ready {
        applied: Box<AppliedAssignment>,
    },
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Terminal {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ObserveInput {
    pub applied: Box<AppliedAssignment>,
    pub observation_attempt: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ObservedAssignment {
    pub applied: Box<AppliedAssignment>,
    pub command_id: NodeCommandId,
    pub observed_desired: String,
    pub observed_state: String,
    pub capability_generation: u64,
    pub package_generation: Option<u64>,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ObserveOutput {
    Ready {
        observed: Box<ObservedAssignment>,
    },
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Terminal {
        reason: String,
    },
}
