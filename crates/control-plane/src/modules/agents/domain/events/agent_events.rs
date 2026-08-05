use crate::modules::agents::domain::{AgentConversation, AgentExecution};
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
