use crate::modules::agents::domain::{AgentConversation, AgentExecution, AgentExecutionCheckpoint};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationCreated {
    pub conversation_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
}

impl AgentConversationCreated {
    pub fn envelope(
        conversation: &AgentConversation,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "agent.conversation.created".into(),
            schema_version: 1,
            organization_id: conversation.organization_id.as_uuid(),
            aggregate_id: conversation.id.as_uuid(),
            aggregate_version: conversation.aggregate_version,
            occurred_at: conversation.created_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                conversation_id: conversation.id.as_uuid(),
                project_id: conversation.project_id.as_uuid(),
                environment_id: conversation.environment_id.as_uuid(),
            })?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionStarted {
    pub conversation_id: Uuid,
    pub execution_id: Uuid,
    pub operation_id: Uuid,
    pub agent_asset_id: Uuid,
    pub agent_asset_release_id: Uuid,
    pub agent_artifact_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionCheckpointCommitted {
    pub conversation_id: Uuid,
    pub execution_id: Uuid,
    pub checkpoint_id: Uuid,
    pub through_event_sequence: u64,
    pub object_digest: String,
}

impl AgentExecutionCheckpointCommitted {
    pub fn envelope(
        checkpoint: &AgentExecutionCheckpoint,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "agent.execution-checkpoint.committed".into(),
            schema_version: 1,
            organization_id: checkpoint.organization_id.as_uuid(),
            aggregate_id: checkpoint.id.as_uuid(),
            aggregate_version: checkpoint.aggregate_version,
            occurred_at: checkpoint.captured_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                conversation_id: checkpoint.conversation_id.as_uuid(),
                execution_id: checkpoint.execution_id.as_uuid(),
                checkpoint_id: checkpoint.id.as_uuid(),
                through_event_sequence: checkpoint.through_event_sequence,
                object_digest: checkpoint.object.digest.as_str().into(),
            })?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionForked {
    pub conversation_id: Uuid,
    pub execution_id: Uuid,
    pub operation_id: Uuid,
    pub parent_execution_id: Uuid,
    pub parent_checkpoint_id: Uuid,
    pub parent_checkpoint_digest: String,
    pub depth: u16,
}

impl AgentExecutionForked {
    pub fn envelope(
        execution: &AgentExecution,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        execution.validate()?;
        let lineage = execution
            .lineage
            .as_ref()
            .ok_or_else(|| "Agent execution fork event requires lineage".to_owned())?;
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "agent.execution.forked".into(),
            schema_version: 1,
            organization_id: execution.organization_id.as_uuid(),
            aggregate_id: execution.id.as_uuid(),
            aggregate_version: execution.aggregate_version,
            occurred_at: execution.requested_at,
            correlation_id,
            causation_id: Some(lineage.parent_checkpoint_id.as_uuid()),
            payload: serde_json::to_value(Self {
                conversation_id: execution.conversation_id.as_uuid(),
                execution_id: execution.id.as_uuid(),
                operation_id: execution.operation_id.as_uuid(),
                parent_execution_id: lineage.parent_execution_id.as_uuid(),
                parent_checkpoint_id: lineage.parent_checkpoint_id.as_uuid(),
                parent_checkpoint_digest: lineage.parent_checkpoint_digest.as_str().into(),
                depth: lineage.depth,
            })
            .map_err(|error| error.to_string())?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionCancellationRequested {
    pub conversation_id: Uuid,
    pub execution_id: Uuid,
    pub operation_id: Uuid,
}

impl AgentExecutionCancellationRequested {
    pub fn envelope(
        execution: &AgentExecution,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "agent.execution.cancellation-requested".into(),
            schema_version: 1,
            organization_id: execution.organization_id.as_uuid(),
            aggregate_id: execution.id.as_uuid(),
            aggregate_version: execution.aggregate_version,
            occurred_at: execution.updated_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                conversation_id: execution.conversation_id.as_uuid(),
                execution_id: execution.id.as_uuid(),
                operation_id: execution.operation_id.as_uuid(),
            })?,
        })
    }
}

impl AgentExecutionStarted {
    pub fn envelope(
        execution: &AgentExecution,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "agent.execution.started".into(),
            schema_version: 1,
            organization_id: execution.organization_id.as_uuid(),
            aggregate_id: execution.id.as_uuid(),
            aggregate_version: execution.aggregate_version,
            occurred_at: execution.requested_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                conversation_id: execution.conversation_id.as_uuid(),
                execution_id: execution.id.as_uuid(),
                operation_id: execution.operation_id.as_uuid(),
                agent_asset_id: execution.agent.asset_id().as_uuid(),
                agent_asset_release_id: execution.agent.asset_release_id().as_uuid(),
                agent_artifact_digest: execution.agent.artifact_digest().as_str().into(),
            })?,
        })
    }
}
