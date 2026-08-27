use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AgentExecutionId, DeploymentId, NodeId, Sha256Digest, WorkloadId,
    WorkloadReplicaId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{
    AgentProtocolEventPageV1, AgentProtocolRunIdentityV1, AgentProtocolRunStateV1,
    AgentProviderEventPageV1, AgentProviderRunIdentityV1, AgentProviderRunStateV1,
    AgentProviderSemanticEventV1, HarnessInvocationProfileV1, NodeAgentProviderRuntimeBindingV1,
    NodeCodeAgentRuntimeBindingV1, AGENT_PROTOCOL_V1,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCodeRunBinding {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider: Option<super::AgentProviderProfileBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    invocation_profile: Option<HarnessInvocationProfileV1>,
    node_id: NodeId,
    workload_id: WorkloadId,
    workload_revision_id: WorkloadRevisionId,
    deployment_id: DeploymentId,
    replica_id: WorkloadReplicaId,
    runtime_unit_id: String,
    runtime_generation: u64,
    runtime_spec_digest: Sha256Digest,
    service_port_name: String,
    identity: AgentProtocolRunIdentityV1,
    accepted_after_event_sequence: Option<u64>,
    observed_state: AgentProviderRunStateV1,
    bound_at: DateTime<Utc>,
    observed_at: Option<DateTime<Utc>>,
}

impl AgentCodeRunBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        node_id: NodeId,
        workload_id: WorkloadId,
        workload_revision_id: WorkloadRevisionId,
        deployment_id: DeploymentId,
        replica_id: WorkloadReplicaId,
        runtime_unit_id: impl Into<String>,
        runtime_generation: u64,
        runtime_spec_digest: Sha256Digest,
        service_port_name: impl Into<String>,
        identity: AgentProtocolRunIdentityV1,
        bound_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::new_with_provider(
            super::AgentProviderProfileBinding::native_code()?,
            node_id,
            workload_id,
            workload_revision_id,
            deployment_id,
            replica_id,
            runtime_unit_id,
            runtime_generation,
            runtime_spec_digest,
            service_port_name,
            identity,
            bound_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_provider(
        provider: super::AgentProviderProfileBinding,
        node_id: NodeId,
        workload_id: WorkloadId,
        workload_revision_id: WorkloadRevisionId,
        deployment_id: DeploymentId,
        replica_id: WorkloadReplicaId,
        runtime_unit_id: impl Into<String>,
        runtime_generation: u64,
        runtime_spec_digest: Sha256Digest,
        service_port_name: impl Into<String>,
        identity: AgentProtocolRunIdentityV1,
        bound_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::restore_with_provider(
            provider,
            node_id,
            workload_id,
            workload_revision_id,
            deployment_id,
            replica_id,
            runtime_unit_id,
            runtime_generation,
            runtime_spec_digest,
            service_port_name,
            identity,
            None,
            AgentProviderRunStateV1::Created,
            bound_at,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        node_id: NodeId,
        workload_id: WorkloadId,
        workload_revision_id: WorkloadRevisionId,
        deployment_id: DeploymentId,
        replica_id: WorkloadReplicaId,
        runtime_unit_id: impl Into<String>,
        runtime_generation: u64,
        runtime_spec_digest: Sha256Digest,
        service_port_name: impl Into<String>,
        identity: AgentProtocolRunIdentityV1,
        accepted_after_event_sequence: Option<u64>,
        observed_state: AgentProtocolRunStateV1,
        bound_at: DateTime<Utc>,
        observed_at: Option<DateTime<Utc>>,
    ) -> Result<Self, String> {
        Self::restore_with_provider(
            super::AgentProviderProfileBinding::native_code()?,
            node_id,
            workload_id,
            workload_revision_id,
            deployment_id,
            replica_id,
            runtime_unit_id,
            runtime_generation,
            runtime_spec_digest,
            service_port_name,
            identity,
            accepted_after_event_sequence,
            provider_state(observed_state),
            bound_at,
            observed_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore_with_provider(
        provider: super::AgentProviderProfileBinding,
        node_id: NodeId,
        workload_id: WorkloadId,
        workload_revision_id: WorkloadRevisionId,
        deployment_id: DeploymentId,
        replica_id: WorkloadReplicaId,
        runtime_unit_id: impl Into<String>,
        runtime_generation: u64,
        runtime_spec_digest: Sha256Digest,
        service_port_name: impl Into<String>,
        identity: AgentProtocolRunIdentityV1,
        accepted_after_event_sequence: Option<u64>,
        observed_state: AgentProviderRunStateV1,
        bound_at: DateTime<Utc>,
        observed_at: Option<DateTime<Utc>>,
    ) -> Result<Self, String> {
        let binding = Self {
            provider: Some(provider),
            invocation_profile: None,
            node_id,
            workload_id,
            workload_revision_id,
            deployment_id,
            replica_id,
            runtime_unit_id: runtime_unit_id.into(),
            runtime_generation,
            runtime_spec_digest,
            service_port_name: service_port_name.into(),
            identity,
            accepted_after_event_sequence,
            observed_state,
            bound_at: canonical_timestamp(bound_at),
            observed_at: observed_at.map(canonical_timestamp),
        };
        binding.validate()?;
        Ok(binding)
    }

    pub(crate) fn with_invocation_profile(
        mut self,
        invocation_profile: HarnessInvocationProfileV1,
    ) -> Result<Self, String> {
        if !self.is_initial() || self.invocation_profile.is_some() {
            return Err(
                "Agent run Harness invocation profile must be bound before observation".into(),
            );
        }
        self.invocation_profile = Some(invocation_profile);
        self.validate()?;
        Ok(self)
    }

    pub(crate) fn restore_invocation_profile(
        mut self,
        invocation_profile: HarnessInvocationProfileV1,
    ) -> Result<Self, String> {
        if self.invocation_profile.is_some() {
            return Err("Agent run Harness invocation profile is immutable".into());
        }
        self.invocation_profile = Some(invocation_profile);
        self.validate()?;
        Ok(self)
    }

    pub fn restore_legacy_provider(&mut self) -> Result<bool, String> {
        if self.provider.is_some() {
            return Ok(false);
        }
        self.provider = Some(super::AgentProviderProfileBinding::native_code()?);
        self.validate()?;
        Ok(true)
    }

    pub fn provider(&self) -> Result<&super::AgentProviderProfileBinding, String> {
        self.provider
            .as_ref()
            .ok_or_else(|| "Agent Code run binding has no immutable provider profile".into())
    }

    pub fn provider_identity(&self) -> Result<AgentProviderRunIdentityV1, String> {
        let provider = self.provider()?;
        let mut identity = AgentProviderRunIdentityV1::new(
            provider.profile_digest().into(),
            provider.capability_digest().into(),
            self.identity.agent_release_identity.clone(),
            self.identity.session_id.clone(),
            self.identity.run_id.clone(),
        )?;
        identity.invocation_profile_digest = self
            .invocation_profile
            .as_ref()
            .map(HarnessInvocationProfileV1::digest)
            .transpose()?;
        identity.validate()?;
        Ok(identity)
    }

    pub const fn invocation_profile(&self) -> Option<&HarnessInvocationProfileV1> {
        self.invocation_profile.as_ref()
    }

    pub fn require_invocation_profile(&self) -> Result<&HarnessInvocationProfileV1, String> {
        self.invocation_profile
            .as_ref()
            .ok_or_else(|| "Agent run has no immutable Harness invocation profile".into())
    }

    pub const fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub const fn workload_id(&self) -> WorkloadId {
        self.workload_id
    }

    pub const fn workload_revision_id(&self) -> WorkloadRevisionId {
        self.workload_revision_id
    }

    pub const fn deployment_id(&self) -> DeploymentId {
        self.deployment_id
    }

    pub const fn replica_id(&self) -> WorkloadReplicaId {
        self.replica_id
    }

    pub fn runtime_unit_id(&self) -> &str {
        &self.runtime_unit_id
    }

    pub const fn runtime_generation(&self) -> u64 {
        self.runtime_generation
    }

    pub const fn runtime_spec_digest(&self) -> &Sha256Digest {
        &self.runtime_spec_digest
    }

    pub fn service_port_name(&self) -> &str {
        &self.service_port_name
    }

    pub const fn identity(&self) -> &AgentProtocolRunIdentityV1 {
        &self.identity
    }

    pub const fn accepted_after_event_sequence(&self) -> Option<u64> {
        self.accepted_after_event_sequence
    }

    pub const fn observed_state(&self) -> AgentProviderRunStateV1 {
        self.observed_state
    }

    pub const fn bound_at(&self) -> DateTime<Utc> {
        self.bound_at
    }

    pub const fn observed_at(&self) -> Option<DateTime<Utc>> {
        self.observed_at
    }

    pub fn is_initial(&self) -> bool {
        self.accepted_after_event_sequence.is_none()
            && self.observed_state == AgentProviderRunStateV1::Created
            && self.observed_at.is_none()
    }

    pub fn has_same_run_binding(&self, other: &Self) -> bool {
        self.has_same_runtime_binding(other)
            && self.identity == other.identity
            && self.bound_at == other.bound_at
    }

    pub fn has_same_runtime_binding(&self, other: &Self) -> bool {
        self.provider == other.provider
            && self.invocation_profile == other.invocation_profile
            && self.node_id == other.node_id
            && self.workload_id == other.workload_id
            && self.workload_revision_id == other.workload_revision_id
            && self.deployment_id == other.deployment_id
            && self.replica_id == other.replica_id
            && self.runtime_unit_id == other.runtime_unit_id
            && self.runtime_generation == other.runtime_generation
            && self.runtime_spec_digest == other.runtime_spec_digest
            && self.service_port_name == other.service_port_name
    }

    pub fn recovery_run_id(execution_id: AgentExecutionId, checkpoint_run_id: &str) -> String {
        let recovery_id = uuid::Uuid::new_v5(
            &execution_id.as_uuid(),
            format!("a3s-code-recovery-v1:{checkpoint_run_id}").as_bytes(),
        );
        format!("agent-recovery-{recovery_id}")
    }

    pub fn recovery_successor(
        &self,
        execution_id: AgentExecutionId,
        recovered_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let mut identity = self.identity.clone();
        identity.run_id = Self::recovery_run_id(execution_id, &self.identity.run_id);
        let successor = Self::new_with_provider(
            self.provider()?.clone(),
            self.node_id,
            self.workload_id,
            self.workload_revision_id,
            self.deployment_id,
            self.replica_id,
            self.runtime_unit_id.clone(),
            self.runtime_generation,
            self.runtime_spec_digest.clone(),
            self.service_port_name.clone(),
            identity,
            recovered_at,
        )?;
        match &self.invocation_profile {
            Some(profile) => successor.with_invocation_profile(profile.clone()),
            None => Ok(successor),
        }
    }

    pub fn is_recovery_successor_of(
        &self,
        previous: &Self,
        execution_id: AgentExecutionId,
    ) -> bool {
        self.has_same_runtime_binding(previous)
            && self.identity.schema == previous.identity.schema
            && self.identity.protocol == previous.identity.protocol
            && self.identity.agent_release_identity == previous.identity.agent_release_identity
            && self.identity.session_id == previous.identity.session_id
            && self.identity.run_id
                == Self::recovery_run_id(execution_id, &previous.identity.run_id)
            && self.bound_at >= previous.bound_at
    }

    pub fn can_settle_recovery_predecessor_runtime_binding(
        &self,
        previous: &NodeCodeAgentRuntimeBindingV1,
        execution_id: AgentExecutionId,
    ) -> bool {
        let mut expected = previous.clone();
        expected.code_run_identity.run_id =
            Self::recovery_run_id(execution_id, &previous.code_run_identity.run_id);
        let current = self.node_runtime_binding(execution_id.as_uuid());
        if current == expected {
            return true;
        }

        let mut same_lineage = previous.clone();
        same_lineage.code_run_identity.run_id = self.identity.run_id.clone();
        current == same_lineage
            && is_recovery_run_id(&self.identity.run_id)
            && (previous.code_run_identity.run_id == format!("agent-execution-{execution_id}")
                || is_recovery_run_id(&previous.code_run_identity.run_id))
    }

    pub fn can_settle_recovery_predecessor_provider_runtime_binding(
        &self,
        previous: &NodeAgentProviderRuntimeBindingV1,
        execution_id: AgentExecutionId,
    ) -> Result<bool, String> {
        previous.validate()?;
        let mut expected = previous.clone();
        expected.provider_run_identity.run_id =
            Self::recovery_run_id(execution_id, &previous.provider_run_identity.run_id);
        let current = self.node_provider_runtime_binding(execution_id.as_uuid())?;
        if current == expected {
            return Ok(true);
        }

        let mut same_lineage = previous.clone();
        same_lineage.provider_run_identity.run_id = self.provider_identity()?.run_id;
        Ok(current == same_lineage
            && is_recovery_run_id(&self.identity.run_id)
            && (previous.provider_run_identity.run_id == format!("agent-execution-{execution_id}")
                || is_recovery_run_id(&previous.provider_run_identity.run_id)))
    }

    pub fn validate_recovery_page(&self, page: &AgentProtocolEventPageV1) -> Result<(), String> {
        page.validate()
            .map_err(|error| format!("invalid A3S Code event page ({})", error.code()))?;
        let page_observed_at = page_observed_at(page)?;
        if page.identity != self.identity
            || page.after_event_sequence != self.accepted_after_event_sequence
            || !page.retention_gap
            || self
                .observed_at
                .is_some_and(|observed_at| page_observed_at < observed_at)
        {
            return Err("A3S Code recovery page does not continue its exact bound run".into());
        }
        Ok(())
    }

    pub fn validate_provider_recovery_page(
        &self,
        page: &AgentProviderEventPageV1,
    ) -> Result<(), String> {
        page.validate_for(&self.provider()?.profile()?)?;
        let page_observed_at = provider_page_observed_at(page)?;
        if page.identity != self.provider_identity()?
            || page.after_event_sequence != self.accepted_after_event_sequence
            || !page.retention_gap
            || self
                .observed_at
                .is_some_and(|observed_at| page_observed_at < observed_at)
        {
            return Err(
                "Agent provider recovery page does not continue its exact bound run".into(),
            );
        }
        Ok(())
    }

    pub fn node_runtime_binding(&self, execution_id: uuid::Uuid) -> NodeCodeAgentRuntimeBindingV1 {
        NodeCodeAgentRuntimeBindingV1 {
            schema: NodeCodeAgentRuntimeBindingV1::SCHEMA.into(),
            execution_id,
            workload_id: self.workload_id.as_uuid(),
            workload_revision_id: self.workload_revision_id.as_uuid(),
            deployment_id: self.deployment_id.as_uuid(),
            replica_id: self.replica_id.as_uuid(),
            runtime_unit_id: self.runtime_unit_id.clone(),
            runtime_generation: self.runtime_generation,
            runtime_spec_digest: self.runtime_spec_digest.as_str().into(),
            service_port_name: self.service_port_name.clone(),
            code_run_identity: self.identity.clone(),
        }
    }

    pub fn node_provider_runtime_binding(
        &self,
        execution_id: uuid::Uuid,
    ) -> Result<NodeAgentProviderRuntimeBindingV1, String> {
        let provider = self.provider()?;
        Ok(NodeAgentProviderRuntimeBindingV1 {
            schema: NodeAgentProviderRuntimeBindingV1::SCHEMA.into(),
            execution_id,
            workload_id: self.workload_id.as_uuid(),
            workload_revision_id: self.workload_revision_id.as_uuid(),
            deployment_id: self.deployment_id.as_uuid(),
            replica_id: self.replica_id.as_uuid(),
            runtime_unit_id: self.runtime_unit_id.clone(),
            runtime_generation: self.runtime_generation,
            runtime_spec_digest: self.runtime_spec_digest.as_str().into(),
            service_port_name: self.service_port_name.clone(),
            provider_profile_acl: provider.profile_acl().into(),
            provider_profile_digest: provider.profile_digest().into(),
            provider_run_identity: self.provider_identity()?,
        })
    }

    pub fn accept_event_page(&mut self, page: &AgentProtocolEventPageV1) -> Result<(), String> {
        page.validate()
            .map_err(|error| format!("invalid A3S Code event page ({})", error.code()))?;
        let page_observed_at = page_observed_at(page)?;
        if page.identity != self.identity
            || page.after_event_sequence != self.accepted_after_event_sequence
            || page.retention_gap
            || self
                .observed_at
                .is_some_and(|observed_at| page_observed_at < observed_at)
            || self.observed_state.is_terminal()
                && provider_state(page.state) != self.observed_state
        {
            return Err("A3S Code event page does not continue its exact bound run".into());
        }
        self.accepted_after_event_sequence = page.next_after_event_sequence;
        self.observed_state = provider_state(page.state);
        self.observed_at = Some(canonical_timestamp(page_observed_at));
        self.validate()
    }

    pub fn accept_provider_event_page(
        &mut self,
        page: &AgentProviderEventPageV1,
    ) -> Result<(), String> {
        page.validate_for(&self.provider()?.profile()?)?;
        self.validate_provider_tool_events(page)?;
        let page_observed_at = provider_page_observed_at(page)?;
        if page.identity != self.provider_identity()?
            || page.after_event_sequence != self.accepted_after_event_sequence
            || page.retention_gap
            || self
                .observed_at
                .is_some_and(|observed_at| page_observed_at < observed_at)
            || self.observed_state.is_terminal() && self.observed_state != page.state
        {
            return Err("Agent provider event page does not continue its exact bound run".into());
        }
        self.accepted_after_event_sequence = page.next_after_event_sequence;
        self.observed_state = page.state;
        self.observed_at = Some(canonical_timestamp(page_observed_at));
        self.validate()
    }

    fn validate_provider_tool_events(&self, page: &AgentProviderEventPageV1) -> Result<(), String> {
        let tool_events = page.events.iter().filter_map(|record| match &record.event {
            AgentProviderSemanticEventV1::ToolRequest { tool, .. }
            | AgentProviderSemanticEventV1::ToolResult { tool, .. } => Some(tool),
            AgentProviderSemanticEventV1::ModelOutput { .. } => None,
        });
        let mut invocation = None;
        for tool in tool_events {
            let profile = match invocation {
                Some(profile) => profile,
                None => {
                    let profile = self.require_invocation_profile()?;
                    invocation = Some(profile);
                    profile
                }
            };
            if !profile.tools.contains(tool) {
                return Err(
                    "Agent provider Tool event is outside its immutable invocation profile".into(),
                );
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        let provider = self.provider()?;
        provider.validate()?;
        if let Some(invocation) = &self.invocation_profile {
            invocation.validate_for(&provider.profile()?)?;
            if invocation.provider.kind != provider.kind()
                || invocation.workspace.workload_id != self.workload_id.as_uuid()
                || invocation.workspace.workload_revision_id != self.workload_revision_id.as_uuid()
                || invocation.workspace.runtime_unit_id != self.runtime_unit_id
                || invocation.workspace.runtime_generation != self.runtime_generation
                || invocation.workspace.runtime_spec_digest != self.runtime_spec_digest.as_str()
                || invocation.agent.artifact_digest != self.identity.agent_release_identity
            {
                return Err("Harness invocation profile changed its Agent Runtime binding".into());
            }
        }
        let native_code = provider.kind() == super::NATIVE_CODE_AGENT_PROVIDER_KIND;
        if self.identity.schema != AgentProtocolRunIdentityV1::SCHEMA
            || self.identity.protocol != provider.native_protocol()
            || native_code && provider.native_protocol() != AGENT_PROTOCOL_V1
        {
            return Err("Agent run binding has a different provider profile".into());
        }
        self.provider_identity()?
            .validate_for(&provider.profile()?)?;
        if Sha256Digest::parse(self.runtime_spec_digest.as_str())? != self.runtime_spec_digest {
            return Err("Agent Code Runtime spec digest is invalid".into());
        }
        if native_code {
            self.identity
                .validate()
                .map_err(|error| format!("invalid A3S Code run identity ({})", error.code()))?;
        }
        if self.node_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.deployment_id.as_uuid().is_nil()
            || self.replica_id.as_uuid().is_nil()
            || self.runtime_unit_id.trim().is_empty()
            || self.runtime_unit_id.len() > 512
            || self.runtime_unit_id.contains(['\0', '\r', '\n'])
            || self.runtime_generation == 0
            || self.service_port_name.trim().is_empty()
            || self.service_port_name.len() > 128
            || self.service_port_name.contains(['\0', '\r', '\n'])
            || self.bound_at != canonical_timestamp(self.bound_at)
            || self
                .observed_at
                .is_some_and(|value| value != canonical_timestamp(value))
            || (self.observed_at.is_none()
                && (self.accepted_after_event_sequence.is_some()
                    || self.observed_state != AgentProviderRunStateV1::Created))
        {
            return Err("Agent Code run binding is invalid".into());
        }
        if native_code {
            self.node_runtime_binding(uuid::Uuid::from_u128(1))
                .validate()
                .map_err(|error| format!("invalid legacy A3S Code Node binding: {error}"))?;
        }
        self.node_provider_runtime_binding(uuid::Uuid::from_u128(1))?
            .validate()
    }
}

fn page_observed_at(page: &AgentProtocolEventPageV1) -> Result<DateTime<Utc>, String> {
    i64::try_from(page.observed_at_ms)
        .ok()
        .and_then(DateTime::<Utc>::from_timestamp_millis)
        .ok_or_else(|| "A3S Code event page timestamp exceeds supported bounds".to_string())
}

fn provider_page_observed_at(page: &AgentProviderEventPageV1) -> Result<DateTime<Utc>, String> {
    i64::try_from(page.observed_at_ms)
        .ok()
        .and_then(DateTime::<Utc>::from_timestamp_millis)
        .ok_or_else(|| "Agent provider event page timestamp exceeds supported bounds".into())
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

fn is_recovery_run_id(run_id: &str) -> bool {
    run_id
        .strip_prefix("agent-recovery-")
        .is_some_and(|value| uuid::Uuid::parse_str(value).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{
        AgentProviderCapabilityV1, AgentProviderEventRecordV1, AgentProviderProfile,
        AgentProviderSemanticEventV1, AgentProviderToolPayloadIdentityV1,
        HarnessAgentReleaseBindingV1, HarnessInvocationProfileV1, HarnessProviderBindingV1,
        HarnessToolBindingV1, HarnessWorkspaceBindingV1,
    };

    const TOOL_PROVIDER_PROFILE: &str = r#"agent_provider "test.tools" {
  capabilities = ["cancellation", "cleanup", "event_pages", "tool_calls"]
  native_protocol = "test.tools.v1"
  protocol = "a3s.cloud.agent-provider.v1"
  revision = "1.0.0"
  schema = "a3s.cloud.agent-provider-profile.v1"
}
"#;

    fn binding(bound_at: DateTime<Utc>) -> AgentCodeRunBinding {
        AgentCodeRunBinding::new(
            NodeId::new(),
            WorkloadId::new(),
            WorkloadRevisionId::new(),
            DeploymentId::new(),
            WorkloadReplicaId::new(),
            "agent-runtime:revision:1",
            1,
            Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("Runtime digest"),
            "agent",
            AgentProtocolRunIdentityV1 {
                schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
                protocol: a3s_cloud_contracts::AGENT_PROTOCOL_V1.into(),
                agent_release_identity: format!("sha256:{}", "a".repeat(64)),
                session_id: "agent-conversation-1".into(),
                run_id: "agent-execution-1".into(),
            },
            bound_at,
        )
        .expect("Code binding")
    }

    #[test]
    fn recovery_successor_is_deterministic_without_comparing_provider_and_cloud_clocks() {
        let bound_at = canonical_timestamp(Utc::now());
        let execution_id = AgentExecutionId::new();
        let mut checkpoint = binding(bound_at);
        let provider_observed_at = bound_at + chrono::Duration::days(1);
        let checkpoint_page = AgentProtocolEventPageV1 {
            schema: AgentProtocolEventPageV1::SCHEMA.into(),
            identity: checkpoint.identity().clone(),
            after_event_sequence: None,
            first_available_sequence: None,
            latest_sequence_exclusive: 0,
            next_after_event_sequence: None,
            state: AgentProtocolRunStateV1::Planning,
            observed_at_ms: u64::try_from(provider_observed_at.timestamp_millis())
                .expect("provider timestamp"),
            retention_gap: false,
            has_more: false,
            events: Vec::new(),
        };
        checkpoint
            .accept_event_page(&checkpoint_page)
            .expect("checkpoint observation");

        let recovered_at = bound_at + chrono::Duration::seconds(1);
        let mut successor = checkpoint
            .recovery_successor(execution_id, recovered_at)
            .expect("recovery successor");
        let replay = checkpoint
            .recovery_successor(execution_id, recovered_at)
            .expect("deterministic recovery successor");
        assert_eq!(successor, replay);
        assert_eq!(
            successor.identity().run_id,
            AgentCodeRunBinding::recovery_run_id(execution_id, &checkpoint.identity().run_id)
        );
        assert!(successor.is_recovery_successor_of(&checkpoint, execution_id));
        assert!(successor.can_settle_recovery_predecessor_runtime_binding(
            &checkpoint.node_runtime_binding(execution_id.as_uuid()),
            execution_id,
        ));

        let mut managed_checkpoint = checkpoint.clone();
        managed_checkpoint.identity.run_id = format!("agent-execution-{execution_id}");
        let first = managed_checkpoint
            .recovery_successor(execution_id, recovered_at)
            .expect("first managed recovery");
        let second = first
            .recovery_successor(execution_id, recovered_at + chrono::Duration::seconds(1))
            .expect("second managed recovery");
        assert!(second.can_settle_recovery_predecessor_runtime_binding(
            &managed_checkpoint.node_runtime_binding(execution_id.as_uuid()),
            execution_id,
        ));

        let successor_page = AgentProtocolEventPageV1 {
            schema: AgentProtocolEventPageV1::SCHEMA.into(),
            identity: successor.identity().clone(),
            after_event_sequence: None,
            first_available_sequence: None,
            latest_sequence_exclusive: 0,
            next_after_event_sequence: None,
            state: AgentProtocolRunStateV1::Executing,
            observed_at_ms: u64::try_from(
                (provider_observed_at + chrono::Duration::seconds(1)).timestamp_millis(),
            )
            .expect("successor provider timestamp"),
            retention_gap: false,
            has_more: false,
            events: Vec::new(),
        };
        successor
            .accept_event_page(&successor_page)
            .expect("advance recovered run");
        assert!(!successor.is_initial());
        assert!(successor.is_recovery_successor_of(&checkpoint, execution_id));
    }

    #[test]
    fn provider_tool_events_must_match_the_immutable_invocation_profile() {
        let bound_at = canonical_timestamp(Utc::now());
        let provider =
            AgentProviderProfile::parse_acl(TOOL_PROVIDER_PROFILE).expect("Tool provider profile");
        let provider_binding = super::super::AgentProviderProfileBinding::from_profile(&provider)
            .expect("Tool provider binding");
        let workload_id = WorkloadId::new();
        let workload_revision_id = WorkloadRevisionId::new();
        let runtime_spec_digest =
            Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("Runtime digest");
        let tool = HarnessToolBindingV1 {
            name: "workspace.search".into(),
            revision: "1.0.0".into(),
            contract_digest: format!("sha256:{}", "c".repeat(64)),
            approval_required: false,
        };
        let invocation = HarnessInvocationProfileV1 {
            schema: HarnessInvocationProfileV1::SCHEMA.into(),
            agent: HarnessAgentReleaseBindingV1 {
                organization_id: uuid::Uuid::from_u128(1),
                asset_id: uuid::Uuid::from_u128(2),
                asset_release_id: uuid::Uuid::from_u128(3),
                build_run_id: uuid::Uuid::from_u128(4),
                artifact_digest: format!("sha256:{}", "a".repeat(64)),
            },
            provider: HarnessProviderBindingV1 {
                kind: provider.kind().into(),
                revision: provider.revision().into(),
                profile_digest: provider.digest().into(),
                capability_digest: provider.capability_digest().into(),
            },
            instructions_digest: format!("sha256:{}", "a".repeat(64)),
            environment_policy_digest: format!("sha256:{}", "d".repeat(64)),
            security_policy_digest: format!("sha256:{}", "e".repeat(64)),
            workspace: HarnessWorkspaceBindingV1 {
                workload_id: workload_id.as_uuid(),
                workload_revision_id: workload_revision_id.as_uuid(),
                runtime_unit_id: "agent-runtime:revision:1".into(),
                runtime_generation: 1,
                runtime_spec_digest: runtime_spec_digest.as_str().into(),
                working_directory: Some("/workspace".into()),
            },
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            models: Vec::new(),
            secrets: Vec::new(),
            tools: vec![tool.clone()],
            required_capabilities: vec![
                AgentProviderCapabilityV1::Cancellation,
                AgentProviderCapabilityV1::Cleanup,
                AgentProviderCapabilityV1::EventPages,
                AgentProviderCapabilityV1::ToolCalls,
            ],
        };
        let unprofiled = AgentCodeRunBinding::new_with_provider(
            provider_binding,
            NodeId::new(),
            workload_id,
            workload_revision_id,
            DeploymentId::new(),
            WorkloadReplicaId::new(),
            "agent-runtime:revision:1",
            1,
            runtime_spec_digest,
            "agent",
            AgentProtocolRunIdentityV1 {
                schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
                protocol: provider.native_protocol().into(),
                agent_release_identity: format!("sha256:{}", "a".repeat(64)),
                session_id: "agent-conversation-1".into(),
                run_id: "agent-execution-1".into(),
            },
            bound_at,
        )
        .expect("provider binding");
        let mut observed = unprofiled.clone();
        let observed_identity = observed.provider_identity().expect("provider identity");
        observed
            .accept_provider_event_page(&AgentProviderEventPageV1 {
                schema: AgentProviderEventPageV1::SCHEMA.into(),
                identity: observed_identity,
                after_event_sequence: None,
                first_available_sequence: None,
                source_first_sequence: None,
                source_last_sequence: None,
                source_event_count: 0,
                latest_sequence_exclusive: 0,
                next_after_event_sequence: None,
                state: AgentProviderRunStateV1::Planning,
                observed_at_ms: u64::try_from(bound_at.timestamp_millis()).expect("event time") + 1,
                retention_gap: false,
                has_more: false,
                terminal_failure: None,
                events: Vec::new(),
            })
            .expect("provider observation");
        assert!(observed
            .with_invocation_profile(invocation.clone())
            .is_err());
        let binding = unprofiled
            .with_invocation_profile(invocation)
            .expect("invocation profile");
        let event = |tool: HarnessToolBindingV1| AgentProviderEventPageV1 {
            schema: AgentProviderEventPageV1::SCHEMA.into(),
            identity: binding.provider_identity().expect("provider identity"),
            after_event_sequence: None,
            first_available_sequence: Some(0),
            source_first_sequence: Some(0),
            source_last_sequence: Some(0),
            source_event_count: 1,
            latest_sequence_exclusive: 1,
            next_after_event_sequence: Some(0),
            state: AgentProviderRunStateV1::Executing,
            observed_at_ms: u64::try_from(bound_at.timestamp_millis()).expect("event time") + 1,
            retention_gap: false,
            has_more: false,
            terminal_failure: None,
            events: vec![AgentProviderEventRecordV1 {
                sequence: 0,
                occurred_at_ms: u64::try_from(bound_at.timestamp_millis()).expect("event time") + 1,
                event: AgentProviderSemanticEventV1::ToolRequest {
                    call_id: "call-1".into(),
                    tool,
                    request: AgentProviderToolPayloadIdentityV1 {
                        digest: format!("sha256:{}", "f".repeat(64)),
                        size_bytes: 128,
                        media_type: "application/json".into(),
                    },
                },
            }],
        };

        let mut accepted = binding.clone();
        accepted
            .accept_provider_event_page(&event(tool.clone()))
            .expect("pinned Tool event");
        let mut changed = tool;
        changed.revision = "2.0.0".into();
        assert!(binding
            .clone()
            .accept_provider_event_page(&event(changed))
            .is_err());
    }
}
