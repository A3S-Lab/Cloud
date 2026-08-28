use super::{validate_lower_sha256, validate_single_line, validate_uuid};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const MAX_CONTRACTS_PER_DIRECTION: usize = 128;
const MAX_CONTRACT_ID_BYTES: usize = 255;
const MAX_SAFE_PROTOCOL_GENERATION: u64 = (1_u64 << 53) - 1;
const MAX_HELLO_AGE_SECONDS: i64 = 300;
const MAX_SELECTION_CLOCK_SKEW_SECONDS: i64 = 30;

/// Exact node-protocol schemas the Agent can read from and write to Cloud.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeProtocolContractSet {
    pub agent_readable: Vec<String>,
    pub agent_writable: Vec<String>,
}

impl NodeProtocolContractSet {
    pub fn validate(&self) -> Result<(), String> {
        validate_contracts("Agent-readable", &self.agent_readable)?;
        validate_contracts("Agent-writable", &self.agent_writable)
    }

    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        let encoded = serde_json::to_vec(self)
            .map_err(|error| format!("could not encode node protocol contracts: {error}"))?;
        Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
    }

    pub fn is_subset_of(&self, offered: &Self) -> bool {
        self.agent_readable
            .iter()
            .all(|schema| offered.agent_readable.contains(schema))
            && self
                .agent_writable
                .iter()
                .all(|schema| offered.agent_writable.contains(schema))
    }
}

/// Durable reference used to chain reconnect negotiation without downgrading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeSessionSelectionReference {
    pub session_id: Uuid,
    pub generation: u64,
    pub contracts_digest: String,
}

impl NodeSessionSelectionReference {
    pub fn validate(&self) -> Result<(), String> {
        validate_uuid("node session ID", self.session_id)?;
        if self.generation == 0 || self.generation > MAX_SAFE_PROTOCOL_GENERATION {
            return Err("node session selection generation is invalid".into());
        }
        validate_lower_sha256("node session contract-set digest", &self.contracts_digest)
    }
}

/// Agent offer sent on every authenticated start and reconnect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeSessionHello {
    pub schema: String,
    pub node_id: Uuid,
    pub agent_instance_id: Uuid,
    pub session_epoch: Uuid,
    pub hello_sequence: u64,
    pub offered_at: DateTime<Utc>,
    pub agent_version: String,
    pub contracts: NodeProtocolContractSet,
    pub previous_selection: Option<NodeSessionSelectionReference>,
}

impl NodeSessionHello {
    pub const SCHEMA: &'static str = "a3s.cloud.node-session-hello.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node session hello schema {:?}",
                self.schema
            ));
        }
        validate_uuid("node session node ID", self.node_id)?;
        validate_uuid("node session Agent instance ID", self.agent_instance_id)?;
        validate_uuid("node session epoch", self.session_epoch)?;
        if self.hello_sequence == 0 || self.hello_sequence > MAX_SAFE_PROTOCOL_GENERATION {
            return Err("node session hello sequence is invalid".into());
        }
        validate_single_line("node session Agent version", &self.agent_version, 255)?;
        self.contracts.validate()?;
        if let Some(previous) = &self.previous_selection {
            previous.validate()?;
        }
        Ok(())
    }

    pub fn validate_at(&self, received_at: DateTime<Utc>) -> Result<(), String> {
        self.validate()?;
        if self.offered_at > received_at + Duration::seconds(MAX_SELECTION_CLOCK_SKEW_SECONDS)
            || self.offered_at < received_at - Duration::seconds(MAX_HELLO_AGE_SECONDS)
        {
            return Err("node session hello freshness is invalid".into());
        }
        Ok(())
    }
}

/// Cloud-selected contract set for one exact authenticated Agent session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeSessionSelection {
    pub schema: String,
    pub node_id: Uuid,
    pub agent_instance_id: Uuid,
    pub session_epoch: Uuid,
    pub hello_sequence: u64,
    pub session_id: Uuid,
    pub generation: u64,
    pub selected_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub contracts: NodeProtocolContractSet,
    pub previous_selection: Option<NodeSessionSelectionReference>,
}

impl NodeSessionSelection {
    pub const SCHEMA: &'static str = "a3s.cloud.node-session-selection.v1";
    pub const MAX_LIFETIME_HOURS: i64 = 24;

    pub fn validate_for(&self, hello: &NodeSessionHello, now: DateTime<Utc>) -> Result<(), String> {
        hello.validate()?;
        self.validate_structure()?;
        let expected_generation = hello
            .previous_selection
            .as_ref()
            .map_or(Some(1), |previous| previous.generation.checked_add(1))
            .ok_or_else(|| "node session selection generation is exhausted".to_string())?;
        if self.node_id != hello.node_id
            || self.agent_instance_id != hello.agent_instance_id
            || self.session_epoch != hello.session_epoch
            || self.hello_sequence != hello.hello_sequence
            || self.generation != expected_generation
            || self.previous_selection != hello.previous_selection
        {
            return Err("node session selection changed its hello or downgrade chain".into());
        }
        if self.expires_at <= now
            || self.selected_at > now + Duration::seconds(MAX_SELECTION_CLOCK_SKEW_SECONDS)
        {
            return Err("node session selection freshness is invalid".into());
        }
        self.contracts.validate()?;
        if !self.contracts.is_subset_of(&hello.contracts) {
            return Err("node session selected a contract the Agent did not offer".into());
        }
        Ok(())
    }

    pub fn reference(&self) -> Result<NodeSessionSelectionReference, String> {
        self.validate_structure()?;
        let reference = NodeSessionSelectionReference {
            session_id: self.session_id,
            generation: self.generation,
            contracts_digest: self.contracts.digest()?,
        };
        reference.validate()?;
        Ok(reference)
    }

    fn validate_structure(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node session selection schema {:?}",
                self.schema
            ));
        }
        validate_uuid("selected node ID", self.node_id)?;
        validate_uuid("selected Agent instance ID", self.agent_instance_id)?;
        validate_uuid("selected node session epoch", self.session_epoch)?;
        validate_uuid("selected node session ID", self.session_id)?;
        if self.hello_sequence == 0
            || self.hello_sequence > MAX_SAFE_PROTOCOL_GENERATION
            || self.generation == 0
            || self.generation > MAX_SAFE_PROTOCOL_GENERATION
        {
            return Err("node session selection sequence or generation is invalid".into());
        }
        let lifetime = self.expires_at - self.selected_at;
        if lifetime <= Duration::zero() || lifetime > Duration::hours(Self::MAX_LIFETIME_HOURS) {
            return Err("node session selection lifetime is invalid".into());
        }
        self.contracts.validate()?;
        if let Some(previous) = &self.previous_selection {
            previous.validate()?;
        }
        Ok(())
    }
}

fn validate_contracts(label: &str, contracts: &[String]) -> Result<(), String> {
    if contracts.is_empty()
        || contracts.len() > MAX_CONTRACTS_PER_DIRECTION
        || !contracts.windows(2).all(|pair| pair[0] < pair[1])
        || contracts.iter().any(|schema| !valid_contract_id(schema))
    {
        return Err(format!(
            "{label} node contracts must be a sorted unique bounded schema list"
        ));
    }
    Ok(())
}

fn valid_contract_id(value: &str) -> bool {
    value.len() <= MAX_CONTRACT_ID_BYTES
        && value.starts_with("a3s.")
        && value.ends_with(|character: char| character.is_ascii_digit())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
        })
}
