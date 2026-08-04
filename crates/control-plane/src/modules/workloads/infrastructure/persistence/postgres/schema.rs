use a3s_orm::orm_table;
use chrono::{DateTime, Utc};
use uuid::Uuid;

orm_table! {
    pub(super) struct Workloads => "workloads" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        name: String => "name",
        name_key: String => "name_key",
        desired_state: String => "desired_state",
        active_revision_id: Option<Uuid> => "active_revision_id",
        aggregate_version: u64 => "aggregate_version",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
    }
}

orm_table! {
    pub(super) struct ActiveWorkloads => "active_workloads" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        desired_state: String => "desired_state",
        active_revision_id: Uuid => "active_revision_id",
        updated_at: DateTime<Utc> => "updated_at",
    }
}

orm_table! {
    pub(super) struct WorkloadRevisions => "workload_revisions" {
        id: Uuid => "id",
        workload_id: Uuid => "workload_id",
        generation: u64 => "generation",
        resolution_state: String => "resolution_state",
        artifact_source_uri: String => "artifact_source_uri",
        expected_artifact_digest: Option<String> => "expected_artifact_digest",
        template_request: serde_json::Value => "template_request",
        request_digest: String => "request_digest",
        artifact_uri: Option<String> => "artifact_uri",
        artifact_digest: Option<String> => "artifact_digest",
        artifact_media_type: Option<String> => "artifact_media_type",
        template: Option<serde_json::Value> => "template",
        template_digest: Option<String> => "template_digest",
        created_at: DateTime<Utc> => "created_at",
        resolved_at: Option<DateTime<Utc>> => "resolved_at",
        external_build_organization_id: Option<Uuid> => "external_build_organization_id",
        external_build_project_id: Option<Uuid> => "external_build_project_id",
        external_build_environment_id: Option<Uuid> => "external_build_environment_id",
        external_source_revision_id: Option<Uuid> => "external_source_revision_id",
        external_build_run_id: Option<Uuid> => "external_build_run_id",
        agent_organization_id: Option<Uuid> => "agent_organization_id",
        agent_asset_id: Option<Uuid> => "agent_asset_id",
        agent_asset_release_id: Option<Uuid> => "agent_asset_release_id",
        agent_build_run_id: Option<Uuid> => "agent_build_run_id",
        mcp_organization_id: Option<Uuid> => "mcp_organization_id",
        mcp_asset_id: Option<Uuid> => "mcp_asset_id",
        mcp_asset_release_id: Option<Uuid> => "mcp_asset_release_id",
        mcp_profile_digest: Option<String> => "mcp_profile_digest",
    }
}

orm_table! {
    pub(super) struct McpServiceProfiles => "mcp_service_profiles" {
        organization_id: Uuid => "organization_id",
        asset_id: Uuid => "asset_id",
        asset_release_id: Uuid => "asset_release_id",
        profile_digest: String => "profile_digest",
        acl: String => "acl",
    }
}

orm_table! {
    pub(super) struct Deployments => "deployments" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        workload_id: Uuid => "workload_id",
        revision_id: Uuid => "revision_id",
        operation_id: Uuid => "operation_id",
        node_id: Option<Uuid> => "node_id",
        command_id: Option<Uuid> => "command_id",
        cleanup_command_id: Option<Uuid> => "cleanup_command_id",
        retirement_command_id: Option<Uuid> => "retirement_command_id",
        status: String => "status",
        failure: Option<String> => "failure",
        aggregate_version: u64 => "aggregate_version",
        requested_at: DateTime<Utc> => "requested_at",
        updated_at: DateTime<Utc> => "updated_at",
        activated_at: Option<DateTime<Utc>> => "activated_at",
        cancellation_requested_at: Option<DateTime<Utc>> => "cancellation_requested_at",
        cancelled_at: Option<DateTime<Utc>> => "cancelled_at",
    }
}

orm_table! {
    pub(super) struct OperationRequests => "operation_requests" {
        operation_id: Uuid => "operation_id",
        organization_id: Uuid => "organization_id",
        subject_kind: String => "subject_kind",
        subject_id: Uuid => "subject_id",
        workflow_name: String => "workflow_name",
        workflow_version: String => "workflow_version",
        input: serde_json::Value => "input",
        requested_at: DateTime<Utc> => "requested_at",
    }
}

orm_table! {
    pub(super) struct Secrets => "secrets" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        state: String => "state",
        current_version: u64 => "current_version",
    }
}

orm_table! {
    pub(super) struct SecretVersions => "secret_versions" {
        secret_id: Uuid => "secret_id",
        version: u64 => "version",
        state: String => "state",
    }
}

orm_table! {
    pub(super) struct SecretRotationRestarts => "secret_rotation_restarts" {
        secret_event_id: Uuid => "secret_event_id",
        organization_id: Uuid => "organization_id",
        secret_id: Uuid => "secret_id",
        secret_version: u64 => "secret_version",
        workload_id: Uuid => "workload_id",
        source_revision_id: Uuid => "source_revision_id",
        target_revision_id: Uuid => "target_revision_id",
        deployment_id: Uuid => "deployment_id",
        operation_id: Uuid => "operation_id",
        created_at: DateTime<Utc> => "created_at",
    }
}

orm_table! {
    pub(super) struct SecretRotationReconciliations => "secret_rotation_reconciliations" {
        secret_event_id: Uuid => "secret_event_id",
        organization_id: Uuid => "organization_id",
        secret_id: Uuid => "secret_id",
        secret_version: u64 => "secret_version",
        outcome: String => "outcome",
        restart_count: i64 => "restart_count",
        reconciled_at: DateTime<Utc> => "reconciled_at",
    }
}

orm_table! {
    pub(super) struct WorkloadControls => "workload_controls" {
        workload_id: Uuid => "workload_id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        managed_owner_kind: Option<String> => "managed_owner_kind",
        managed_owner_id: Option<Uuid> => "managed_owner_id",
        managed_owner_generation: Option<u64> => "managed_owner_generation",
        managed_owner_spec_digest: Option<String> => "managed_owner_spec_digest",
        placement_policy: serde_json::Value => "placement_policy",
        placement_policy_digest: String => "placement_policy_digest",
        aggregate_version: u64 => "aggregate_version",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
    }
}

orm_table! {
    pub(super) struct WorkloadReplicas => "workload_replicas" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        workload_id: Uuid => "workload_id",
        ordinal: u32 => "ordinal",
        revision_id: Uuid => "revision_id",
        generation: u64 => "generation",
        aggregate_version: u64 => "aggregate_version",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
    }
}

orm_table! {
    pub(super) struct WorkloadReplicaMembers => "workload_replica_members" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        workload_id: Uuid => "workload_id",
        replica_id: Uuid => "replica_id",
        ordinal: u32 => "ordinal",
        node_id: Option<Uuid> => "node_id",
        placement_generation: u64 => "placement_generation",
        aggregate_version: u64 => "aggregate_version",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
    }
}

orm_table! {
    pub(super) struct DeploymentReplicaBindings => "deployment_replica_bindings" {
        deployment_id: Uuid => "deployment_id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        workload_id: Uuid => "workload_id",
        revision_id: Uuid => "revision_id",
        replica_id: Uuid => "replica_id",
        replica_generation: u64 => "replica_generation",
        member_id: Uuid => "member_id",
        node_id: Option<Uuid> => "node_id",
        placement_generation: u64 => "placement_generation",
        runtime_unit_id: String => "runtime_unit_id",
        runtime_generation: u64 => "runtime_generation",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
    }
}

orm_table! {
    pub(super) struct ResourceClaims => "resource_claims" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        workload_id: Uuid => "workload_id",
        deployment_id: Uuid => "deployment_id",
        replica_id: Uuid => "replica_id",
        replica_generation: u64 => "replica_generation",
        member_id: Uuid => "member_id",
        placement_generation: u64 => "placement_generation",
        node_id: Uuid => "node_id",
        inventory_generation: u64 => "inventory_generation",
        inventory_digest: String => "inventory_digest",
        runtime_unit_id: String => "runtime_unit_id",
        runtime_generation: u64 => "runtime_generation",
        topology_digest: String => "topology_digest",
        reservation_digest: String => "reservation_digest",
        claim_generation: u64 => "claim_generation",
        claim_digest: String => "claim_digest",
        state: String => "state",
        prepare_command_id: Option<Uuid> => "prepare_command_id",
        prepared_binding_digest: Option<String> => "prepared_binding_digest",
        release_command_id: Option<Uuid> => "release_command_id",
        release_evidence: Option<serde_json::Value> => "release_evidence",
        failure: Option<String> => "failure",
        aggregate_version: u64 => "aggregate_version",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
        prepared_at: Option<DateTime<Utc>> => "prepared_at",
        bound_at: Option<DateTime<Utc>> => "bound_at",
        release_requested_at: Option<DateTime<Utc>> => "release_requested_at",
        released_at: Option<DateTime<Utc>> => "released_at",
        orphaned_at: Option<DateTime<Utc>> => "orphaned_at",
    }
}

orm_table! {
    pub(super) struct ResourceClaimSlots => "resource_claim_slots" {
        claim_id: Uuid => "claim_id",
        ordinal: u32 => "ordinal",
        organization_id: Uuid => "organization_id",
        node_id: Uuid => "node_id",
        resource_kind: String => "resource_kind",
        stable_resource_id: String => "stable_resource_id",
        allocation: serde_json::Value => "allocation",
        slot_generation: u64 => "slot_generation",
        fence_token: Uuid => "fence_token",
        created_at: DateTime<Utc> => "created_at",
        released_at: Option<DateTime<Utc>> => "released_at",
    }
}

orm_table! {
    pub(super) struct ResourceSlotLeases => "resource_slot_leases" {
        organization_id: Uuid => "organization_id",
        node_id: Uuid => "node_id",
        resource_kind: String => "resource_kind",
        stable_resource_id: String => "stable_resource_id",
        slot_generation: u64 => "slot_generation",
        fence_token: Uuid => "fence_token",
        active_claim_id: Option<Uuid> => "active_claim_id",
        updated_at: DateTime<Utc> => "updated_at",
    }
}
