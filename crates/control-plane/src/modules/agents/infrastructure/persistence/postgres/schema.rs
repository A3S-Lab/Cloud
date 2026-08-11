use a3s_orm::orm_table;
use chrono::{DateTime, Utc};
use uuid::Uuid;

orm_table! {
    pub(super) struct AgentConversations => "agent_conversations" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        id: Uuid => "id",
        status: String => "status",
        last_event_sequence: u64 => "last_event_sequence",
        aggregate_version: u64 => "aggregate_version",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
        closed_at: Option<DateTime<Utc>> => "closed_at",
    }
}

orm_table! {
    pub(super) struct AgentExecutions => "agent_executions" {
        organization_id: Uuid => "organization_id",
        conversation_id: Uuid => "conversation_id",
        id: Uuid => "id",
        operation_id: Uuid => "operation_id",
        agent_asset_id: Uuid => "agent_asset_id",
        agent_asset_release_id: Uuid => "agent_asset_release_id",
        agent_build_run_id: Uuid => "agent_build_run_id",
        agent_artifact_uri: String => "agent_artifact_uri",
        agent_artifact_digest: String => "agent_artifact_digest",
        agent_artifact_media_type: String => "agent_artifact_media_type",
        agent_artifact_size_bytes: u64 => "agent_artifact_size_bytes",
        status: String => "status",
        failure: Option<String> => "failure",
        aggregate_version: u64 => "aggregate_version",
        requested_at: DateTime<Utc> => "requested_at",
        updated_at: DateTime<Utc> => "updated_at",
        started_at: Option<DateTime<Utc>> => "started_at",
        cancellation_requested_at: Option<DateTime<Utc>> => "cancellation_requested_at",
        finished_at: Option<DateTime<Utc>> => "finished_at",
        code_node_id: Option<Uuid> => "code_node_id",
        code_workload_id: Option<Uuid> => "code_workload_id",
        code_workload_revision_id: Option<Uuid> => "code_workload_revision_id",
        code_deployment_id: Option<Uuid> => "code_deployment_id",
        code_replica_id: Option<Uuid> => "code_replica_id",
        code_runtime_unit_id: Option<String> => "code_runtime_unit_id",
        code_runtime_generation: Option<u64> => "code_runtime_generation",
        code_runtime_spec_digest: Option<String> => "code_runtime_spec_digest",
        code_service_port_name: Option<String> => "code_service_port_name",
        code_protocol: Option<String> => "code_protocol",
        code_release_identity: Option<String> => "code_release_identity",
        code_session_id: Option<String> => "code_session_id",
        code_run_id: Option<String> => "code_run_id",
        code_event_cursor: Option<u64> => "code_event_cursor",
        code_state: Option<String> => "code_state",
        code_bound_at: Option<DateTime<Utc>> => "code_bound_at",
        code_observed_at: Option<DateTime<Utc>> => "code_observed_at",
    }
}

orm_table! {
    pub(super) struct AgentExecutionEvents => "agent_execution_events" {
        organization_id: Uuid => "organization_id",
        conversation_id: Uuid => "conversation_id",
        sequence: u64 => "sequence",
        execution_id: Uuid => "execution_id",
        kind: String => "kind",
        content: serde_json::Value => "content",
        content_digest: String => "content_digest",
        content_size_bytes: u64 => "content_size_bytes",
        occurred_at: DateTime<Utc> => "occurred_at",
    }
}

orm_table! {
    pub(super) struct AgentExecutionChangeSets => "agent_execution_change_sets" {
        organization_id: Uuid => "organization_id",
        execution_id: Uuid => "execution_id",
        batch_id: Uuid => "batch_id",
        node_id: Uuid => "node_id",
        change_set: serde_json::Value => "change_set",
        recorded_at: DateTime<Utc> => "recorded_at",
    }
}
