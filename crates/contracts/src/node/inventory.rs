use crate::{ResourceAllocation, ResourceKind};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{validate_sha256, validate_uuid};
use crate::resource::validate_stable_resource_id;

const MAX_INVENTORY_SLOTS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeResourceSlot {
    pub kind: ResourceKind,
    pub stable_resource_id: String,
    pub allocation: ResourceAllocation,
}

impl NodeResourceSlot {
    pub fn new(
        kind: ResourceKind,
        stable_resource_id: impl Into<String>,
        allocation: ResourceAllocation,
    ) -> Result<Self, String> {
        let slot = Self {
            kind,
            stable_resource_id: stable_resource_id.into(),
            allocation,
        };
        slot.validate()?;
        Ok(slot)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_stable_resource_id(&self.stable_resource_id)?;
        self.allocation.validate_for(self.kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeInventoryReference {
    pub generation: u64,
    pub digest: String,
}

impl NodeInventoryReference {
    pub fn validate(&self) -> Result<(), String> {
        if self.generation == 0 {
            return Err("node inventory generation must be positive".into());
        }
        validate_sha256("node inventory digest", &self.digest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeResourceInventory {
    pub schema: String,
    pub node_id: Uuid,
    pub agent_instance_id: Uuid,
    pub generation: u64,
    pub observed_at: DateTime<Utc>,
    pub slots: Vec<NodeResourceSlot>,
    pub digest: String,
}

impl NodeResourceInventory {
    pub const SCHEMA: &'static str = "a3s.cloud.node-resource-inventory.v1";
    const CONTENT_SCHEMA: &'static str = "a3s.cloud.node-resource-inventory-content.v1";

    pub fn new(
        node_id: Uuid,
        agent_instance_id: Uuid,
        generation: u64,
        observed_at: DateTime<Utc>,
        mut slots: Vec<NodeResourceSlot>,
    ) -> Result<Self, String> {
        slots.sort_by(|left, right| {
            (left.kind, left.stable_resource_id.as_str())
                .cmp(&(right.kind, right.stable_resource_id.as_str()))
        });
        let digest = Self::calculate_digest(&slots)?;
        let inventory = Self {
            schema: Self::SCHEMA.into(),
            node_id,
            agent_instance_id,
            generation,
            observed_at,
            slots,
            digest,
        };
        inventory.validate()?;
        Ok(inventory)
    }

    pub fn reference(&self) -> NodeInventoryReference {
        NodeInventoryReference {
            generation: self.generation,
            digest: self.digest.clone(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node resource inventory schema {:?}",
                self.schema
            ));
        }
        validate_uuid("node_id", self.node_id)?;
        validate_uuid("agent_instance_id", self.agent_instance_id)?;
        if self.generation == 0 || self.observed_at.timestamp_millis() <= 0 {
            return Err("node resource inventory generation or observation time is invalid".into());
        }
        validate_inventory_slots(&self.slots)?;
        validate_sha256("node inventory digest", &self.digest)?;
        if self.digest != Self::calculate_digest(&self.slots)? {
            return Err("node inventory digest does not match its canonical slots".into());
        }
        Ok(())
    }

    fn calculate_digest(slots: &[NodeResourceSlot]) -> Result<String, String> {
        #[derive(Serialize)]
        struct InventoryContent<'a> {
            schema: &'static str,
            slots: &'a [NodeResourceSlot],
        }

        let content = serde_json::to_vec(&InventoryContent {
            schema: Self::CONTENT_SCHEMA,
            slots,
        })
        .map_err(|error| format!("could not encode node inventory content: {error}"))?;
        Ok(format!("sha256:{:x}", Sha256::digest(content)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeResourceInventoryReceipt {
    pub schema: String,
    pub node_id: Uuid,
    pub generation: u64,
    pub digest: String,
    pub replayed: bool,
}

impl NodeResourceInventoryReceipt {
    pub const SCHEMA: &'static str = "a3s.cloud.node-resource-inventory-receipt.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node resource inventory receipt schema {:?}",
                self.schema
            ));
        }
        validate_uuid("node_id", self.node_id)?;
        NodeInventoryReference {
            generation: self.generation,
            digest: self.digest.clone(),
        }
        .validate()
    }
}

fn validate_inventory_slots(slots: &[NodeResourceSlot]) -> Result<(), String> {
    if slots.is_empty() || slots.len() > MAX_INVENTORY_SLOTS {
        return Err("node resource inventory must contain 1 to 256 slots".into());
    }
    let mut previous = None;
    for slot in slots {
        slot.validate()?;
        let key = (slot.kind, slot.stable_resource_id.as_str());
        if previous.is_some_and(|candidate| candidate >= key) {
            return Err(
                "node resource inventory slots must be uniquely and canonically sorted".into(),
            );
        }
        previous = Some(key);
    }
    Ok(())
}
