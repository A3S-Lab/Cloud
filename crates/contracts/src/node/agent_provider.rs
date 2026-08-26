use super::{validate_lower_sha256, validate_single_line, validate_uuid};
use crate::{
    AgentProtocolCommandReceiptV1, AgentProtocolCommandV1, AgentProtocolRunCancelV1,
    AgentProtocolRunIdentityV1, AgentProtocolRunRecoverV1, AgentProtocolRunStartV1,
    AgentProtocolRunStateV1, AgentProviderCommandReceiptV1, AgentProviderCommandV1,
    AgentProviderEventPageV1, AgentProviderEventReceiptV1, AgentProviderProfile,
    AgentProviderRunIdentityV1, AgentProviderRunStateV1, AGENT_PROTOCOL_V1,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const NODE_AGENT_PROVIDER_COMMAND_SCHEMA_V1: &str = "a3s.cloud.node-agent-provider-command.v1";

/// Exact existing Workload and Runtime Service carrying one immutable Agent
/// provider profile and one provider-neutral logical run identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeAgentProviderRuntimeBindingV1 {
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
    pub provider_profile_acl: String,
    pub provider_profile_digest: String,
    pub provider_run_identity: AgentProviderRunIdentityV1,
}

impl NodeAgentProviderRuntimeBindingV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.node-agent-provider-runtime-binding.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Node Agent provider Runtime binding schema {:?}",
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
            return Err("Agent provider Runtime generation must be positive".into());
        }
        validate_lower_sha256("Runtime spec digest", &self.runtime_spec_digest)?;
        validate_single_line(
            "Agent provider service port name",
            &self.service_port_name,
            128,
        )?;
        let profile = self.profile()?;
        self.provider_run_identity.validate_for(&profile)
    }

    pub fn validate_command(&self, command: &AgentProviderCommandV1) -> Result<(), String> {
        self.validate()?;
        command.validate_for(&self.profile()?)?;
        if command.identity() != &self.provider_run_identity {
            return Err("Agent provider command does not match its Runtime binding".into());
        }
        Ok(())
    }

    pub fn profile(&self) -> Result<AgentProviderProfile, String> {
        AgentProviderProfile::restore(&self.provider_profile_acl, &self.provider_profile_digest)
    }

    pub fn code_binding(&self) -> Result<super::NodeCodeAgentRuntimeBindingV1, String> {
        let profile = self.profile()?;
        if profile.kind() != "a3s.code" || profile.native_protocol() != AGENT_PROTOCOL_V1 {
            return Err("Agent provider Runtime binding is not the native Code adapter".into());
        }
        Ok(super::NodeCodeAgentRuntimeBindingV1 {
            schema: super::NodeCodeAgentRuntimeBindingV1::SCHEMA.into(),
            execution_id: self.execution_id,
            workload_id: self.workload_id,
            workload_revision_id: self.workload_revision_id,
            deployment_id: self.deployment_id,
            replica_id: self.replica_id,
            runtime_unit_id: self.runtime_unit_id.clone(),
            runtime_generation: self.runtime_generation,
            runtime_spec_digest: self.runtime_spec_digest.clone(),
            service_port_name: self.service_port_name.clone(),
            code_run_identity: AgentProtocolRunIdentityV1 {
                schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
                protocol: AGENT_PROTOCOL_V1.into(),
                agent_release_identity: self.provider_run_identity.agent_release_identity.clone(),
                session_id: self.provider_run_identity.session_id.clone(),
                run_id: self.provider_run_identity.run_id.clone(),
            },
        })
    }

    pub fn code_command(
        &self,
        command: &AgentProviderCommandV1,
    ) -> Result<AgentProtocolCommandV1, String> {
        self.validate_command(command)?;
        let identity = self.code_binding()?.code_run_identity;
        let native = match command {
            AgentProviderCommandV1::Start { request } => AgentProtocolCommandV1::Start {
                request: AgentProtocolRunStartV1 {
                    schema: AgentProtocolRunStartV1::SCHEMA.into(),
                    request_id: request.request_id.clone(),
                    identity,
                    prompt: request.prompt.clone(),
                },
            },
            AgentProviderCommandV1::Cancel { request } => AgentProtocolCommandV1::Cancel {
                request: AgentProtocolRunCancelV1 {
                    schema: AgentProtocolRunCancelV1::SCHEMA.into(),
                    request_id: request.request_id.clone(),
                    identity,
                    reason: request.reason.clone(),
                },
            },
            AgentProviderCommandV1::Recover { request } => AgentProtocolCommandV1::Recover {
                request: AgentProtocolRunRecoverV1 {
                    schema: AgentProtocolRunRecoverV1::SCHEMA.into(),
                    request_id: request.request_id.clone(),
                    identity,
                    checkpoint_run_id: request.checkpoint_run_id.clone(),
                },
            },
        };
        native
            .validate()
            .map_err(|error| format!("invalid native A3S Code command ({})", error.code()))?;
        Ok(native)
    }

    pub fn code_receipt(
        &self,
        command: &AgentProviderCommandV1,
        receipt: &AgentProtocolCommandReceiptV1,
    ) -> Result<AgentProviderCommandReceiptV1, String> {
        let native = self.code_command(command)?;
        receipt
            .validate_for(&native)
            .map_err(|error| format!("invalid native A3S Code receipt ({})", error.code()))?;
        AgentProviderCommandReceiptV1::accepted(
            &self.profile()?,
            command,
            provider_state(receipt.state),
            receipt.observed_at_ms,
            receipt.replayed,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeAgentProviderEventBatchV1 {
    pub schema: String,
    pub batch_id: Uuid,
    pub node_id: Uuid,
    pub binding: NodeAgentProviderRuntimeBindingV1,
    pub page: AgentProviderEventPageV1,
    pub sent_at_ms: u64,
}

impl NodeAgentProviderEventBatchV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.node-agent-provider-event-batch.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Node Agent provider event-batch schema {:?}",
                self.schema
            ));
        }
        validate_uuid("batch_id", self.batch_id)?;
        validate_uuid("node_id", self.node_id)?;
        self.binding.validate()?;
        self.page.validate_for(&self.binding.profile()?)?;
        if self.page.identity != self.binding.provider_run_identity
            || self.sent_at_ms < self.page.observed_at_ms
        {
            return Err("Agent provider event page does not match its Node binding".into());
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        let encoded = serde_json::to_vec(self)
            .map_err(|error| format!("could not encode Agent provider event batch: {error}"))?;
        Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeAgentProviderEventReceiptV1 {
    pub schema: String,
    pub batch_id: Uuid,
    pub node_id: Uuid,
    pub execution_id: Uuid,
    pub receipt: AgentProviderEventReceiptV1,
}

impl NodeAgentProviderEventReceiptV1 {
    pub const SCHEMA: &'static str = "a3s.cloud.node-agent-provider-event-receipt.v1";

    pub fn validate_for(&self, batch: &NodeAgentProviderEventBatchV1) -> Result<(), String> {
        batch.validate()?;
        if self.schema != Self::SCHEMA
            || self.batch_id != batch.batch_id
            || self.node_id != batch.node_id
            || self.execution_id != batch.binding.execution_id
        {
            return Err("Node Agent provider event receipt changed its batch identity".into());
        }
        self.receipt
            .validate_for(&batch.binding.profile()?, batch.batch_id, &batch.page)?;
        if self.receipt.accepted_at_ms < batch.sent_at_ms {
            return Err("Node Agent provider event receipt predates its delivery".into());
        }
        Ok(())
    }
}

fn provider_state(state: AgentProtocolRunStateV1) -> AgentProviderRunStateV1 {
    match state {
        AgentProtocolRunStateV1::Created => AgentProviderRunStateV1::Created,
        AgentProtocolRunStateV1::Planning => AgentProviderRunStateV1::Planning,
        AgentProtocolRunStateV1::Executing => AgentProviderRunStateV1::Executing,
        AgentProtocolRunStateV1::Verifying => AgentProviderRunStateV1::Verifying,
        AgentProtocolRunStateV1::Completed => AgentProviderRunStateV1::Completed,
        AgentProtocolRunStateV1::Failed => AgentProviderRunStateV1::Failed,
        AgentProtocolRunStateV1::Cancelled => AgentProviderRunStateV1::Cancelled,
    }
}
