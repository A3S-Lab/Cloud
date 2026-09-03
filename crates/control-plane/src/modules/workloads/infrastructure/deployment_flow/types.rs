use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, BuildRunId, DeploymentId, NodeCommandId, NodeId, OrganizationId,
    Sha256Digest, SourceRevisionId, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::services::DeploymentGatewayPublication;
use a3s_runtime::contract::RuntimeUnitSpec;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DeploymentFlowInput {
    pub deployment_id: DeploymentId,
    pub organization_id: OrganizationId,
    pub revision_id: WorkloadRevisionId,
    #[serde(default)]
    pub rollback_source_revision_id: Option<WorkloadRevisionId>,
    pub workload_id: WorkloadId,
    // Deployment operation inputs also carry durable provenance for the command that
    // created the deployment. The Flow only needs the identifiers above, but it must
    // be able to replay the exact persisted operation envelope without dropping or
    // rejecting that metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_asset_id: Option<AssetId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_asset_release_id: Option<AssetReleaseId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_binding_action: Option<SkillBindingAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_source_revision_id: Option<SourceRevisionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_run_id: Option<BuildRunId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_artifact_digest: Option<Sha256Digest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SkillBindingAction {
    Bind,
    Unbind,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_persisted_skill_and_source_metadata_while_rejecting_unknown_fields() {
        let deployment_id = DeploymentId::new();
        let organization_id = OrganizationId::new();
        let revision_id = WorkloadRevisionId::new();
        let workload_id = WorkloadId::new();
        let rollback_source_revision_id = WorkloadRevisionId::new();
        let skill_asset_id = AssetId::new();
        let skill_asset_release_id = AssetReleaseId::new();
        let source_revision_id = SourceRevisionId::new();
        let build_run_id = BuildRunId::new();
        let published_artifact_digest = Sha256Digest::from_bytes(b"deployment-flow");

        let persisted = json!({
            "deploymentId": deployment_id,
            "organizationId": organization_id,
            "revisionId": revision_id,
            "rollbackSourceRevisionId": rollback_source_revision_id,
            "workloadId": workload_id,
            "skillAssetId": skill_asset_id,
            "skillAssetReleaseId": skill_asset_release_id,
            "skillBindingAction": "bind",
            "externalSourceRevisionId": source_revision_id,
            "buildRunId": build_run_id,
            "publishedArtifactDigest": published_artifact_digest,
        });

        let decoded: DeploymentFlowInput = serde_json::from_value(persisted.clone())
            .expect("persisted deployment operation metadata should be replayable");
        assert_eq!(decoded.deployment_id, deployment_id);
        assert_eq!(
            decoded.rollback_source_revision_id,
            Some(rollback_source_revision_id)
        );
        assert_eq!(decoded.skill_asset_id, Some(skill_asset_id));
        assert_eq!(decoded.skill_asset_release_id, Some(skill_asset_release_id));
        assert_eq!(decoded.skill_binding_action, Some(SkillBindingAction::Bind));
        assert_eq!(
            decoded.external_source_revision_id,
            Some(source_revision_id)
        );
        assert_eq!(decoded.build_run_id, Some(build_run_id));
        assert_eq!(
            decoded.published_artifact_digest,
            Some(published_artifact_digest.clone())
        );
        assert_eq!(
            serde_json::to_value(&decoded).expect("serialize metadata"),
            persisted
        );

        let legacy = json!({
            "deploymentId": deployment_id,
            "organizationId": organization_id,
            "revisionId": revision_id,
            "workloadId": workload_id,
        });
        let legacy_decoded: DeploymentFlowInput = serde_json::from_value(legacy)
            .expect("legacy deployment operation input should remain replayable");
        assert!(legacy_decoded.skill_asset_id.is_none());
        assert!(legacy_decoded.external_source_revision_id.is_none());

        let mut unknown = persisted;
        unknown["unexpectedMetadata"] = json!(true);
        assert!(serde_json::from_value::<DeploymentFlowInput>(unknown).is_err());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ResolveStepOutput {
    pub deployment_id: DeploymentId,
    pub organization_id: OrganizationId,
    pub revision_id: WorkloadRevisionId,
    pub workload_id: WorkloadId,
    pub spec: RuntimeUnitSpec,
    pub convergence_deadline: DateTime<Utc>,
    #[serde(default)]
    pub previous_runtime: Option<PreviousRuntime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PreviousRuntime {
    #[serde(default)]
    pub deployment_id: Option<DeploymentId>,
    pub revision_id: WorkloadRevisionId,
    pub node_id: NodeId,
    pub spec: RuntimeUnitSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ResolveCancellationOutput {
    pub cleaned_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum ResolveStepResult {
    Resolved(Box<ResolveStepOutput>),
    CancellationRequested(ResolveCancellationOutput),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ScheduleStepInput {
    pub resolved: ResolveStepOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ScheduleStepOutput {
    Ready {
        node_id: NodeId,
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
pub(super) struct PrepareClaimStepInput {
    pub resolved: ResolveStepOutput,
    pub node_id: NodeId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum PrepareClaimStepOutput {
    Ready {
        node_id: NodeId,
        binding_digest: String,
        prepared_at: DateTime<Utc>,
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
pub(super) struct PrestartGateStepInput {
    pub resolved: ResolveStepOutput,
    pub node_id: NodeId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum PrestartGateStepOutput {
    Ready {
        node_id: NodeId,
        completed_at: DateTime<Utc>,
    },
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Failed {
        reason: String,
    },
    CancellationRequested {
        completed_at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DispatchStepInput {
    pub resolved: ResolveStepOutput,
    pub node_id: NodeId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DispatchedRuntime {
    pub node_id: NodeId,
    pub command_id: NodeCommandId,
    pub result_deadline: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum DispatchStepOutput {
    Ready { dispatched: DispatchedRuntime },
    Failed { reason: String },
    CancellationRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReleaseClaimStepInput {
    pub organization_id: OrganizationId,
    pub deployment_id: DeploymentId,
    pub released_after: DateTime<Utc>,
    pub deadline_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ReleaseClaimStepOutput {
    Ready {
        released_at: DateTime<Utc>,
    },
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ObserveStepInput {
    pub resolved: ResolveStepOutput,
    pub dispatched: DispatchedRuntime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ObserveStepOutput {
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Ready {
        observed_at: DateTime<Utc>,
        received_at: DateTime<Utc>,
        spec_digest: String,
    },
    Failed {
        reason: String,
    },
    CancellationRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct VerifyStepInput {
    pub resolved: ResolveStepOutput,
    pub observation: ObserveStepOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum VerifyStepOutput {
    Verified { verified_at: DateTime<Utc> },
    CancellationRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StageGatewayStepInput {
    pub resolved: ResolveStepOutput,
    pub dispatched: DispatchedRuntime,
    pub verification: VerifyStepOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum StageGatewayStepOutput {
    NotRequired {
        gated_at: DateTime<Utc>,
    },
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Ready {
        publication: DeploymentGatewayPublication,
    },
    Failed {
        reason: String,
    },
    CancellationRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ObserveGatewayStepInput {
    pub resolved: ResolveStepOutput,
    pub publication: DeploymentGatewayPublication,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ObserveGatewayStepOutput {
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Ready {
        acknowledged_at: DateTime<Utc>,
    },
    Failed {
        reason: String,
    },
    CancellationRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum RouteGate {
    NotRequired {
        gated_at: DateTime<Utc>,
    },
    Acknowledged {
        publication: DeploymentGatewayPublication,
        acknowledged_at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ActivateStepInput {
    pub resolved: ResolveStepOutput,
    pub verification: VerifyStepOutput,
    #[serde(default)]
    pub routing: Option<RouteGate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum ActivateStepOutput {
    Active {
        deployment_id: DeploymentId,
        workload_id: WorkloadId,
        revision_id: WorkloadRevisionId,
        activated_at: DateTime<Utc>,
        #[serde(default)]
        retired_at: Option<DateTime<Utc>>,
    },
    CancellationRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RetirementDispatchStepInput {
    pub resolved: ResolveStepOutput,
    pub activation: ActivateStepOutput,
    pub attempt: u32,
    pub issued_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DispatchedRetirement {
    pub node_id: NodeId,
    pub command_id: NodeCommandId,
    pub result_deadline: DateTime<Utc>,
    pub retirement_deadline: DateTime<Utc>,
    pub attempt: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum RetirementDispatchStepOutput {
    NotRequired {
        retired_at: DateTime<Utc>,
    },
    Ready {
        dispatched: DispatchedRetirement,
    },
    Retry {
        reason: String,
        next_attempt_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RetirementObserveStepInput {
    pub resolved: ResolveStepOutput,
    pub dispatched: DispatchedRetirement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum RetirementObserveStepOutput {
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Ready {
        retired_at: DateTime<Utc>,
    },
    Retry {
        reason: String,
        next_attempt_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CompleteRetirementStepInput {
    pub resolved: ResolveStepOutput,
    pub activation: ActivateStepOutput,
    pub retired_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CleanupDispatchStepInput {
    pub resolved: ResolveStepOutput,
    pub attempt: u32,
    pub issued_at: Option<DateTime<Utc>>,
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
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CleanupObserveStepInput {
    pub resolved: ResolveStepOutput,
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
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CompleteCancellationStepInput {
    pub deployment_id: DeploymentId,
    pub organization_id: OrganizationId,
    pub cleaned_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CompleteCancellationStepOutput {
    pub deployment_id: DeploymentId,
    pub cancelled_at: DateTime<Utc>,
    pub operation_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FailStepInput {
    pub deployment_id: DeploymentId,
    pub organization_id: OrganizationId,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FailStepOutput {
    pub deployment_id: DeploymentId,
    pub failed_at: DateTime<Utc>,
    pub reason: String,
}
