use a3s_orm::orm_table;
use chrono::{DateTime, Utc};
use uuid::Uuid;

orm_table! {
    pub(super) struct Nodes => "nodes" {
        organization_id: Uuid => "organization_id",
        id: Uuid => "id",
        state: String => "state",
        agent_instance_id: Uuid => "agent_instance_id",
    }
}

orm_table! {
    pub(super) struct NodeResourceInventories => "node_resource_inventories" {
        organization_id: Uuid => "organization_id",
        node_id: Uuid => "node_id",
        generation: u64 => "generation",
        inventory_digest: String => "inventory_digest",
        agent_instance_id: Uuid => "agent_instance_id",
        observed_at: DateTime<Utc> => "observed_at",
        received_at: DateTime<Utc> => "received_at",
        snapshot: serde_json::Value => "snapshot",
    }
}

orm_table! {
    pub(super) struct NodeResourceInventorySlots => "node_resource_inventory_slots" {
        organization_id: Uuid => "organization_id",
        node_id: Uuid => "node_id",
        inventory_generation: u64 => "inventory_generation",
        ordinal: u32 => "ordinal",
        resource_kind: String => "resource_kind",
        stable_resource_id: String => "stable_resource_id",
        allocation: serde_json::Value => "allocation",
    }
}

orm_table! {
    pub(super) struct NodeResourceInventoryHeads => "node_resource_inventory_heads" {
        organization_id: Uuid => "organization_id",
        node_id: Uuid => "node_id",
        generation: u64 => "generation",
        inventory_digest: String => "inventory_digest",
        agent_instance_id: Uuid => "agent_instance_id",
        observed_at: DateTime<Utc> => "observed_at",
        received_at: DateTime<Utc> => "received_at",
    }
}
