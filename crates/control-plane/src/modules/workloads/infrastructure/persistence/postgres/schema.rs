use a3s_orm::orm_table;
use chrono::{DateTime, Utc};
use serde_json::Value;
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
        agent_release_contract: Option<Value> => "agent_release_contract",
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
    pub(super) struct WorkloadRevisionSkillBindings => "workload_revision_skill_bindings" {
        organization_id: Uuid => "organization_id",
        workload_id: Uuid => "workload_id",
        revision_id: Uuid => "revision_id",
        asset_id: Uuid => "asset_id",
        asset_release_id: Uuid => "asset_release_id",
        artifact_digest: String => "artifact_digest",
        artifact_size_bytes: u64 => "artifact_size_bytes",
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
        node_pool_id: Option<Uuid> => "node_pool_id",
        members_per_replica: u32 => "members_per_replica",
        placement_topology: String => "placement_topology",
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
        revision_generation: u64 => "revision_generation",
        generation: u64 => "generation",
        lifecycle: String => "lifecycle",
        evacuation_node_id: Option<Uuid> => "evacuation_node_id",
        retirement_command_id: Option<Uuid> => "retirement_command_id",
        runtime_fenced_at: Option<DateTime<Utc>> => "runtime_fenced_at",
        aggregate_version: u64 => "aggregate_version",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
    }
}

orm_table! {
    pub(super) struct WorkloadWriterFenceReceipts => "workload_writer_fence_receipts" {
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        workload_id: Uuid => "workload_id",
        workload_revision_id: Uuid => "workload_revision_id",
        workload_revision_generation: u64 => "workload_revision_generation",
        replica_id: Uuid => "replica_id",
        replica_ordinal: u32 => "replica_ordinal",
        writer_epoch: u64 => "writer_epoch",
        member_id: Uuid => "member_id",
        placement_generation: u64 => "placement_generation",
        managed_owner_kind: String => "managed_owner_kind",
        managed_owner_id: Uuid => "managed_owner_id",
        managed_owner_generation: u64 => "managed_owner_generation",
        managed_owner_spec_digest: String => "managed_owner_spec_digest",
        node_id: Uuid => "node_id",
        runtime_unit_id: String => "runtime_unit_id",
        command_id: Uuid => "command_id",
        command_kind: String => "command_kind",
        command_payload_digest: String => "command_payload_digest",
        acknowledgement_digest: String => "acknowledgement_digest",
        continuation_operation_id: Uuid => "continuation_operation_id",
        receipt_schema: String => "receipt_schema",
        receipt_digest: String => "receipt_digest",
        fenced_at: DateTime<Utc> => "fenced_at",
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
    pub(super) struct WorkloadPlacementGroups => "workload_placement_groups" {
        id: Uuid => "id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        workload_id: Uuid => "workload_id",
        revision_id: Uuid => "revision_id",
        revision_generation: u64 => "revision_generation",
        replica_id: Uuid => "replica_id",
        replica_generation: u64 => "replica_generation",
        policy_generation: u64 => "policy_generation",
        placement_policy_digest: String => "placement_policy_digest",
        plan_schema: String => "plan_schema",
        plan_digest: String => "plan_digest",
        state: String => "state",
        member_count: u32 => "member_count",
        aggregate_version: u64 => "aggregate_version",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
    }
}

orm_table! {
    pub(super) struct WorkloadPlacementGroupMembers => "workload_placement_group_members" {
        organization_id: Uuid => "organization_id",
        group_id: Uuid => "group_id",
        workload_id: Uuid => "workload_id",
        replica_id: Uuid => "replica_id",
        member_id: Uuid => "member_id",
        ordinal: u32 => "ordinal",
        role: String => "role",
        runtime_unit_id: String => "runtime_unit_id",
        template: serde_json::Value => "template",
        template_digest: String => "template_digest",
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
    pub(super) struct DeploymentReplicaMemberBindings => "deployment_replica_member_bindings" {
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
    pub(super) struct DeploymentPlacementGroupBindings => "deployment_placement_group_bindings" {
        deployment_id: Uuid => "deployment_id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        workload_id: Uuid => "workload_id",
        revision_id: Uuid => "revision_id",
        revision_generation: u64 => "revision_generation",
        replica_id: Uuid => "replica_id",
        replica_generation: u64 => "replica_generation",
        group_id: Uuid => "group_id",
        group_plan_digest: String => "group_plan_digest",
        member_count: u32 => "member_count",
        created_at: DateTime<Utc> => "created_at",
        updated_at: DateTime<Utc> => "updated_at",
    }
}

orm_table! {
    pub(super) struct DeploymentRuntimeExecutionBindings => "deployment_runtime_execution_bindings" {
        deployment_id: Uuid => "deployment_id",
        organization_id: Uuid => "organization_id",
        project_id: Uuid => "project_id",
        environment_id: Uuid => "environment_id",
        workload_id: Uuid => "workload_id",
        workload_revision_id: Uuid => "workload_revision_id",
        node_pool_id: Option<Uuid> => "node_pool_id",
        binding_schema: String => "binding_schema",
        runtime_class: Option<String> => "runtime_class",
        isolation_level: Option<String> => "isolation_level",
        semantics_profile_digest: Option<String> => "semantics_profile_digest",
        identity_attachment_digest: Option<String> => "identity_attachment_digest",
        authorized_at: Option<DateTime<Utc>> => "authorized_at",
        admitted_at: DateTime<Utc> => "admitted_at",
        binding_digest: String => "binding_digest",
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
