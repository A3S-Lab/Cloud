use super::{
    ResourceAllocation, ResourceKind, ResourceSlotRequest, ResourceUnit, ServiceResources,
};
use a3s_cloud_contracts::NodeResourceInventory;
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledResourceRequirements {
    pub slots: Vec<ResourceSlotRequest>,
    pub topology_digest: String,
}

impl CompiledResourceRequirements {
    pub fn compile(
        resources: &ServiceResources,
        inventory: &NodeResourceInventory,
    ) -> Result<Self, String> {
        inventory.validate()?;
        if resources.cpu_millis == 0
            || resources.memory_bytes == 0
            || resources.pids == 0
            || resources.ephemeral_storage_bytes == Some(0)
        {
            return Err("service resource requirements are invalid".into());
        }

        let mut slots = vec![
            select_scalar_slot(
                inventory,
                ResourceKind::Cpu,
                resources.cpu_millis,
                ResourceUnit::MilliCpu,
            )?,
            select_scalar_slot(
                inventory,
                ResourceKind::Memory,
                resources.memory_bytes,
                ResourceUnit::Byte,
            )?,
        ];
        if let Some(bytes) = resources.ephemeral_storage_bytes {
            slots.push(select_scalar_slot(
                inventory,
                ResourceKind::EphemeralStorage,
                bytes,
                ResourceUnit::Byte,
            )?);
        }
        slots.sort_by(|left, right| {
            (left.kind, left.stable_resource_id.as_str())
                .cmp(&(right.kind, right.stable_resource_id.as_str()))
        });
        let topology_digest = topology_digest(&slots)?;
        Ok(Self {
            slots,
            topology_digest,
        })
    }
}

fn select_scalar_slot(
    inventory: &NodeResourceInventory,
    kind: ResourceKind,
    requested: u64,
    unit: ResourceUnit,
) -> Result<ResourceSlotRequest, String> {
    let requested_allocation = ResourceAllocation::Scalar {
        amount: requested,
        unit,
    };
    let mut saw_compatible_slot = false;
    for slot in inventory.slots.iter().filter(|slot| slot.kind == kind) {
        if !matches!(
            slot.allocation,
            ResourceAllocation::Scalar {
                unit: candidate_unit,
                ..
            } if candidate_unit == unit
        ) {
            continue;
        }
        saw_compatible_slot = true;
        if slot.allocation.contains(&requested_allocation) {
            return ResourceSlotRequest::new(
                kind,
                slot.stable_resource_id.clone(),
                requested_allocation,
            );
        }
    }
    let label = kind.as_str();
    if saw_compatible_slot {
        Err(format!(
            "node inventory has insufficient {label} capacity for the service requirement"
        ))
    } else {
        Err(format!(
            "node inventory does not prove compatible {label} capacity"
        ))
    }
}

fn topology_digest(slots: &[ResourceSlotRequest]) -> Result<String, String> {
    #[derive(Serialize)]
    struct Topology<'a> {
        schema: &'static str,
        slots: &'a [ResourceSlotRequest],
    }

    let document = serde_json::to_vec(&Topology {
        schema: "a3s.cloud.resource-topology.v1",
        slots,
    })
    .map_err(|error| format!("could not encode resource topology: {error}"))?;
    Ok(format!("sha256:{:x}", Sha256::digest(document)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::NodeResourceSlot;
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn compiles_canonical_scalar_requirements_from_proven_inventory() {
        let inventory = inventory(vec![
            slot(
                ResourceKind::Memory,
                "memory/system",
                1_024,
                ResourceUnit::Byte,
            ),
            slot(
                ResourceKind::Cpu,
                "cpu/shared",
                2_000,
                ResourceUnit::MilliCpu,
            ),
            slot(
                ResourceKind::EphemeralStorage,
                "ephemeral-storage/state-filesystem",
                4_096,
                ResourceUnit::Byte,
            ),
        ]);
        let compiled = CompiledResourceRequirements::compile(
            &ServiceResources {
                cpu_millis: 500,
                memory_bytes: 512,
                pids: 32,
                ephemeral_storage_bytes: Some(2_048),
            },
            &inventory,
        )
        .expect("compiled requirements");

        assert_eq!(
            compiled
                .slots
                .iter()
                .map(|slot| (slot.kind, slot.stable_resource_id.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (ResourceKind::Cpu, "cpu/shared"),
                (ResourceKind::Memory, "memory/system"),
                (
                    ResourceKind::EphemeralStorage,
                    "ephemeral-storage/state-filesystem"
                ),
            ]
        );
        assert!(compiled.topology_digest.starts_with("sha256:"));
    }

    #[test]
    fn fails_closed_when_memory_is_missing_or_capacity_is_insufficient() {
        let cpu_only = inventory(vec![slot(
            ResourceKind::Cpu,
            "cpu/shared",
            1_000,
            ResourceUnit::MilliCpu,
        )]);
        let resources = ServiceResources {
            cpu_millis: 250,
            memory_bytes: 512,
            pids: 32,
            ephemeral_storage_bytes: None,
        };
        assert!(CompiledResourceRequirements::compile(&resources, &cpu_only)
            .expect_err("missing memory must fail")
            .contains("does not prove compatible memory"));

        let undersized = inventory(vec![
            slot(ResourceKind::Cpu, "cpu/shared", 100, ResourceUnit::MilliCpu),
            slot(
                ResourceKind::Memory,
                "memory/system",
                1_024,
                ResourceUnit::Byte,
            ),
        ]);
        assert!(
            CompiledResourceRequirements::compile(&resources, &undersized)
                .expect_err("undersized CPU must fail")
                .contains("insufficient cpu capacity")
        );
    }

    fn inventory(slots: Vec<NodeResourceSlot>) -> NodeResourceInventory {
        NodeResourceInventory::new(Uuid::now_v7(), Uuid::now_v7(), 1, Utc::now(), slots)
            .expect("inventory")
    }

    fn slot(
        kind: ResourceKind,
        stable_resource_id: &str,
        amount: u64,
        unit: ResourceUnit,
    ) -> NodeResourceSlot {
        NodeResourceSlot::new(
            kind,
            stable_resource_id,
            ResourceAllocation::Scalar { amount, unit },
        )
        .expect("slot")
    }
}
