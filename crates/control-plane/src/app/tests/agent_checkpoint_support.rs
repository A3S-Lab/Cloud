use crate::modules::agents::{
    AgentCodeRunBinding, AgentExecution, AgentExecutionCheckpointObjectError,
    AgentExecutionCheckpointObjectReference, AgentExecutionCheckpointObjectWrite,
    IAgentExecutionCheckpointObjectStore,
};
use crate::modules::shared_kernel::domain::{
    DeploymentId, NodeId, Sha256Digest, WorkloadId, WorkloadReplicaId, WorkloadRevisionId,
};
use a3s_boot::BootError;
use a3s_cloud_contracts::{
    AgentProtocolRunIdentityV1, AgentProviderCapabilityV1, HarnessAgentReleaseBindingV1,
    HarnessInvocationProfileV1, HarnessProviderBindingV1, HarnessWorkspaceBindingV1,
    AGENT_PROTOCOL_V1,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub(super) struct TestAgentExecutionCheckpointObjectStore {
    bodies: RwLock<BTreeMap<String, Vec<u8>>>,
}

impl TestAgentExecutionCheckpointObjectStore {
    fn validate(
        reference: &AgentExecutionCheckpointObjectReference,
        body: &[u8],
    ) -> Result<(), AgentExecutionCheckpointObjectError> {
        reference
            .validate()
            .map_err(AgentExecutionCheckpointObjectError::Invalid)?;
        if u64::try_from(body.len()).map_err(|_| {
            AgentExecutionCheckpointObjectError::Integrity(
                "test checkpoint object length overflowed".into(),
            )
        })? != reference.size_bytes
            || Sha256Digest::from_bytes(body) != reference.digest
        {
            return Err(AgentExecutionCheckpointObjectError::Integrity(
                "test checkpoint object changed its digest or length".into(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl IAgentExecutionCheckpointObjectStore for TestAgentExecutionCheckpointObjectStore {
    async fn put(
        &self,
        reference: &AgentExecutionCheckpointObjectReference,
        body: Vec<u8>,
    ) -> Result<AgentExecutionCheckpointObjectWrite, AgentExecutionCheckpointObjectError> {
        Self::validate(reference, &body)?;
        let mut bodies = self.bodies.write().await;
        match bodies.get(&reference.object_ref) {
            Some(existing) if existing == &body => {
                Ok(AgentExecutionCheckpointObjectWrite { replayed: true })
            }
            Some(_) => Err(AgentExecutionCheckpointObjectError::Conflict(
                reference.object_ref.clone(),
            )),
            None => {
                bodies.insert(reference.object_ref.clone(), body);
                Ok(AgentExecutionCheckpointObjectWrite { replayed: false })
            }
        }
    }

    async fn get(
        &self,
        reference: &AgentExecutionCheckpointObjectReference,
    ) -> Result<Vec<u8>, AgentExecutionCheckpointObjectError> {
        let body = self
            .bodies
            .read()
            .await
            .get(&reference.object_ref)
            .cloned()
            .ok_or(AgentExecutionCheckpointObjectError::NotFound)?;
        Self::validate(reference, &body)?;
        Ok(body)
    }
}

pub(super) fn checkpoint_test_binding(
    execution: &AgentExecution,
) -> a3s_boot::Result<AgentCodeRunBinding> {
    let workload_id = WorkloadId::new();
    let workload_revision_id = WorkloadRevisionId::new();
    let runtime_unit_id = "agent-runtime:revision:checkpoint-test";
    let runtime_spec_digest =
        Sha256Digest::parse(format!("sha256:{}", "c".repeat(64))).map_err(BootError::Internal)?;
    let mut required_capabilities = vec![
        AgentProviderCapabilityV1::Cancellation,
        AgentProviderCapabilityV1::Cleanup,
        AgentProviderCapabilityV1::EventPages,
    ];
    required_capabilities.sort_by_key(|capability| capability.as_str());
    let invocation = HarnessInvocationProfileV1 {
        schema: HarnessInvocationProfileV1::SCHEMA.into(),
        agent: HarnessAgentReleaseBindingV1 {
            organization_id: execution.organization_id.as_uuid(),
            asset_id: execution.agent.asset_id().as_uuid(),
            asset_release_id: execution.agent.asset_release_id().as_uuid(),
            build_run_id: execution.agent.build_run_id().as_uuid(),
            artifact_digest: execution.agent.artifact_digest().as_str().into(),
        },
        provider: HarnessProviderBindingV1 {
            kind: execution.provider.kind().into(),
            revision: execution.provider.revision().into(),
            profile_digest: execution.provider.profile_digest().into(),
            capability_digest: execution.provider.capability_digest().into(),
        },
        instructions_digest: execution.agent.artifact_digest().as_str().into(),
        environment_policy_digest: format!("sha256:{}", "d".repeat(64)),
        security_policy_digest: format!("sha256:{}", "e".repeat(64)),
        workspace: HarnessWorkspaceBindingV1 {
            workload_id: workload_id.as_uuid(),
            workload_revision_id: workload_revision_id.as_uuid(),
            runtime_unit_id: runtime_unit_id.into(),
            runtime_generation: 1,
            runtime_spec_digest: runtime_spec_digest.as_str().into(),
            working_directory: Some("/workspace".into()),
        },
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        models: Vec::new(),
        secrets: Vec::new(),
        tools: Vec::new(),
        required_capabilities,
    };
    AgentCodeRunBinding::new(
        NodeId::new(),
        workload_id,
        workload_revision_id,
        DeploymentId::new(),
        WorkloadReplicaId::new(),
        runtime_unit_id,
        1,
        runtime_spec_digest,
        "agent",
        AgentProtocolRunIdentityV1 {
            schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
            protocol: AGENT_PROTOCOL_V1.into(),
            agent_release_identity: execution.agent.artifact_digest().as_str().into(),
            session_id: format!("agent-conversation-{}", execution.conversation_id),
            run_id: format!("agent-execution-{}", execution.id),
        },
        execution.updated_at + chrono::Duration::milliseconds(1),
    )
    .and_then(|binding| binding.with_invocation_profile(invocation))
    .map_err(BootError::Internal)
}

pub(super) fn checkpoint_test_fork_binding(
    execution: &AgentExecution,
    parent: &AgentCodeRunBinding,
) -> a3s_boot::Result<AgentCodeRunBinding> {
    AgentCodeRunBinding::new_with_provider(
        execution.provider.clone(),
        parent.node_id(),
        parent.workload_id(),
        parent.workload_revision_id(),
        parent.deployment_id(),
        parent.replica_id(),
        parent.runtime_unit_id(),
        parent.runtime_generation(),
        parent.runtime_spec_digest().clone(),
        parent.service_port_name(),
        AgentProtocolRunIdentityV1 {
            schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
            protocol: execution.provider.native_protocol().into(),
            agent_release_identity: execution.agent.artifact_digest().as_str().into(),
            session_id: format!("agent-conversation-{}", execution.conversation_id),
            run_id: format!("agent-execution-{}", execution.id),
        },
        execution.updated_at + chrono::Duration::milliseconds(1),
    )
    .and_then(|binding| {
        binding.with_invocation_profile(parent.require_invocation_profile()?.clone())
    })
    .map_err(BootError::Internal)
}
