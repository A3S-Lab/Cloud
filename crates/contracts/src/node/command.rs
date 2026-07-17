use a3s_runtime::contract::{
    RuntimeActionRequest, RuntimeApplyRequest, RuntimeInspection, RuntimeObservation,
    RuntimeRemoval,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
    validate_sha256, validate_single_line, validate_uuid, GatewaySnapshot, NodeGatewayAck,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NodeCommandPayload {
    RuntimeApply { request: Box<RuntimeApplyRequest> },
    RuntimeInspect { unit_id: String, generation: u64 },
    RuntimeStop { request: RuntimeActionRequest },
    RuntimeRemove { request: RuntimeActionRequest },
    GatewaySnapshotInstall { snapshot: Box<GatewaySnapshot> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeCommandMetadata {
    pub command_id: Uuid,
    pub lease_id: Uuid,
    pub node_id: Uuid,
    pub sequence: u64,
    pub aggregate_id: Uuid,
    pub issued_at: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub correlation_id: Uuid,
}

impl NodeCommandPayload {
    pub fn schema(&self) -> &'static str {
        match self {
            Self::RuntimeApply { .. } => RuntimeApplyRequest::SCHEMA,
            Self::RuntimeInspect { .. } => "a3s.runtime.inspect-request.v1",
            Self::RuntimeStop { .. } => "a3s.runtime.stop-request.v1",
            Self::RuntimeRemove { .. } => "a3s.runtime.remove-request.v1",
            Self::GatewaySnapshotInstall { .. } => GatewaySnapshot::SCHEMA,
        }
    }

    pub fn generation(&self) -> u64 {
        match self {
            Self::RuntimeApply { request } => request.spec.generation,
            Self::RuntimeInspect { generation, .. } => *generation,
            Self::RuntimeStop { request } | Self::RuntimeRemove { request } => request.generation,
            Self::GatewaySnapshotInstall { snapshot } => snapshot.revision,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::RuntimeApply { request } => request.validate(),
            Self::RuntimeInspect {
                unit_id,
                generation,
            } => {
                validate_single_line("Runtime unit ID", unit_id, 512)?;
                if *generation == 0 {
                    return Err("Runtime inspection generation must be positive".into());
                }
                Ok(())
            }
            Self::RuntimeStop { request } | Self::RuntimeRemove { request } => request.validate(),
            Self::GatewaySnapshotInstall { snapshot } => snapshot.validate(),
        }
    }

    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        let encoded = serde_json::to_vec(self)
            .map_err(|error| format!("could not encode node command payload: {error}"))?;
        Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeCommandEnvelope {
    pub schema: String,
    pub command_id: Uuid,
    pub lease_id: Uuid,
    pub node_id: Uuid,
    pub sequence: u64,
    pub aggregate_id: Uuid,
    pub generation: u64,
    pub payload_schema: String,
    pub payload_digest: String,
    pub issued_at: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub correlation_id: Uuid,
    pub payload: NodeCommandPayload,
}

impl NodeCommandEnvelope {
    pub const SCHEMA: &'static str = "a3s.cloud.node-command.v1";

    pub fn new(metadata: NodeCommandMetadata, payload: NodeCommandPayload) -> Result<Self, String> {
        let envelope = Self {
            schema: Self::SCHEMA.into(),
            command_id: metadata.command_id,
            lease_id: metadata.lease_id,
            node_id: metadata.node_id,
            sequence: metadata.sequence,
            aggregate_id: metadata.aggregate_id,
            generation: payload.generation(),
            payload_schema: payload.schema().into(),
            payload_digest: payload.digest()?,
            issued_at: metadata.issued_at,
            not_after: metadata.not_after,
            correlation_id: metadata.correlation_id,
            payload,
        };
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!("unsupported node command schema {:?}", self.schema));
        }
        validate_uuid("command_id", self.command_id)?;
        validate_uuid("lease_id", self.lease_id)?;
        validate_uuid("node_id", self.node_id)?;
        validate_uuid("aggregate_id", self.aggregate_id)?;
        validate_uuid("correlation_id", self.correlation_id)?;
        if self.sequence == 0 || self.generation == 0 {
            return Err("command sequence and generation must be positive".into());
        }
        if self.not_after <= self.issued_at {
            return Err("command expiry must follow issue time".into());
        }
        self.payload.validate()?;
        if self.generation != self.payload.generation() {
            return Err("command generation does not match its payload".into());
        }
        if self.payload_schema != self.payload.schema() {
            return Err("command payload schema does not match its payload".into());
        }
        validate_sha256("command payload digest", &self.payload_digest)?;
        if self.payload_digest != self.payload.digest()? {
            return Err("command payload digest does not match its payload".into());
        }
        Ok(())
    }

    pub fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
        now >= self.not_after
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NodeCommandResult {
    RuntimeApplied {
        observation: Box<RuntimeObservation>,
    },
    RuntimeInspected {
        inspection: RuntimeInspection,
    },
    RuntimeStopped {
        inspection: RuntimeInspection,
    },
    RuntimeRemoved {
        removal: RuntimeRemoval,
    },
    GatewaySnapshotInstalled {
        acknowledgement: NodeGatewayAck,
    },
}

impl NodeCommandResult {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::RuntimeApplied { observation } => observation.validate(),
            Self::RuntimeInspected { inspection } | Self::RuntimeStopped { inspection } => {
                inspection.validate()
            }
            Self::RuntimeRemoved { removal } => removal.validate(),
            Self::GatewaySnapshotInstalled { acknowledgement } => acknowledgement.validate(),
        }
    }

    fn validate_against(&self, command: &NodeCommandEnvelope) -> Result<(), String> {
        self.validate()?;
        match (&command.payload, self) {
            (
                NodeCommandPayload::RuntimeApply { request },
                Self::RuntimeApplied { observation },
            ) => observation.validate_against(&request.spec),
            (
                NodeCommandPayload::RuntimeInspect {
                    unit_id,
                    generation,
                },
                Self::RuntimeInspected { inspection },
            )
            | (
                NodeCommandPayload::RuntimeStop {
                    request:
                        RuntimeActionRequest {
                            unit_id,
                            generation,
                            ..
                        },
                },
                Self::RuntimeStopped { inspection },
            ) => validate_inspection_identity(inspection, unit_id, *generation),
            (NodeCommandPayload::RuntimeRemove { request }, Self::RuntimeRemoved { removal })
                if removal.request_id == request.request_id
                    && removal.unit_id == request.unit_id
                    && removal.generation == request.generation =>
            {
                Ok(())
            }
            (NodeCommandPayload::RuntimeRemove { .. }, Self::RuntimeRemoved { .. }) => {
                Err("node command result identity does not match its payload".into())
            }
            (
                NodeCommandPayload::GatewaySnapshotInstall { snapshot },
                Self::GatewaySnapshotInstalled { acknowledgement },
            ) => acknowledgement.validate_for(command.command_id, command.node_id, snapshot),
            _ => Err("node command result kind does not match its payload".into()),
        }
    }
}

fn validate_inspection_identity(
    inspection: &RuntimeInspection,
    expected_unit_id: &str,
    expected_generation: u64,
) -> Result<(), String> {
    match inspection {
        RuntimeInspection::Found { observation, .. }
            if observation.unit_id == expected_unit_id
                && observation.generation == expected_generation =>
        {
            Ok(())
        }
        RuntimeInspection::NotFound { unit_id, .. } if unit_id == expected_unit_id => Ok(()),
        _ => Err("Runtime inspection identity does not match its command".into()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeCommandFailure {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl NodeCommandFailure {
    fn validate(&self) -> Result<(), String> {
        validate_single_line("command failure code", &self.code, 127)?;
        validate_single_line("command failure message", &self.message, 16 * 1024)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum NodeCommandOutcome {
    Succeeded { result: Box<NodeCommandResult> },
    Rejected { failure: NodeCommandFailure },
    Failed { failure: NodeCommandFailure },
}

impl NodeCommandOutcome {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Succeeded { result } => result.validate(),
            Self::Rejected { failure } | Self::Failed { failure } => failure.validate(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeCommandAck {
    pub schema: String,
    pub command_id: Uuid,
    pub lease_id: Uuid,
    pub node_id: Uuid,
    pub sequence: u64,
    pub payload_digest: String,
    pub completed_at: DateTime<Utc>,
    pub outcome: NodeCommandOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeCommandAckReceipt {
    pub schema: String,
    pub command_id: Uuid,
    pub node_id: Uuid,
    pub replayed: bool,
}

impl NodeCommandAckReceipt {
    pub const SCHEMA: &'static str = "a3s.cloud.node-command-ack-receipt.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node command acknowledgement receipt schema {:?}",
                self.schema
            ));
        }
        validate_uuid("command_id", self.command_id)?;
        validate_uuid("node_id", self.node_id)
    }
}

impl NodeCommandAck {
    pub const SCHEMA: &'static str = "a3s.cloud.node-command-ack.v1";

    pub fn validate_against(&self, command: &NodeCommandEnvelope) -> Result<(), String> {
        command.validate()?;
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node command acknowledgement schema {:?}",
                self.schema
            ));
        }
        if self.command_id != command.command_id
            || self.lease_id != command.lease_id
            || self.node_id != command.node_id
            || self.sequence != command.sequence
            || self.payload_digest != command.payload_digest
        {
            return Err("command acknowledgement identity does not match the command".into());
        }
        if self.completed_at < command.issued_at {
            return Err("command acknowledgement predates the command".into());
        }
        self.outcome.validate()?;
        if let NodeCommandOutcome::Succeeded { result } = &self.outcome {
            result.validate_against(command)?;
            if let NodeCommandResult::GatewaySnapshotInstalled { acknowledgement } = result.as_ref()
            {
                if acknowledgement.acknowledged_at < command.issued_at
                    || acknowledgement.acknowledged_at > self.completed_at
                {
                    return Err(
                        "Gateway acknowledgement time falls outside command execution".into(),
                    );
                }
            }
        }
        Ok(())
    }
}

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
