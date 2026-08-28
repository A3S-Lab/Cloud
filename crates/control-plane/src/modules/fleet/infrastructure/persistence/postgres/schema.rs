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
    pub(super) struct NodePoolMembers => "node_pool_members" {
        organization_id: Uuid => "organization_id",
        node_pool_id: Uuid => "node_pool_id",
        node_id: Uuid => "node_id",
        removal_generation: Option<u64> => "removal_generation",
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

orm_table! {
    pub(super) struct NodeProtocolSessionHeads => "node_protocol_session_heads" {
        organization_id: Uuid => "organization_id",
        node_id: Uuid => "node_id",
        agent_instance_id: Uuid => "agent_instance_id",
        session_epoch: Uuid => "session_epoch",
        hello_sequence: u64 => "hello_sequence",
        session_id: Uuid => "session_id",
        generation: u64 => "generation",
        contracts_digest: String => "contracts_digest",
        selected_at: DateTime<Utc> => "selected_at",
        expires_at: DateTime<Utc> => "expires_at",
        hello: serde_json::Value => "hello",
        selection: serde_json::Value => "selection",
    }
}
