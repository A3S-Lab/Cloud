use crate::modules::artifacts::domain::{
    BuildArtifact, BuildRunStatus, OciPublicationTarget, PublishedOciArtifact,
    ValidatedOciBuildOutput,
};
use crate::modules::shared_kernel::domain::{
    BuildRunId, NodeCommandId, NodeId, OrganizationId, SourceRevisionId,
};
use crate::modules::sources::domain::BuildRecipe;
use a3s_runtime::contract::RuntimeUnitSpec;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct BuildFlowInput {
    pub organization_id: OrganizationId,
    pub build_run_id: BuildRunId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PreparedBuild {
    pub organization_id: OrganizationId,
    pub build_run_id: BuildRunId,
    pub source_revision_id: SourceRevisionId,
    pub source_content_digest: String,
    pub input_artifact: BuildArtifact,
    pub recipe: BuildRecipe,
    pub convergence_deadline: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum PrepareStepOutput {
    Ready { prepared: Box<PreparedBuild> },
    Failed { reason: String },
    Rejected { reason: String },
    CancellationRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ScheduleStepInput {
    pub prepared: PreparedBuild,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ScheduleStepOutput {
    Ready {
        node_id: NodeId,
        spec: Box<RuntimeUnitSpec>,
    },
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Failed {
        reason: String,
    },
    CancellationRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ScheduledBuild {
    pub prepared: PreparedBuild,
    pub node_id: NodeId,
    pub spec: RuntimeUnitSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DispatchStepInput {
    pub scheduled: ScheduledBuild,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DispatchedBuild {
    pub scheduled: ScheduledBuild,
    pub command_id: NodeCommandId,
    pub result_deadline: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum DispatchStepOutput {
    Ready { dispatched: Box<DispatchedBuild> },
    Failed { reason: String },
    CancellationRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ObserveStepInput {
    pub dispatched: DispatchedBuild,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ObserveStepOutput {
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Succeeded {
        artifact: BuildArtifact,
        completed_at: DateTime<Utc>,
    },
    Failed {
        reason: String,
    },
    CancellationRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ValidateStepInput {
    pub flow: BuildFlowInput,
    pub artifact: BuildArtifact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ValidateStepOutput {
    Ready { output: ValidatedOciBuildOutput },
    Failed { reason: String },
    CancellationRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PreparePublicationStepInput {
    pub flow: BuildFlowInput,
    pub output: ValidatedOciBuildOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum PreparePublicationStepOutput {
    Ready {
        target: OciPublicationTarget,
        deadline_at: DateTime<Utc>,
    },
    Failed {
        reason: String,
    },
    CancellationRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PublishStepInput {
    pub flow: BuildFlowInput,
    pub output: ValidatedOciBuildOutput,
    pub target: OciPublicationTarget,
    pub deadline_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum PublishStepOutput {
    Ready {
        artifact: PublishedOciArtifact,
    },
    Failed {
        reason: String,
    },
    CancellationRequested {
        artifact: Option<PublishedOciArtifact>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AttestStepInput {
    pub flow: BuildFlowInput,
    pub artifact: PublishedOciArtifact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum AttestStepOutput {
    Ready {
        sbom_digest: String,
        provenance_digest: String,
        key_id: String,
        attested_at: DateTime<Utc>,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FailStepInput {
    pub flow: BuildFlowInput,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FailStepOutput {
    pub reason: String,
    pub failed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CleanupDispatchStepInput {
    pub flow: BuildFlowInput,
    pub attempt: u32,
    pub issued_at: Option<DateTime<Utc>>,
    pub cleanup_deadline: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DispatchedCleanup {
    pub node_id: NodeId,
    pub command_id: NodeCommandId,
    pub result_deadline: DateTime<Utc>,
    pub cleanup_deadline: DateTime<Utc>,
    pub attempt: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum CleanupDispatchStepOutput {
    NotRequired {
        cleaned_at: DateTime<Utc>,
    },
    Ready {
        dispatched: DispatchedCleanup,
    },
    Retry {
        reason: String,
        next_attempt_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CleanupObserveStepInput {
    pub flow: BuildFlowInput,
    pub dispatched: DispatchedCleanup,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum CleanupObserveStepOutput {
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Ready {
        cleaned_at: DateTime<Utc>,
    },
    Retry {
        reason: String,
        next_attempt_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CompleteStepInput {
    pub flow: BuildFlowInput,
    pub cleaned_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CompleteStepOutput {
    pub build_run_id: BuildRunId,
    pub status: BuildRunStatus,
    pub output: Option<ValidatedOciBuildOutput>,
    pub published_artifact: Option<PublishedOciArtifact>,
    pub failure: Option<String>,
    pub finished_at: DateTime<Utc>,
}
