use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_STABLE_RESOURCE_ID_LENGTH: usize = 255;
const MAX_RESOURCE_SLOTS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Cpu,
    Memory,
    EphemeralStorage,
    HostPort,
    Accelerator,
    Volume,
}

impl ResourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Memory => "memory",
            Self::EphemeralStorage => "ephemeral_storage",
            Self::HostPort => "host_port",
            Self::Accelerator => "accelerator",
            Self::Volume => "volume",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "cpu" => Ok(Self::Cpu),
            "memory" => Ok(Self::Memory),
            "ephemeral_storage" => Ok(Self::EphemeralStorage),
            "host_port" => Ok(Self::HostPort),
            "accelerator" => Ok(Self::Accelerator),
            "volume" => Ok(Self::Volume),
            _ => Err(format!("unsupported hard resource kind {value:?}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceUnit {
    MilliCpu,
    Byte,
    Port,
    Count,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceAllocation {
    Scalar {
        amount: u64,
        unit: ResourceUnit,
    },
    Range {
        start: u64,
        end_inclusive: u64,
        unit: ResourceUnit,
    },
}

impl ResourceAllocation {
    pub fn validate_for(&self, kind: ResourceKind) -> Result<(), String> {
        match (kind, self) {
            (
                ResourceKind::Cpu,
                Self::Scalar {
                    amount,
                    unit: ResourceUnit::MilliCpu,
                },
            )
            | (
                ResourceKind::Memory | ResourceKind::EphemeralStorage,
                Self::Scalar {
                    amount,
                    unit: ResourceUnit::Byte,
                },
            )
            | (
                ResourceKind::Accelerator | ResourceKind::Volume,
                Self::Scalar {
                    amount,
                    unit: ResourceUnit::Count,
                },
            ) if *amount > 0 => Ok(()),
            (
                ResourceKind::HostPort,
                Self::Range {
                    start,
                    end_inclusive,
                    unit: ResourceUnit::Port,
                },
            ) if *start > 0 && start <= end_inclusive && *end_inclusive <= u16::MAX.into() => {
                Ok(())
            }
            _ => Err("hard resource allocation does not match its resource kind".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceSlotRequest {
    pub kind: ResourceKind,
    pub stable_resource_id: String,
    pub allocation: ResourceAllocation,
}

impl ResourceSlotRequest {
    pub fn new(
        kind: ResourceKind,
        stable_resource_id: impl Into<String>,
        allocation: ResourceAllocation,
    ) -> Result<Self, String> {
        let request = Self {
            kind,
            stable_resource_id: stable_resource_id.into(),
            allocation,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_stable_resource_id(&self.stable_resource_id)?;
        self.allocation.validate_for(self.kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceSlotBinding {
    pub kind: ResourceKind,
    pub stable_resource_id: String,
    pub allocation: ResourceAllocation,
    pub slot_generation: u64,
    pub fence_token: Uuid,
}

impl ResourceSlotBinding {
    pub fn validate(&self) -> Result<(), String> {
        validate_stable_resource_id(&self.stable_resource_id)?;
        self.allocation.validate_for(self.kind)?;
        if self.slot_generation == 0 || self.fence_token.is_nil() {
            return Err("hard resource slot fencing identity is invalid".into());
        }
        Ok(())
    }

    pub fn evidence(&self) -> ResourceSlotEvidence {
        ResourceSlotEvidence {
            kind: self.kind,
            stable_resource_id: self.stable_resource_id.clone(),
            slot_generation: self.slot_generation,
            fence_token: self.fence_token,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceSlotEvidence {
    pub kind: ResourceKind,
    pub stable_resource_id: String,
    pub slot_generation: u64,
    pub fence_token: Uuid,
}

impl ResourceSlotEvidence {
    pub fn validate(&self) -> Result<(), String> {
        validate_stable_resource_id(&self.stable_resource_id)?;
        if self.slot_generation == 0 || self.fence_token.is_nil() {
            return Err("hard resource slot evidence is invalid".into());
        }
        Ok(())
    }
}

pub fn validate_slot_requests(slots: &[ResourceSlotRequest]) -> Result<(), String> {
    if slots.is_empty() || slots.len() > MAX_RESOURCE_SLOTS {
        return Err("hard resource claim must contain a bounded non-empty slot set".into());
    }
    let mut previous = None;
    for slot in slots {
        slot.validate()?;
        let key = (slot.kind, slot.stable_resource_id.as_str());
        if previous.is_some_and(|candidate| candidate >= key) {
            return Err(
                "hard resource slot requests must be uniquely and canonically sorted".into(),
            );
        }
        previous = Some(key);
    }
    Ok(())
}

pub fn validate_slot_bindings(slots: &[ResourceSlotBinding]) -> Result<(), String> {
    if slots.is_empty() || slots.len() > MAX_RESOURCE_SLOTS {
        return Err("hard resource claim must contain a bounded non-empty slot set".into());
    }
    let mut previous = None;
    for slot in slots {
        slot.validate()?;
        let key = (slot.kind, slot.stable_resource_id.as_str());
        if previous.is_some_and(|candidate| candidate >= key) {
            return Err(
                "hard resource slot bindings must be uniquely and canonically sorted".into(),
            );
        }
        previous = Some(key);
    }
    Ok(())
}

pub fn validate_slot_evidence(slots: &[ResourceSlotEvidence]) -> Result<(), String> {
    if slots.is_empty() || slots.len() > MAX_RESOURCE_SLOTS {
        return Err("hard resource evidence must contain a bounded non-empty slot set".into());
    }
    let mut previous = None;
    for slot in slots {
        slot.validate()?;
        let key = (slot.kind, slot.stable_resource_id.as_str());
        if previous.is_some_and(|candidate| candidate >= key) {
            return Err(
                "hard resource slot evidence must be uniquely and canonically sorted".into(),
            );
        }
        previous = Some(key);
    }
    Ok(())
}

pub(crate) fn validate_stable_resource_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_STABLE_RESOURCE_ID_LENGTH
        || value.contains(['\0', '\r', '\n', '\t'])
        || value.trim() != value
    {
        return Err("stable hard resource ID is invalid".into());
    }
    Ok(())
}
