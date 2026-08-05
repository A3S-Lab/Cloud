use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DeploymentId, NodeId, Sha256Digest, WorkloadId, WorkloadReplicaId,
    WorkloadRevisionId,
};
use a3s_cloud_contracts::{
    AgentProtocolEventPageV1, AgentProtocolRunIdentityV1, AgentProtocolRunStateV1,
    NodeCodeAgentRuntimeBindingV1,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCodeRunBinding {
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
    observed_state: AgentProtocolRunStateV1,
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
        Self::restore(
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
            AgentProtocolRunStateV1::Created,
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
        let binding = Self {
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

    pub const fn observed_state(&self) -> AgentProtocolRunStateV1 {
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
            && self.observed_state == AgentProtocolRunStateV1::Created
            && self.observed_at.is_none()
    }

    pub fn has_same_run_binding(&self, other: &Self) -> bool {
        self.node_id == other.node_id
            && self.workload_id == other.workload_id
            && self.workload_revision_id == other.workload_revision_id
            && self.deployment_id == other.deployment_id
            && self.replica_id == other.replica_id
            && self.runtime_unit_id == other.runtime_unit_id
            && self.runtime_generation == other.runtime_generation
            && self.runtime_spec_digest == other.runtime_spec_digest
            && self.service_port_name == other.service_port_name
            && self.identity == other.identity
            && self.bound_at == other.bound_at
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

    pub fn accept_event_page(&mut self, page: &AgentProtocolEventPageV1) -> Result<(), String> {
        page.validate()
            .map_err(|error| format!("invalid A3S Code event page ({})", error.code()))?;
        let page_observed_at = i64::try_from(page.observed_at_ms)
            .ok()
            .and_then(DateTime::<Utc>::from_timestamp_millis)
            .ok_or_else(|| "A3S Code event page timestamp exceeds supported bounds".to_string())?;
        if page.identity != self.identity
            || page.after_event_sequence != self.accepted_after_event_sequence
            || page.retention_gap
            || self
                .observed_at
                .is_some_and(|observed_at| page_observed_at < observed_at)
            || self.observed_state.is_terminal() && page.state != self.observed_state
        {
            return Err("A3S Code event page does not continue its exact bound run".into());
        }
        self.accepted_after_event_sequence = page.next_after_event_sequence;
        self.observed_state = page.state;
        self.observed_at = Some(canonical_timestamp(page_observed_at));
        self.validate()
    }

    pub fn validate(&self) -> Result<(), String> {
        if Sha256Digest::parse(self.runtime_spec_digest.as_str())? != self.runtime_spec_digest {
            return Err("Agent Code Runtime spec digest is invalid".into());
        }
        self.identity
            .validate()
            .map_err(|error| format!("invalid A3S Code run identity ({})", error.code()))?;
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
                    || self.observed_state != AgentProtocolRunStateV1::Created))
        {
            return Err("Agent Code run binding is invalid".into());
        }
        self.node_runtime_binding(uuid::Uuid::from_u128(1))
            .validate()
    }
}
