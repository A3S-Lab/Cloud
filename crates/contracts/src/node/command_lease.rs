use super::{command::NodeCommandEnvelope, validate_uuid};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeCommandLeaseRequest {
    pub schema: String,
    pub node_id: Uuid,
    pub agent_instance_id: Uuid,
    pub after_sequence: u64,
    pub max_commands: u16,
    pub wait_ms: u64,
}

impl NodeCommandLeaseRequest {
    pub const SCHEMA: &'static str = "a3s.cloud.node-command-lease-request.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported command lease request schema {:?}",
                self.schema
            ));
        }
        validate_uuid("node_id", self.node_id)?;
        validate_uuid("agent_instance_id", self.agent_instance_id)?;
        if self.max_commands == 0 || self.max_commands > 64 || self.wait_ms > 60_000 {
            return Err("command lease bounds are invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeCommandLeaseResponse {
    pub schema: String,
    pub lease_id: Uuid,
    pub node_id: Uuid,
    pub agent_instance_id: Uuid,
    pub leased_until: DateTime<Utc>,
    pub commands: Vec<NodeCommandEnvelope>,
}

impl NodeCommandLeaseResponse {
    pub const SCHEMA: &'static str = "a3s.cloud.node-command-lease-response.v1";

    pub fn validate(&self, now: DateTime<Utc>) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported command lease response schema {:?}",
                self.schema
            ));
        }
        validate_uuid("lease_id", self.lease_id)?;
        validate_uuid("node_id", self.node_id)?;
        validate_uuid("agent_instance_id", self.agent_instance_id)?;
        if self.leased_until <= now || self.commands.len() > 64 {
            return Err("command lease expiry or batch size is invalid".into());
        }
        let mut previous = None;
        for command in &self.commands {
            command.validate()?;
            if command.lease_id != self.lease_id || command.node_id != self.node_id {
                return Err("leased command identity does not match its lease".into());
            }
            if previous.is_some_and(|sequence| command.sequence <= sequence) {
                return Err("leased commands are not ordered by sequence".into());
            }
            previous = Some(command.sequence);
        }
        Ok(())
    }
}
