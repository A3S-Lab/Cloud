use super::{validate_lower_sha256, validate_single_line, validate_uuid};
use a3s_code_core::{
    AgentProtocolChangeSetV1, AgentProtocolCommandV1, AgentProtocolEventPageV1,
    AgentProtocolRunIdentityV1, AgentProtocolRunStateV1, AGENT_PROTOCOL_MAX_EVENTS_PER_PAGE,
    AGENT_PROTOCOL_V1,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const NODE_CODE_AGENT_COMMAND_SCHEMA_V1: &str = "a3s.cloud.code-agent-command.v1";

/// Exact existing Workload and Runtime Service that carries one A3S Code
/// Harness. This adds Cloud placement identity around the Code-owned protocol;
/// it does not define another Agent runtime lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeCodeAgentRuntimeBindingV1 {
    pub schema: String,
    pub execution_id: Uuid,
    pub workload_id: Uuid,
    pub workload_revision_id: Uuid,
    pub deployment_id: Uuid,
    pub replica_id: Uuid,
    pub runtime_unit_id: String,
    pub runtime_generation: u64,
    pub runtime_spec_digest: String,
    pub service_port_name: String,
    pub code_run_identity: AgentProtocolRunIdentityV1,
}

impl NodeCodeAgentRuntimeBindingV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.code-agent-runtime-binding.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Code Agent Runtime binding schema {:?}",
                self.schema
            ));
        }
        validate_uuid("execution_id", self.execution_id)?;
        validate_uuid("workload_id", self.workload_id)?;
        validate_uuid("workload_revision_id", self.workload_revision_id)?;
        validate_uuid("deployment_id", self.deployment_id)?;
        validate_uuid("replica_id", self.replica_id)?;
        validate_single_line("Runtime unit ID", &self.runtime_unit_id, 512)?;
        if self.runtime_generation == 0 {
            return Err("Code Agent Runtime generation must be positive".into());
        }
        validate_lower_sha256("Runtime spec digest", &self.runtime_spec_digest)?;
        validate_single_line(
            "Code Harness service port name",
            &self.service_port_name,
            128,
        )?;
        self.code_run_identity
            .validate()
            .map_err(|error| format!("invalid A3S Code run identity ({})", error.code()))
    }

    pub fn validate_command(&self, command: &AgentProtocolCommandV1) -> Result<(), String> {
        self.validate()?;
        command
            .validate()
            .map_err(|error| format!("invalid A3S Code command ({})", error.code()))?;
        if command.identity() != &self.code_run_identity {
            return Err("A3S Code command does not match its Runtime binding".into());
        }
        Ok(())
    }
}

/// One durable Node Agent delivery of an unmodified A3S Code event page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeCodeAgentEventBatchV1 {
    pub schema: String,
    pub batch_id: Uuid,
    pub node_id: Uuid,
    pub binding: NodeCodeAgentRuntimeBindingV1,
    pub page: AgentProtocolEventPageV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_set: Option<AgentProtocolChangeSetV1>,
    pub sent_at_ms: u64,
}

impl NodeCodeAgentEventBatchV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.code-agent-event-batch.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Code Agent event batch schema {:?}",
                self.schema
            ));
        }
        validate_uuid("batch_id", self.batch_id)?;
        validate_uuid("node_id", self.node_id)?;
        self.binding.validate()?;
        self.page
            .validate()
            .map_err(|error| format!("invalid A3S Code event page ({})", error.code()))?;
        if self.page.identity.protocol != AGENT_PROTOCOL_V1
            || self.page.identity != self.binding.code_run_identity
            || self.sent_at_ms < self.page.observed_at_ms
        {
            return Err("A3S Code event page does not match its delivery binding".into());
        }
        if let Some(change_set) = &self.change_set {
            change_set
                .validate()
                .map_err(|error| format!("invalid A3S Code change set ({})", error.code()))?;
            if self.page.retention_gap
                || !self.page.state.is_terminal()
                || self.page.has_more
                || change_set.identity != self.binding.code_run_identity
                || change_set.state != self.page.state
                || self.sent_at_ms < change_set.observed_at_ms
            {
                return Err(
                    "A3S Code change set does not match its terminal delivery binding".into(),
                );
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        let encoded = serde_json::to_vec(self)
            .map_err(|error| format!("could not encode Code Agent event batch: {error}"))?;
        Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
    }
}

/// Exact receipt that advances the existing Node Agent outbound-batch cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeCodeAgentEventReceiptV1 {
    pub schema: String,
    pub batch_id: Uuid,
    pub node_id: Uuid,
    pub execution_id: Uuid,
    pub identity: AgentProtocolRunIdentityV1,
    pub page_digest: String,
    pub accepted_after_event_sequence: Option<u64>,
    pub accepted_state: AgentProtocolRunStateV1,
    pub accepted_events: u16,
    pub accepted_at_ms: u64,
    pub replayed: bool,
}

impl NodeCodeAgentEventReceiptV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.code-agent-event-receipt.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Code Agent event receipt schema {:?}",
                self.schema
            ));
        }
        validate_uuid("batch_id", self.batch_id)?;
        validate_uuid("node_id", self.node_id)?;
        validate_uuid("execution_id", self.execution_id)?;
        self.identity
            .validate()
            .map_err(|error| format!("invalid A3S Code run identity ({})", error.code()))?;
        validate_lower_sha256("Code Agent event page digest", &self.page_digest)?;
        if usize::from(self.accepted_events) > AGENT_PROTOCOL_MAX_EVENTS_PER_PAGE
            || self.accepted_at_ms == 0
        {
            return Err("Code Agent event receipt bounds are invalid".into());
        }
        Ok(())
    }

    pub fn validate_for(&self, batch: &NodeCodeAgentEventBatchV1) -> Result<(), String> {
        batch.validate()?;
        self.validate()?;
        let accepted_events = u16::try_from(batch.page.events.len())
            .map_err(|_| "Code Agent event count exceeds receipt bounds".to_string())?;
        if self.batch_id != batch.batch_id
            || self.node_id != batch.node_id
            || self.execution_id != batch.binding.execution_id
            || self.identity != batch.page.identity
            || self.page_digest != batch.page.digest().map_err(|error| error.to_string())?
            || self.accepted_after_event_sequence != batch.page.next_after_event_sequence
            || self.accepted_state != batch.page.state
            || self.accepted_events != accepted_events
        {
            return Err("Code Agent event receipt changed its pending batch identity".into());
        }
        Ok(())
    }
}
