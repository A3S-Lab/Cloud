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
        provider_kind: Option<String> => "provider_kind",
        provider_revision: Option<String> => "provider_revision",
        provider_protocol: Option<String> => "provider_protocol",
        provider_native_protocol: Option<String> => "provider_native_protocol",
        provider_profile_acl: Option<String> => "provider_profile_acl",
        provider_profile_digest: Option<String> => "provider_profile_digest",
        provider_capability_digest: Option<String> => "provider_capability_digest",
        provider_node_id: Option<Uuid> => "provider_node_id",
        provider_workload_id: Option<Uuid> => "provider_workload_id",
        provider_workload_revision_id: Option<Uuid> => "provider_workload_revision_id",
        provider_deployment_id: Option<Uuid> => "provider_deployment_id",
        provider_replica_id: Option<Uuid> => "provider_replica_id",
        provider_runtime_unit_id: Option<String> => "provider_runtime_unit_id",
        provider_runtime_generation: Option<u64> => "provider_runtime_generation",
        provider_runtime_spec_digest: Option<String> => "provider_runtime_spec_digest",
        provider_service_port_name: Option<String> => "provider_service_port_name",
        provider_release_identity: Option<String> => "provider_release_identity",
        provider_session_id: Option<String> => "provider_session_id",
        provider_run_id: Option<String> => "provider_run_id",
        provider_event_cursor: Option<u64> => "provider_event_cursor",
        provider_state: Option<String> => "provider_state",
        provider_bound_at: Option<DateTime<Utc>> => "provider_bound_at",
        provider_observed_at: Option<DateTime<Utc>> => "provider_observed_at",
        invocation_profile: Option<serde_json::Value> => "invocation_profile",
        invocation_profile_digest: Option<String> => "invocation_profile_digest",
        parent_execution_id: Option<Uuid> => "parent_execution_id",
        parent_checkpoint_id: Option<Uuid> => "parent_checkpoint_id",
        parent_checkpoint_digest: Option<String> => "parent_checkpoint_digest",
        fork_depth: Option<u16> => "fork_depth",
    }
}

orm_table! {
    pub(super) struct AgentExecutionCheckpoints => "agent_execution_checkpoints" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        conversation_id: Uuid => "conversation_id",
        execution_id: Uuid => "execution_id",
        id: Uuid => "id",
        through_event_sequence: u64 => "through_event_sequence",
        event_count: u16 => "event_count",
        agent_artifact_digest: String => "agent_artifact_digest",
        provider_profile_digest: String => "provider_profile_digest",
        invocation_profile_digest: String => "invocation_profile_digest",
        object_schema: String => "object_schema",
        object_namespace: String => "object_namespace",
        object_ref: String => "object_ref",
        object_digest: String => "object_digest",
        object_size_bytes: u64 => "object_size_bytes",
        object_media_type: String => "object_media_type",
        operation_id: Uuid => "operation_id",
        provider_run_identity_digest: String => "provider_run_identity_digest",
        node_id: Uuid => "node_id",
        workload_id: Uuid => "workload_id",
        workload_revision_id: Uuid => "workload_revision_id",
        deployment_id: Uuid => "deployment_id",
        replica_id: Uuid => "replica_id",
        runtime_unit_id: String => "runtime_unit_id",
        runtime_generation: u64 => "runtime_generation",
        aggregate_version: u64 => "aggregate_version",
        captured_at: DateTime<Utc> => "captured_at",
    }
}

orm_table! {
    pub(super) struct AgentApprovalCheckpoints => "agent_approval_checkpoints" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        conversation_id: Uuid => "conversation_id",
        execution_id: Uuid => "execution_id",
        id: Uuid => "id",
        provider_run_identity_digest: String => "provider_run_identity_digest",
        invocation_profile_digest: String => "invocation_profile_digest",
        source_event_sequence: u64 => "source_event_sequence",
        call_id: String => "call_id",
        tool_name: String => "tool_name",
        tool_revision: String => "tool_revision",
        tool_contract_digest: String => "tool_contract_digest",
        request_digest: String => "request_digest",
        request_size_bytes: u64 => "request_size_bytes",
        request_media_type: String => "request_media_type",
        status: String => "status",
        decision_id: Option<Uuid> => "decision_id",
        outcome: Option<String> => "outcome",
        decided_by: Option<Uuid> => "decided_by",
        authorization_decision_id: Option<String> => "authorization_decision_id",
        authorization_decision_digest: Option<String> => "authorization_decision_digest",
        reason: Option<String> => "reason",
        decision_digest: Option<String> => "decision_digest",
        resume_command_id: Option<Uuid> => "resume_command_id",
        resume_command_digest: Option<String> => "resume_command_digest",
        aggregate_version: u64 => "aggregate_version",
        requested_at: DateTime<Utc> => "requested_at",
        expires_at: DateTime<Utc> => "expires_at",
        updated_at: DateTime<Utc> => "updated_at",
        decided_at: Option<DateTime<Utc>> => "decided_at",
        resumed_at: Option<DateTime<Utc>> => "resumed_at",
        cancelled_at: Option<DateTime<Utc>> => "cancelled_at",
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
