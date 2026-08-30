use crate::{
    validate_slot_bindings, validate_slot_evidence, NodeResourceInventory, ResourceAllocation,
    ResourceKind, ResourceSlotBinding, ResourceSlotEvidence, ResourceUnit,
};
use a3s_runtime::contract::{RuntimeEvidence, RuntimeObservation, RuntimeUnitSpec};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use uuid::Uuid;

use super::{validate_sha256, validate_single_line, validate_uuid};

pub const RUNTIME_RESOURCE_CLAIM_ID_KEY: &str = "a3s.cloud.resource-claim.id";
pub const RUNTIME_RESOURCE_BINDING_DIGEST_KEY: &str = "a3s.cloud.resource-claim.binding-digest";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeResourceClaimBinding {
    pub schema: String,
    pub claim_id: Uuid,
    pub node_id: Uuid,
    pub agent_instance_id: Uuid,
    pub inventory_generation: u64,
    pub inventory_digest: String,
    pub runtime_unit_id: String,
    pub runtime_generation: u64,
    pub topology_digest: String,
    pub slots: Vec<ResourceSlotBinding>,
}

impl NodeResourceClaimBinding {
    pub const SCHEMA: &'static str = "a3s.cloud.node-resource-claim-binding.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node resource claim binding schema {:?}",
                self.schema
            ));
        }
        validate_uuid("claim_id", self.claim_id)?;
        validate_uuid("node_id", self.node_id)?;
        validate_uuid("agent_instance_id", self.agent_instance_id)?;
        if self.inventory_generation == 0 || self.runtime_generation == 0 {
            return Err("resource claim inventory and Runtime generations must be positive".into());
        }
        validate_sha256("resource inventory digest", &self.inventory_digest)?;
        validate_sha256("resource topology digest", &self.topology_digest)?;
        validate_single_line("Runtime unit ID", &self.runtime_unit_id, 512)?;
        validate_slot_bindings(&self.slots)
    }

    pub fn digest(&self) -> Result<String, String> {
        canonical_digest(self, self.validate())
    }

    pub fn validate_inventory(&self, inventory: &NodeResourceInventory) -> Result<(), String> {
        self.validate()?;
        inventory.validate()?;
        if inventory.node_id != self.node_id
            || inventory.agent_instance_id != self.agent_instance_id
            || inventory.generation != self.inventory_generation
            || inventory.digest != self.inventory_digest
        {
            return Err(
                "resource claim binding does not reference the current Agent inventory".into(),
            );
        }
        for requested in &self.slots {
            let capacity = inventory
                .slots
                .iter()
                .find(|slot| {
                    slot.kind == requested.kind
                        && slot.stable_resource_id == requested.stable_resource_id
                })
                .ok_or_else(|| {
                    format!(
                        "resource claim slot {} is absent from the current Agent inventory",
                        requested.stable_resource_id
                    )
                })?;
            if !capacity.allocation.contains(&requested.allocation) {
                return Err(format!(
                    "resource claim slot {} exceeds the current Agent inventory",
                    requested.stable_resource_id
                ));
            }
        }
        Ok(())
    }

    pub fn validate_runtime_spec(&self, spec: &RuntimeUnitSpec) -> Result<(), String> {
        self.validate()?;
        spec.validate()?;
        if self.runtime_unit_id != spec.unit_id || self.runtime_generation != spec.generation {
            return Err("resource claim binding does not match the Runtime unit identity".into());
        }
        let cpu = required_scalar(&self.slots, ResourceKind::Cpu, ResourceUnit::MilliCpu)?;
        let memory = required_scalar(&self.slots, ResourceKind::Memory, ResourceUnit::Byte)?;
        let ephemeral = optional_scalar(
            &self.slots,
            ResourceKind::EphemeralStorage,
            ResourceUnit::Byte,
        )?;
        if cpu != spec.resources.cpu_millis
            || memory != spec.resources.memory_bytes
            || ephemeral != spec.resources.ephemeral_storage_bytes
        {
            return Err(
                "resource claim scalar allocations do not match the Runtime resource limits".into(),
            );
        }
        if self.slots.iter().any(|slot| {
            !matches!(
                slot.kind,
                ResourceKind::Cpu | ResourceKind::Memory | ResourceKind::EphemeralStorage
            )
        }) {
            return Err(
                "the current Runtime binding protocol cannot enforce an exclusive resource slot"
                    .into(),
            );
        }
        Ok(())
    }

    pub fn bind_runtime_observation(
        &self,
        observation: &mut RuntimeObservation,
    ) -> Result<(), String> {
        self.validate()?;
        observation.validate()?;
        if observation.unit_id != self.runtime_unit_id
            || observation.generation != self.runtime_generation
            || observation.provider_resource_id.is_none()
        {
            return Err(
                "Runtime observation does not prove the prepared resource binding identity".into(),
            );
        }
        let binding_digest = self.digest()?;
        let evidence = observation.evidence.get_or_insert_with(|| RuntimeEvidence {
            provider_build: observation
                .provider_build
                .clone()
                .unwrap_or_else(|| "unknown".into()),
            spec_digest: observation.spec_digest.clone(),
            semantics_profile_digest: None,
            identity_attachment_digest: None,
            claims: BTreeMap::new(),
        });
        insert_exact_claim(
            &mut evidence.claims,
            RUNTIME_RESOURCE_CLAIM_ID_KEY,
            self.claim_id.to_string(),
        )?;
        insert_exact_claim(
            &mut evidence.claims,
            RUNTIME_RESOURCE_BINDING_DIGEST_KEY,
            binding_digest,
        )?;
        observation.validate()
    }

    pub fn validate_runtime_observation(
        &self,
        observation: &RuntimeObservation,
    ) -> Result<(), String> {
        self.validate()?;
        observation.validate()?;
        let evidence = observation.evidence.as_ref().ok_or_else(|| {
            "Runtime observation omitted resource allocation-binding evidence".to_string()
        })?;
        if observation.unit_id != self.runtime_unit_id
            || observation.generation != self.runtime_generation
            || evidence.claims.get(RUNTIME_RESOURCE_CLAIM_ID_KEY)
                != Some(&self.claim_id.to_string())
            || evidence.claims.get(RUNTIME_RESOURCE_BINDING_DIGEST_KEY) != Some(&self.digest()?)
        {
            return Err(
                "Runtime observation resource allocation-binding evidence is inconsistent".into(),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeResourceClaimPrepare {
    pub schema: String,
    pub claim_generation: u64,
    pub claim_digest: String,
    pub binding: NodeResourceClaimBinding,
}

impl NodeResourceClaimPrepare {
    pub const SCHEMA: &'static str = "a3s.cloud.node-resource-claim-prepare.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node resource claim prepare schema {:?}",
                self.schema
            ));
        }
        if self.claim_generation == 0 {
            return Err("resource claim generation must be positive".into());
        }
        validate_sha256("resource claim digest", &self.claim_digest)?;
        self.binding.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeResourceClaimPrepared {
    pub schema: String,
    pub claim_id: Uuid,
    pub claim_generation: u64,
    pub claim_digest: String,
    pub binding_digest: String,
    pub slots: Vec<ResourceSlotEvidence>,
    pub prepared_at: DateTime<Utc>,
}

impl NodeResourceClaimPrepared {
    pub const SCHEMA: &'static str = "a3s.cloud.node-resource-claim-prepared.v1";

    pub fn new(
        request: &NodeResourceClaimPrepare,
        prepared_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        request.validate()?;
        let prepared = Self {
            schema: Self::SCHEMA.into(),
            claim_id: request.binding.claim_id,
            claim_generation: request.claim_generation,
            claim_digest: request.claim_digest.clone(),
            binding_digest: request.binding.digest()?,
            slots: request
                .binding
                .slots
                .iter()
                .map(ResourceSlotBinding::evidence)
                .collect(),
            prepared_at,
        };
        prepared.validate_for(request)?;
        Ok(prepared)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node resource claim prepared schema {:?}",
                self.schema
            ));
        }
        validate_uuid("claim_id", self.claim_id)?;
        if self.claim_generation == 0 || self.prepared_at.timestamp_millis() <= 0 {
            return Err("prepared resource claim generation or time is invalid".into());
        }
        validate_sha256("resource claim digest", &self.claim_digest)?;
        validate_sha256("resource binding digest", &self.binding_digest)?;
        validate_slot_evidence(&self.slots)
    }

    pub fn validate_for(&self, request: &NodeResourceClaimPrepare) -> Result<(), String> {
        self.validate()?;
        request.validate()?;
        let expected_slots = request
            .binding
            .slots
            .iter()
            .map(ResourceSlotBinding::evidence)
            .collect::<Vec<_>>();
        if self.claim_id != request.binding.claim_id
            || self.claim_generation != request.claim_generation
            || self.claim_digest != request.claim_digest
            || self.binding_digest != request.binding.digest()?
            || self.slots != expected_slots
        {
            return Err("prepared resource claim evidence does not match its exact request".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeResourceClaimRelease {
    pub schema: String,
    pub claim_generation: u64,
    pub claim_digest: String,
    pub binding: NodeResourceClaimBinding,
}

impl NodeResourceClaimRelease {
    pub const SCHEMA: &'static str = "a3s.cloud.node-resource-claim-release.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node resource claim release schema {:?}",
                self.schema
            ));
        }
        if self.claim_generation == 0 {
            return Err("resource claim release generation must be positive".into());
        }
        validate_sha256("resource claim digest", &self.claim_digest)?;
        self.binding.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeResourceClaimReleased {
    pub schema: String,
    pub claim_id: Uuid,
    pub claim_generation: u64,
    pub claim_digest: String,
    pub binding_digest: String,
    pub slots: Vec<ResourceSlotEvidence>,
    pub released_at: DateTime<Utc>,
}

impl NodeResourceClaimReleased {
    pub const SCHEMA: &'static str = "a3s.cloud.node-resource-claim-released.v1";

    pub fn new(
        request: &NodeResourceClaimRelease,
        released_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        request.validate()?;
        let released = Self {
            schema: Self::SCHEMA.into(),
            claim_id: request.binding.claim_id,
            claim_generation: request.claim_generation,
            claim_digest: request.claim_digest.clone(),
            binding_digest: request.binding.digest()?,
            slots: request
                .binding
                .slots
                .iter()
                .map(ResourceSlotBinding::evidence)
                .collect(),
            released_at,
        };
        released.validate_for(request)?;
        Ok(released)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node resource claim released schema {:?}",
                self.schema
            ));
        }
        validate_uuid("claim_id", self.claim_id)?;
        if self.claim_generation == 0 || self.released_at.timestamp_millis() <= 0 {
            return Err("released resource claim generation or time is invalid".into());
        }
        validate_sha256("resource claim digest", &self.claim_digest)?;
        validate_sha256("resource binding digest", &self.binding_digest)?;
        validate_slot_evidence(&self.slots)
    }

    pub fn validate_for(&self, request: &NodeResourceClaimRelease) -> Result<(), String> {
        self.validate()?;
        request.validate()?;
        let expected_slots = request
            .binding
            .slots
            .iter()
            .map(ResourceSlotBinding::evidence)
            .collect::<Vec<_>>();
        if self.claim_id != request.binding.claim_id
            || self.claim_generation != request.claim_generation
            || self.claim_digest != request.claim_digest
            || self.binding_digest != request.binding.digest()?
            || self.slots != expected_slots
        {
            return Err("released resource claim evidence does not match its exact request".into());
        }
        Ok(())
    }

    pub fn evidence_digest(&self) -> Result<String, String> {
        canonical_digest(self, self.validate())
    }
}

fn required_scalar(
    slots: &[ResourceSlotBinding],
    kind: ResourceKind,
    unit: ResourceUnit,
) -> Result<u64, String> {
    optional_scalar(slots, kind, unit)?
        .ok_or_else(|| format!("resource claim binding omitted required {}", kind.as_str()))
}

fn optional_scalar(
    slots: &[ResourceSlotBinding],
    kind: ResourceKind,
    unit: ResourceUnit,
) -> Result<Option<u64>, String> {
    let matching = slots
        .iter()
        .filter(|slot| slot.kind == kind)
        .collect::<Vec<_>>();
    match matching.as_slice() {
        [] => Ok(None),
        [slot] => match slot.allocation {
            ResourceAllocation::Scalar {
                amount,
                unit: actual,
            } if actual == unit => Ok(Some(amount)),
            _ => Err(format!(
                "resource claim {} allocation has the wrong scalar unit",
                kind.as_str()
            )),
        },
        _ => Err(format!(
            "resource claim binding contains multiple {} slots",
            kind.as_str()
        )),
    }
}

fn insert_exact_claim(
    claims: &mut BTreeMap<String, String>,
    key: &str,
    value: String,
) -> Result<(), String> {
    if claims.get(key).is_some_and(|existing| existing != &value) {
        return Err(format!("Runtime evidence reserved claim {key:?} conflicts"));
    }
    claims.insert(key.into(), value);
    Ok(())
}

fn canonical_digest<T: Serialize>(
    value: &T,
    validation: Result<(), String>,
) -> Result<String, String> {
    validation?;
    let encoded = serde_json::to_vec(value)
        .map_err(|error| format!("could not encode resource claim evidence: {error}"))?;
    Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
}
