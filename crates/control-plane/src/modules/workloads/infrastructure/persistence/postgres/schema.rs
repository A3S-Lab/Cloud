use a3s_orm::orm_table;
use chrono::{DateTime, Utc};
use uuid::Uuid;

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
