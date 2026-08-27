use crate::modules::agents::application::{
    AgentExecutionEventPage, AgentExecutionTrajectoryPage, CancelAgentExecutionResult,
    CaptureAgentExecutionCheckpointResult, CreateAgentConversationResult, ForkAgentExecutionResult,
    StartAgentExecutionResult,
};
use crate::modules::agents::domain::{
    AgentApprovalCheckpoint, AgentApprovalCheckpointStatus, AgentConversation,
    AgentConversationStatus, AgentExecution, AgentExecutionChangeSet, AgentExecutionCheckpoint,
    AgentExecutionCheckpointObjectReference, AgentExecutionCheckpointSnapshot, AgentExecutionEvent,
    AgentExecutionEventKind, AgentExecutionLineage, AgentExecutionStatus,
    AgentExecutionTelemetryCorrelation,
};
use crate::presentation::{format_sequence_cursor, SequencePage, SequenceRecord};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationMutationResponse {
    pub conversation: AgentConversationResponse,
    pub replayed: bool,
}

impl From<CreateAgentConversationResult> for AgentConversationMutationResponse {
    fn from(result: CreateAgentConversationResult) -> Self {
        Self {
            conversation: result.conversation.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionMutationResponse {
    pub conversation: AgentConversationResponse,
    pub execution: AgentExecutionResponse,
    pub replayed: bool,
}

impl From<StartAgentExecutionResult> for AgentExecutionMutationResponse {
    fn from(result: StartAgentExecutionResult) -> Self {
        Self {
            conversation: result.conversation.into(),
            execution: result.execution.into(),
            replayed: result.replayed,
        }
    }
}

impl From<CancelAgentExecutionResult> for AgentExecutionMutationResponse {
    fn from(result: CancelAgentExecutionResult) -> Self {
        Self {
            conversation: result.conversation.into(),
            execution: result.execution.into(),
            replayed: result.replayed,
        }
    }
}

impl From<ForkAgentExecutionResult> for AgentExecutionMutationResponse {
    fn from(result: ForkAgentExecutionResult) -> Self {
        Self {
            conversation: result.conversation.into(),
            execution: result.execution.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub id: Uuid,
    pub status: AgentConversationStatus,
    pub last_event_sequence: u64,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
}

impl From<AgentConversation> for AgentConversationResponse {
    fn from(conversation: AgentConversation) -> Self {
        Self {
            organization_id: conversation.organization_id.as_uuid(),
            project_id: conversation.project_id.as_uuid(),
            environment_id: conversation.environment_id.as_uuid(),
            id: conversation.id.as_uuid(),
            status: conversation.status,
            last_event_sequence: conversation.last_event_sequence,
            aggregate_version: conversation.aggregate_version,
            created_at: conversation.created_at,
            updated_at: conversation.updated_at,
            closed_at: conversation.closed_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionResponse {
    pub organization_id: Uuid,
    pub conversation_id: Uuid,
    pub id: Uuid,
    pub operation_id: Uuid,
    pub agent: AgentReleaseBindingResponse,
    pub provider: AgentProviderProfileResponse,
    pub invocation_profile: Option<a3s_cloud_contracts::HarnessInvocationProfileV1>,
    pub lineage: Option<AgentExecutionLineageResponse>,
    pub status: AgentExecutionStatus,
    pub failure: Option<String>,
    pub aggregate_version: u64,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub cancellation_requested_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl From<AgentExecution> for AgentExecutionResponse {
    fn from(execution: AgentExecution) -> Self {
        let invocation_profile = execution
            .code
            .as_ref()
            .and_then(|binding| binding.invocation_profile())
            .cloned();
        let lineage = execution.lineage.clone().map(Into::into);
        Self {
            organization_id: execution.organization_id.as_uuid(),
            conversation_id: execution.conversation_id.as_uuid(),
            id: execution.id.as_uuid(),
            operation_id: execution.operation_id.as_uuid(),
            agent: AgentReleaseBindingResponse {
                asset_id: execution.agent.asset_id().as_uuid(),
                asset_release_id: execution.agent.asset_release_id().as_uuid(),
                build_run_id: execution.agent.build_run_id().as_uuid(),
                artifact_uri: execution.agent.artifact_uri().to_owned(),
                artifact_digest: execution.agent.artifact_digest().as_str().to_owned(),
                artifact_media_type: execution.agent.artifact_media_type().to_owned(),
                artifact_size_bytes: execution.agent.artifact_size_bytes(),
            },
            provider: AgentProviderProfileResponse {
                kind: execution.provider.kind().to_owned(),
                revision: execution.provider.revision().to_owned(),
                protocol: execution.provider.protocol().to_owned(),
                native_protocol: execution.provider.native_protocol().to_owned(),
                profile_digest: execution.provider.profile_digest().to_owned(),
                capability_digest: execution.provider.capability_digest().to_owned(),
            },
            invocation_profile,
            lineage,
            status: execution.status,
            failure: execution.failure,
            aggregate_version: execution.aggregate_version,
            requested_at: execution.requested_at,
            updated_at: execution.updated_at,
            started_at: execution.started_at,
            cancellation_requested_at: execution.cancellation_requested_at,
            finished_at: execution.finished_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionLineageResponse {
    pub parent_execution_id: Uuid,
    pub parent_checkpoint_id: Uuid,
    pub parent_checkpoint_digest: String,
    pub depth: u16,
}

impl From<AgentExecutionLineage> for AgentExecutionLineageResponse {
    fn from(lineage: AgentExecutionLineage) -> Self {
        Self {
            parent_execution_id: lineage.parent_execution_id.as_uuid(),
            parent_checkpoint_id: lineage.parent_checkpoint_id.as_uuid(),
            parent_checkpoint_digest: lineage.parent_checkpoint_digest.as_str().to_owned(),
            depth: lineage.depth,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProviderProfileResponse {
    pub kind: String,
    pub revision: String,
    pub protocol: String,
    pub native_protocol: String,
    pub profile_digest: String,
    pub capability_digest: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentReleaseBindingResponse {
    pub asset_id: Uuid,
    pub asset_release_id: Uuid,
    pub build_run_id: Uuid,
    pub artifact_uri: String,
    pub artifact_digest: String,
    pub artifact_media_type: String,
    pub artifact_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentApprovalCheckpointMutationResponse {
    pub checkpoint: AgentApprovalCheckpointResponse,
    pub replayed: bool,
}

impl From<crate::modules::agents::application::DecideAgentApprovalCheckpointResult>
    for AgentApprovalCheckpointMutationResponse
{
    fn from(
        result: crate::modules::agents::application::DecideAgentApprovalCheckpointResult,
    ) -> Self {
        Self {
            checkpoint: result.checkpoint.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentApprovalCheckpointResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub conversation_id: Uuid,
    pub execution_id: Uuid,
    pub id: Uuid,
    pub provider_run_identity_digest: String,
    pub invocation_profile_digest: String,
    pub source_event_sequence: u64,
    pub call_id: String,
    pub tool: a3s_cloud_contracts::HarnessToolBindingV1,
    pub request: a3s_cloud_contracts::AgentProviderToolPayloadIdentityV1,
    pub status: AgentApprovalCheckpointStatus,
    pub decision_id: Option<Uuid>,
    pub outcome: Option<a3s_cloud_contracts::AgentProviderApprovalOutcomeV1>,
    pub decided_by: Option<Uuid>,
    pub authorization_decision_id: Option<String>,
    pub authorization_decision_digest: Option<String>,
    pub reason: Option<String>,
    pub decision_digest: Option<String>,
    pub resume_command_id: Option<Uuid>,
    pub resume_command_digest: Option<String>,
    pub aggregate_version: u64,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
    pub resumed_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
}

impl From<AgentApprovalCheckpoint> for AgentApprovalCheckpointResponse {
    fn from(checkpoint: AgentApprovalCheckpoint) -> Self {
        let (authorization_decision_id, authorization_decision_digest) = checkpoint
            .authorization_decision
            .map(|decision| (Some(decision.id), Some(decision.digest.as_str().to_owned())))
            .unwrap_or((None, None));
        Self {
            organization_id: checkpoint.organization_id.as_uuid(),
            project_id: checkpoint.project_id.as_uuid(),
            environment_id: checkpoint.environment_id.as_uuid(),
            conversation_id: checkpoint.conversation_id.as_uuid(),
            execution_id: checkpoint.execution_id.as_uuid(),
            id: checkpoint.id.as_uuid(),
            provider_run_identity_digest: checkpoint
                .provider_run_identity_digest
                .as_str()
                .to_owned(),
            invocation_profile_digest: checkpoint.invocation_profile_digest.as_str().to_owned(),
            source_event_sequence: checkpoint.source_event_sequence,
            call_id: checkpoint.call_id,
            tool: checkpoint.tool,
            request: checkpoint.request,
            status: checkpoint.status,
            decision_id: checkpoint.decision_id.map(|value| value.as_uuid()),
            outcome: checkpoint.outcome,
            decided_by: checkpoint.decided_by.map(|value| value.as_uuid()),
            authorization_decision_id,
            authorization_decision_digest,
            reason: checkpoint.reason,
            decision_digest: checkpoint
                .decision_digest
                .map(|value| value.as_str().to_owned()),
            resume_command_id: checkpoint.resume_command_id.map(|value| value.as_uuid()),
            resume_command_digest: checkpoint
                .resume_command_digest
                .map(|value| value.as_str().to_owned()),
            aggregate_version: checkpoint.aggregate_version,
            requested_at: checkpoint.requested_at,
            expires_at: checkpoint.expires_at,
            updated_at: checkpoint.updated_at,
            decided_at: checkpoint.decided_at,
            resumed_at: checkpoint.resumed_at,
            cancelled_at: checkpoint.cancelled_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionCheckpointMutationResponse {
    pub checkpoint: AgentExecutionCheckpointResponse,
    pub replayed: bool,
}

impl From<CaptureAgentExecutionCheckpointResult> for AgentExecutionCheckpointMutationResponse {
    fn from(result: CaptureAgentExecutionCheckpointResult) -> Self {
        Self {
            checkpoint: result.checkpoint.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionCheckpointResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub conversation_id: Uuid,
    pub execution_id: Uuid,
    pub id: Uuid,
    pub through_event_sequence: u64,
    pub event_count: u16,
    pub agent_artifact_digest: String,
    pub provider_profile_digest: String,
    pub invocation_profile_digest: String,
    pub object: AgentExecutionCheckpointObjectResponse,
    pub telemetry_correlation: AgentExecutionTelemetryCorrelationResponse,
    pub aggregate_version: u64,
    pub captured_at: DateTime<Utc>,
}

impl From<AgentExecutionCheckpoint> for AgentExecutionCheckpointResponse {
    fn from(checkpoint: AgentExecutionCheckpoint) -> Self {
        Self {
            organization_id: checkpoint.organization_id.as_uuid(),
            project_id: checkpoint.project_id.as_uuid(),
            environment_id: checkpoint.environment_id.as_uuid(),
            conversation_id: checkpoint.conversation_id.as_uuid(),
            execution_id: checkpoint.execution_id.as_uuid(),
            id: checkpoint.id.as_uuid(),
            through_event_sequence: checkpoint.through_event_sequence,
            event_count: checkpoint.event_count,
            agent_artifact_digest: checkpoint.agent_artifact_digest.as_str().to_owned(),
            provider_profile_digest: checkpoint.provider_profile_digest.as_str().to_owned(),
            invocation_profile_digest: checkpoint.invocation_profile_digest.as_str().to_owned(),
            object: checkpoint.object.into(),
            telemetry_correlation: checkpoint.telemetry_correlation.into(),
            aggregate_version: checkpoint.aggregate_version,
            captured_at: checkpoint.captured_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionCheckpointObjectResponse {
    pub schema: String,
    pub namespace: String,
    pub object_ref: String,
    pub digest: String,
    pub size_bytes: u64,
    pub media_type: String,
}

impl From<AgentExecutionCheckpointObjectReference> for AgentExecutionCheckpointObjectResponse {
    fn from(reference: AgentExecutionCheckpointObjectReference) -> Self {
        Self {
            schema: reference.schema,
            namespace: reference.namespace,
            object_ref: reference.object_ref,
            digest: reference.digest.as_str().to_owned(),
            size_bytes: reference.size_bytes,
            media_type: reference.media_type,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionTelemetryCorrelationResponse {
    pub operation_id: Uuid,
    pub provider_run_identity_digest: String,
    pub node_id: Uuid,
    pub workload_id: Uuid,
    pub workload_revision_id: Uuid,
    pub deployment_id: Uuid,
    pub replica_id: Uuid,
    pub runtime_unit_id: String,
    pub runtime_generation: u64,
}

impl From<AgentExecutionTelemetryCorrelation> for AgentExecutionTelemetryCorrelationResponse {
    fn from(correlation: AgentExecutionTelemetryCorrelation) -> Self {
        Self {
            operation_id: correlation.operation_id.as_uuid(),
            provider_run_identity_digest: correlation
                .provider_run_identity_digest
                .as_str()
                .to_owned(),
            node_id: correlation.node_id.as_uuid(),
            workload_id: correlation.workload_id.as_uuid(),
            workload_revision_id: correlation.workload_revision_id.as_uuid(),
            deployment_id: correlation.deployment_id.as_uuid(),
            replica_id: correlation.replica_id.as_uuid(),
            runtime_unit_id: correlation.runtime_unit_id,
            runtime_generation: correlation.runtime_generation,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionCheckpointSnapshotResponse {
    pub schema: String,
    pub organization_id: Uuid,
    pub conversation_id: Uuid,
    pub execution_id: Uuid,
    pub operation_id: Uuid,
    pub agent_artifact_digest: String,
    pub provider_profile_digest: String,
    pub invocation_profile_digest: String,
    pub through_event_sequence: u64,
    pub event_count: u16,
    pub telemetry_correlation: AgentExecutionTelemetryCorrelationResponse,
    pub events: Vec<AgentExecutionCheckpointEventResponse>,
    pub captured_at: DateTime<Utc>,
}

impl From<AgentExecutionCheckpointSnapshot> for AgentExecutionCheckpointSnapshotResponse {
    fn from(snapshot: AgentExecutionCheckpointSnapshot) -> Self {
        Self {
            schema: snapshot.schema,
            organization_id: snapshot.organization_id.as_uuid(),
            conversation_id: snapshot.conversation_id.as_uuid(),
            execution_id: snapshot.execution_id.as_uuid(),
            operation_id: snapshot.operation_id.as_uuid(),
            agent_artifact_digest: snapshot.agent_artifact_digest.as_str().to_owned(),
            provider_profile_digest: snapshot.provider_profile_digest.as_str().to_owned(),
            invocation_profile_digest: snapshot.invocation_profile_digest.as_str().to_owned(),
            through_event_sequence: snapshot.through_event_sequence,
            event_count: snapshot.event_count,
            telemetry_correlation: snapshot.telemetry_correlation.into(),
            events: snapshot
                .events
                .into_iter()
                .map(|event| AgentExecutionCheckpointEventResponse {
                    sequence: event.sequence,
                    kind: event.kind,
                    content: event.content,
                    content_digest: event.content_digest.as_str().to_owned(),
                    content_size_bytes: event.content_size_bytes,
                    occurred_at: event.occurred_at,
                })
                .collect(),
            captured_at: snapshot.captured_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionCheckpointEventResponse {
    pub sequence: u64,
    pub kind: AgentExecutionEventKind,
    pub content: serde_json::Value,
    pub content_digest: String,
    pub content_size_bytes: u64,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionChangeSetResponse {
    pub organization_id: Uuid,
    pub execution_id: Uuid,
    pub batch_id: Uuid,
    pub node_id: Uuid,
    /// Code-owned protocol payload; its snake_case wire fields remain exact.
    pub change_set: a3s_cloud_contracts::AgentProtocolChangeSetV1,
    pub recorded_at: DateTime<Utc>,
}

impl From<AgentExecutionChangeSet> for AgentExecutionChangeSetResponse {
    fn from(value: AgentExecutionChangeSet) -> Self {
        Self {
            organization_id: value.organization_id.as_uuid(),
            execution_id: value.execution_id.as_uuid(),
            batch_id: value.batch_id,
            node_id: value.node_id.as_uuid(),
            change_set: value.change_set,
            recorded_at: value.recorded_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionEventResponse {
    pub organization_id: Uuid,
    pub conversation_id: Uuid,
    pub execution_id: Uuid,
    pub sequence: u64,
    pub kind: AgentExecutionEventKind,
    pub content: serde_json::Value,
    pub content_digest: String,
    pub content_size_bytes: u64,
    pub occurred_at: DateTime<Utc>,
}

impl From<AgentExecutionEvent> for AgentExecutionEventResponse {
    fn from(event: AgentExecutionEvent) -> Self {
        Self {
            organization_id: event.organization_id.as_uuid(),
            conversation_id: event.conversation_id.as_uuid(),
            execution_id: event.execution_id.as_uuid(),
            sequence: event.sequence,
            kind: event.kind,
            content: event.content.value().clone(),
            content_digest: event.content.digest().as_str().to_owned(),
            content_size_bytes: event.content.size_bytes(),
            occurred_at: event.occurred_at,
        }
    }
}

impl SequenceRecord for AgentExecutionEventResponse {
    fn sequence(&self) -> u64 {
        self.sequence
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionEventPageResponse {
    pub conversation_id: Uuid,
    pub head_sequence: u64,
    pub records: Vec<AgentExecutionEventResponse>,
    pub next_cursor: Option<String>,
}

impl From<AgentExecutionEventPage> for AgentExecutionEventPageResponse {
    fn from(page: AgentExecutionEventPage) -> Self {
        Self {
            conversation_id: page.conversation_id.as_uuid(),
            head_sequence: page.head_sequence,
            records: page.records.into_iter().map(Into::into).collect(),
            next_cursor: page.next_after_sequence.map(format_sequence_cursor),
        }
    }
}

impl SequencePage for AgentExecutionEventPageResponse {
    type Record = AgentExecutionEventResponse;

    fn records(&self) -> &[Self::Record] {
        &self.records
    }

    fn take_records(&mut self) -> Vec<Self::Record> {
        std::mem::take(&mut self.records)
    }

    fn replace_records(&mut self, records: Vec<Self::Record>) {
        self.records = records;
    }

    fn set_next_cursor(&mut self, cursor: Option<String>) {
        self.next_cursor = cursor;
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionTrajectoryPageResponse {
    pub execution_id: Uuid,
    pub records: Vec<AgentExecutionEventResponse>,
    pub next_cursor: Option<String>,
}

impl From<AgentExecutionTrajectoryPage> for AgentExecutionTrajectoryPageResponse {
    fn from(page: AgentExecutionTrajectoryPage) -> Self {
        Self {
            execution_id: page.execution_id.as_uuid(),
            records: page.records.into_iter().map(Into::into).collect(),
            next_cursor: page.next_after_sequence.map(format_sequence_cursor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::agents::domain::{AgentEventContent, AgentExecutionEventKind};
    use crate::modules::shared_kernel::domain::{
        AgentConversationId, AgentExecutionId, OrganizationId,
    };

    #[test]
    fn event_response_is_camel_case_and_exposes_verified_content_identity() {
        let event = AgentExecutionEvent::from_draft(
            OrganizationId::new(),
            AgentConversationId::new(),
            AgentExecutionId::new(),
            1,
            crate::modules::agents::domain::AgentExecutionEventDraft::new(
                AgentExecutionEventKind::ModelOutput,
                AgentEventContent::inline_json(serde_json::json!({"text": "hello"}))
                    .expect("content"),
                Utc::now(),
            )
            .expect("draft"),
        )
        .expect("event");
        let encoded =
            serde_json::to_value(AgentExecutionEventResponse::from(event)).expect("response");
        assert_eq!(encoded["sequence"], 1);
        assert!(encoded.get("conversationId").is_some());
        assert!(encoded.get("contentDigest").is_some());
        assert!(encoded.get("contentSizeBytes").is_some());
    }
}
