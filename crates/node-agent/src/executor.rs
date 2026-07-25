use crate::resource_claim::{self, ResourceClaimExecutionError};
use crate::{
    CommandJournalError, FileCommandJournal, GatewaySnapshotInstallError,
    GatewaySnapshotInstallOutcome, GatewaySnapshotInstaller, JournalDecision, NodeArtifactError,
    NodeArtifactManager, NodeResourceInventoryAuthority,
};
use a3s_cloud_contracts::{
    GatewayAckState, NodeCommandAck, NodeCommandEnvelope, NodeCommandFailure, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult, NodeGatewayAck, NodeResourceClaimPrepared,
    NodeResourceClaimReleased,
};
use a3s_runtime::contract::RuntimeInspection;
use a3s_runtime::{RuntimeClient, RuntimeError};
use chrono::{DateTime, Utc};
use std::sync::Arc;

pub struct CommandExecutor {
    journal: FileCommandJournal,
    runtime: Arc<dyn RuntimeClient>,
    gateway: Arc<dyn GatewaySnapshotInstaller>,
    artifacts: Option<Arc<NodeArtifactManager>>,
    resource_inventory: Option<Arc<dyn NodeResourceInventoryAuthority>>,
}

impl CommandExecutor {
    pub fn runtime_only(journal: FileCommandJournal, runtime: Arc<dyn RuntimeClient>) -> Self {
        Self::new(journal, runtime, Arc::new(RuntimeOnlyGatewayInstaller))
    }

    pub fn new(
        journal: FileCommandJournal,
        runtime: Arc<dyn RuntimeClient>,
        gateway: Arc<dyn GatewaySnapshotInstaller>,
    ) -> Self {
        Self {
            journal,
            runtime,
            gateway,
            artifacts: None,
            resource_inventory: None,
        }
    }

    pub fn with_artifacts(mut self, artifacts: Arc<NodeArtifactManager>) -> Self {
        self.artifacts = Some(artifacts);
        self
    }

    pub fn with_resource_inventory(
        mut self,
        resource_inventory: Arc<dyn NodeResourceInventoryAuthority>,
    ) -> Self {
        self.resource_inventory = Some(resource_inventory);
        self
    }

    pub async fn execute(
        &self,
        envelope: NodeCommandEnvelope,
    ) -> Result<NodeCommandAck, CommandExecutionError> {
        match self.journal.begin(envelope.clone()).await? {
            JournalDecision::Replay(acknowledgement) => return Ok(acknowledgement),
            JournalDecision::Execute => {}
        }
        let now = Utc::now();
        let outcome = if envelope.is_expired_at(now) {
            rejected("command_expired", "command expired before Runtime dispatch")
        } else {
            match self.dispatch(&envelope).await {
                Ok(_) if Utc::now() > envelope.not_after => NodeCommandOutcome::Failed {
                    failure: NodeCommandFailure {
                        code: "command_completed_after_deadline".into(),
                        message: "Runtime operation completed after the command deadline".into(),
                        retryable: true,
                    },
                },
                Ok(result) => NodeCommandOutcome::Succeeded {
                    result: Box::new(result),
                },
                Err(error) => dispatch_failure(error),
            }
        };
        let completed_at = completion_timestamp(&envelope, &outcome);
        self.journal
            .complete(envelope.command_id, completed_at, outcome)
            .await
            .map_err(Into::into)
    }

    pub fn journal(&self) -> &FileCommandJournal {
        &self.journal
    }

    async fn dispatch(
        &self,
        envelope: &NodeCommandEnvelope,
    ) -> Result<NodeCommandResult, DispatchError> {
        match &envelope.payload {
            NodeCommandPayload::ResourceClaimPrepare { request } => {
                let inventory = self.current_resource_inventory().await?;
                let active = self.journal.active_resource_claim_bindings().await?;
                resource_claim::validate_prepare(request, &inventory, &active)?;
                Ok(NodeCommandResult::ResourceClaimPrepared {
                    prepared: NodeResourceClaimPrepared::new(request, command_timestamp(envelope))
                        .map_err(ResourceClaimExecutionError::Invalid)?,
                })
            }
            NodeCommandPayload::RuntimeApply {
                request,
                resource_claim,
            } => {
                self.journal
                    .validate_runtime_resource_binding(
                        request.spec.clone(),
                        resource_claim.as_deref().cloned(),
                    )
                    .await?;
                if let Some(binding) = resource_claim {
                    let inventory = self.current_resource_inventory().await?;
                    binding
                        .validate_inventory(&inventory)
                        .map_err(ResourceClaimExecutionError::Conflict)?;
                }
                if let Some(artifacts) = &self.artifacts {
                    artifacts.prepare_command(envelope).await?;
                }
                let mut observation = self.runtime.apply(request).await?;
                if let Some(artifacts) = &self.artifacts {
                    observation = artifacts
                        .publish_command_outputs(envelope, &observation)
                        .await?;
                }
                if let Some(binding) = resource_claim {
                    binding
                        .bind_runtime_observation(&mut observation)
                        .map_err(ResourceClaimExecutionError::Conflict)?;
                }
                Ok(NodeCommandResult::RuntimeApplied {
                    observation: Box::new(observation),
                })
            }
            NodeCommandPayload::RuntimeInspect {
                unit_id,
                generation,
            } => {
                let mut inspection = self.runtime.inspect(unit_id).await?;
                if let RuntimeInspection::Found { observation, .. } = &mut inspection {
                    if observation.generation != *generation {
                        return Err((if observation.generation > *generation {
                            RuntimeError::StaleGeneration {
                                unit_id: unit_id.clone(),
                                requested: *generation,
                                current: observation.generation,
                            }
                        } else {
                            RuntimeError::GenerationConflict {
                                unit_id: unit_id.clone(),
                                generation: *generation,
                            }
                        })
                        .into());
                    }
                    if let Some(binding) = self
                        .journal
                        .active_resource_claim_bindings()
                        .await?
                        .into_iter()
                        .find(|binding| {
                            binding.runtime_unit_id == *unit_id
                                && binding.runtime_generation == *generation
                        })
                    {
                        binding
                            .bind_runtime_observation(observation)
                            .map_err(ResourceClaimExecutionError::Conflict)?;
                    }
                }
                Ok(NodeCommandResult::RuntimeInspected { inspection })
            }
            NodeCommandPayload::RuntimeStop { request } => {
                let inspection = self.runtime.stop(request).await?;
                Ok(NodeCommandResult::RuntimeStopped { inspection })
            }
            NodeCommandPayload::RuntimeRemove { request } => {
                let removal = self.runtime.remove(request).await?;
                Ok(NodeCommandResult::RuntimeRemoved { removal })
            }
            NodeCommandPayload::ResourceClaimRelease { request } => {
                self.journal
                    .validate_resource_claim_release(request.as_ref().clone())
                    .await?;
                Ok(NodeCommandResult::ResourceClaimReleased {
                    released: NodeResourceClaimReleased::new(request, command_timestamp(envelope))
                        .map_err(ResourceClaimExecutionError::Invalid)?,
                })
            }
            NodeCommandPayload::GatewaySnapshotInstall { snapshot } => {
                let installed = self.gateway.install(snapshot).await?;
                let (state, message, management_protocol) = match installed {
                    GatewaySnapshotInstallOutcome::Applied { protocol } => {
                        (GatewayAckState::Applied, None, Some(protocol))
                    }
                    GatewaySnapshotInstallOutcome::Rejected { message, protocol } => {
                        (GatewayAckState::Rejected, Some(message), protocol)
                    }
                };
                let acknowledgement = NodeGatewayAck {
                    schema: NodeGatewayAck::SCHEMA.into(),
                    acknowledgement_id: uuid::Uuid::now_v7(),
                    command_id: envelope.command_id,
                    node_id: envelope.node_id,
                    gateway_id: snapshot.gateway_id,
                    revision: snapshot.revision,
                    snapshot_digest: snapshot.snapshot_digest.clone(),
                    expires_at: snapshot.expires_at,
                    state,
                    ready: state == GatewayAckState::Applied,
                    message,
                    acknowledged_at: command_timestamp(envelope),
                    management_protocol,
                };
                acknowledgement
                    .validate_for(envelope.command_id, envelope.node_id, snapshot)
                    .map_err(|error| {
                        DispatchError::Gateway(GatewaySnapshotInstallError::Protocol(error))
                    })?;
                Ok(NodeCommandResult::GatewaySnapshotInstalled { acknowledgement })
            }
        }
    }

    async fn current_resource_inventory(
        &self,
    ) -> Result<a3s_cloud_contracts::NodeResourceInventory, ResourceClaimExecutionError> {
        self.resource_inventory
            .as_ref()
            .ok_or(ResourceClaimExecutionError::AuthorityUnavailable)?
            .current_resource_inventory()
            .await
            .map_err(Into::into)
    }
}

fn command_timestamp(envelope: &NodeCommandEnvelope) -> DateTime<Utc> {
    Utc::now().max(envelope.issued_at)
}

fn completion_timestamp(
    envelope: &NodeCommandEnvelope,
    outcome: &NodeCommandOutcome,
) -> DateTime<Utc> {
    let evidence_at = match outcome {
        NodeCommandOutcome::Succeeded { result } => match result.as_ref() {
            NodeCommandResult::ResourceClaimPrepared { prepared } => Some(prepared.prepared_at),
            NodeCommandResult::ResourceClaimReleased { released } => Some(released.released_at),
            NodeCommandResult::GatewaySnapshotInstalled { acknowledgement } => {
                Some(acknowledgement.acknowledged_at)
            }
            NodeCommandResult::RuntimeApplied { .. }
            | NodeCommandResult::RuntimeInspected { .. }
            | NodeCommandResult::RuntimeStopped { .. }
            | NodeCommandResult::RuntimeRemoved { .. } => None,
        },
        NodeCommandOutcome::Rejected { .. } | NodeCommandOutcome::Failed { .. } => None,
    };
    evidence_at.map_or_else(
        || command_timestamp(envelope),
        |evidence_at| command_timestamp(envelope).max(evidence_at),
    )
}

struct RuntimeOnlyGatewayInstaller;

#[async_trait::async_trait]
impl GatewaySnapshotInstaller for RuntimeOnlyGatewayInstaller {
    async fn install(
        &self,
        _snapshot: &a3s_cloud_contracts::GatewaySnapshot,
    ) -> Result<GatewaySnapshotInstallOutcome, GatewaySnapshotInstallError> {
        Err(GatewaySnapshotInstallError::Protocol(
            "Gateway installer is not configured for this Runtime-only executor".into(),
        ))
    }
}

enum DispatchError {
    Runtime(RuntimeError),
    Gateway(GatewaySnapshotInstallError),
    Artifact(NodeArtifactError),
    ResourceClaim(ResourceClaimExecutionError),
}

impl From<RuntimeError> for DispatchError {
    fn from(error: RuntimeError) -> Self {
        Self::Runtime(error)
    }
}

impl From<GatewaySnapshotInstallError> for DispatchError {
    fn from(error: GatewaySnapshotInstallError) -> Self {
        Self::Gateway(error)
    }
}

impl From<NodeArtifactError> for DispatchError {
    fn from(error: NodeArtifactError) -> Self {
        Self::Artifact(error)
    }
}

impl From<ResourceClaimExecutionError> for DispatchError {
    fn from(error: ResourceClaimExecutionError) -> Self {
        Self::ResourceClaim(error)
    }
}

impl From<CommandJournalError> for DispatchError {
    fn from(error: CommandJournalError) -> Self {
        Self::ResourceClaim(error.into())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CommandExecutionError {
    #[error(transparent)]
    Journal(#[from] CommandJournalError),
}

fn rejected(code: &str, message: &str) -> NodeCommandOutcome {
    NodeCommandOutcome::Rejected {
        failure: NodeCommandFailure {
            code: code.into(),
            message: message.into(),
            retryable: false,
        },
    }
}

fn runtime_failure(error: RuntimeError) -> NodeCommandOutcome {
    let (status, code, retryable) = match error {
        RuntimeError::InvalidRequest(_) => (FailureStatus::Rejected, "invalid_request", false),
        RuntimeError::NotFound { .. } | RuntimeError::RequestNotFound { .. } => {
            (FailureStatus::Rejected, "not_found", false)
        }
        RuntimeError::RequestConflict { .. } => {
            (FailureStatus::Rejected, "request_conflict", false)
        }
        RuntimeError::StaleGeneration { .. } => {
            (FailureStatus::Rejected, "stale_generation", false)
        }
        RuntimeError::GenerationConflict { .. } => {
            (FailureStatus::Rejected, "generation_conflict", false)
        }
        RuntimeError::DeadlineExceeded(_) => (FailureStatus::Rejected, "deadline_exceeded", false),
        RuntimeError::UnsupportedCapabilities(_) => {
            (FailureStatus::Rejected, "unsupported_capabilities", false)
        }
        RuntimeError::ProviderUnavailable(_) => {
            (FailureStatus::Failed, "provider_unavailable", true)
        }
        RuntimeError::Transport(_) => (FailureStatus::Failed, "runtime_transport", true),
        RuntimeError::LogDiscontinuity { .. } => {
            (FailureStatus::Failed, "log_discontinuity", false)
        }
        RuntimeError::Protocol(_) => (FailureStatus::Failed, "runtime_protocol", false),
    };
    let failure = NodeCommandFailure {
        code: code.into(),
        message: sanitize_error(&error.to_string()),
        retryable,
    };
    match status {
        FailureStatus::Rejected => NodeCommandOutcome::Rejected { failure },
        FailureStatus::Failed => NodeCommandOutcome::Failed { failure },
    }
}

fn dispatch_failure(error: DispatchError) -> NodeCommandOutcome {
    match error {
        DispatchError::Runtime(error) => runtime_failure(error),
        DispatchError::Gateway(error) => {
            let failure = NodeCommandFailure {
                code: error.code().into(),
                message: sanitize_error(&error.to_string()),
                retryable: error.retryable(),
            };
            if error.retryable() {
                NodeCommandOutcome::Failed { failure }
            } else {
                NodeCommandOutcome::Rejected { failure }
            }
        }
        DispatchError::Artifact(error) => artifact_failure(error),
        DispatchError::ResourceClaim(error) => {
            let failure = NodeCommandFailure {
                code: error.code().into(),
                message: sanitize_error(&error.to_string()),
                retryable: error.retryable(),
            };
            if error.retryable() {
                NodeCommandOutcome::Failed { failure }
            } else {
                NodeCommandOutcome::Rejected { failure }
            }
        }
    }
}

fn artifact_failure(error: NodeArtifactError) -> NodeCommandOutcome {
    let (status, code, retryable) = match &error {
        NodeArtifactError::Invalid(_) => (FailureStatus::Rejected, "invalid_artifact", false),
        NodeArtifactError::Integrity(_) => (FailureStatus::Failed, "artifact_integrity", false),
        NodeArtifactError::Storage(_) => (FailureStatus::Failed, "artifact_storage", true),
        NodeArtifactError::Transport(_) => (
            FailureStatus::Failed,
            "artifact_transport",
            error.retryable(),
        ),
    };
    let failure = NodeCommandFailure {
        code: code.into(),
        message: sanitize_error(&error.to_string()),
        retryable,
    };
    match status {
        FailureStatus::Rejected => NodeCommandOutcome::Rejected { failure },
        FailureStatus::Failed => NodeCommandOutcome::Failed { failure },
    }
}

enum FailureStatus {
    Rejected,
    Failed,
}

fn sanitize_error(message: &str) -> String {
    let message = message.replace(['\0', '\r', '\n'], " ");
    let message = message.trim();
    if message.is_empty() {
        "Runtime operation failed".into()
    } else {
        message.chars().take(16 * 1024).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{
        GatewaySnapshot, NodeCommandMetadata, NodeCommandPayload, NodeResourceClaimBinding,
        NodeResourceClaimPrepare, NodeResourceClaimRelease, NodeResourceInventory,
        NodeResourceSlot, ResourceAllocation, ResourceKind, ResourceSlotBinding, ResourceUnit,
    };
    use a3s_runtime::contract::{
        ArtifactRef, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy,
        RuntimeActionRequest, RuntimeApplyRequest, RuntimeCapabilities, RuntimeEvidence,
        RuntimeExecRequest, RuntimeExecResult, RuntimeLogChunk, RuntimeLogQuery,
        RuntimeNetworkSpec, RuntimeObservation, RuntimeProcessSpec, RuntimeRemoval,
        RuntimeUnitClass, RuntimeUnitSpec, RuntimeUnitState,
    };
    use a3s_runtime::RuntimeResult;
    use async_trait::async_trait;
    use chrono::Duration;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use uuid::Uuid;

    struct InspectRuntime {
        calls: AtomicUsize,
        error: bool,
    }

    struct InspectGateway {
        calls: AtomicUsize,
        outcome: GatewaySnapshotInstallOutcome,
    }

    struct FixedInventoryAuthority {
        inventory: NodeResourceInventory,
    }

    #[async_trait]
    impl NodeResourceInventoryAuthority for FixedInventoryAuthority {
        async fn current_resource_inventory(
            &self,
        ) -> Result<NodeResourceInventory, crate::ResourceInventoryError> {
            Ok(self.inventory.clone())
        }
    }

    struct ClaimRuntime {
        apply_calls: AtomicUsize,
        stop_not_found: AtomicBool,
    }

    #[async_trait]
    impl RuntimeClient for ClaimRuntime {
        async fn capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
            Err(RuntimeError::Protocol("unused capabilities call".into()))
        }

        async fn apply(&self, request: &RuntimeApplyRequest) -> RuntimeResult<RuntimeObservation> {
            self.apply_calls.fetch_add(1, Ordering::SeqCst);
            Ok(claim_observation(&request.spec, RuntimeUnitState::Running))
        }

        async fn inspect(&self, unit_id: &str) -> RuntimeResult<RuntimeInspection> {
            Ok(RuntimeInspection::NotFound {
                schema: RuntimeInspection::SCHEMA.into(),
                unit_id: unit_id.into(),
                last_generation: None,
            })
        }

        async fn stop(&self, request: &RuntimeActionRequest) -> RuntimeResult<RuntimeInspection> {
            if self.stop_not_found.load(Ordering::SeqCst) {
                return Err(RuntimeError::NotFound {
                    unit_id: request.unit_id.clone(),
                });
            }
            let mut spec = claim_runtime_spec();
            spec.unit_id = request.unit_id.clone();
            spec.generation = request.generation;
            Ok(RuntimeInspection::Found {
                schema: RuntimeInspection::SCHEMA.into(),
                observation: Box::new(claim_observation(&spec, RuntimeUnitState::Stopped)),
            })
        }

        async fn remove(&self, request: &RuntimeActionRequest) -> RuntimeResult<RuntimeRemoval> {
            Ok(RuntimeRemoval {
                schema: RuntimeRemoval::SCHEMA.into(),
                request_id: request.request_id.clone(),
                unit_id: request.unit_id.clone(),
                generation: request.generation,
                removed_at_ms: 2_000,
                already_absent: false,
            })
        }

        async fn logs(&self, _query: &RuntimeLogQuery) -> RuntimeResult<Vec<RuntimeLogChunk>> {
            Ok(Vec::new())
        }

        async fn exec(&self, _request: &RuntimeExecRequest) -> RuntimeResult<RuntimeExecResult> {
            Err(RuntimeError::Protocol("unused exec call".into()))
        }
    }

    #[async_trait]
    impl GatewaySnapshotInstaller for InspectGateway {
        async fn install(
            &self,
            _snapshot: &GatewaySnapshot,
        ) -> Result<GatewaySnapshotInstallOutcome, GatewaySnapshotInstallError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.outcome.clone())
        }
    }

    fn gateway() -> Arc<InspectGateway> {
        Arc::new(InspectGateway {
            calls: AtomicUsize::new(0),
            outcome: GatewaySnapshotInstallOutcome::Applied {
                protocol: a3s_cloud_contracts::GatewayManagementProtocol::v1(
                    a3s_cloud_contracts::GatewayManagementProtocolDiscovery::Advertised,
                ),
            },
        })
    }

    #[async_trait]
    impl RuntimeClient for InspectRuntime {
        async fn capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
            Err(RuntimeError::Protocol("unused capabilities call".into()))
        }

        async fn apply(&self, _request: &RuntimeApplyRequest) -> RuntimeResult<RuntimeObservation> {
            Err(RuntimeError::Protocol("unused apply call".into()))
        }

        async fn inspect(&self, unit_id: &str) -> RuntimeResult<RuntimeInspection> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.error {
                Err(RuntimeError::ProviderUnavailable(
                    "Docker is offline".into(),
                ))
            } else {
                Ok(RuntimeInspection::NotFound {
                    schema: RuntimeInspection::SCHEMA.into(),
                    unit_id: unit_id.into(),
                    last_generation: Some(1),
                })
            }
        }

        async fn stop(&self, _request: &RuntimeActionRequest) -> RuntimeResult<RuntimeInspection> {
            Err(RuntimeError::Protocol("unused stop call".into()))
        }

        async fn remove(&self, _request: &RuntimeActionRequest) -> RuntimeResult<RuntimeRemoval> {
            Err(RuntimeError::Protocol("unused remove call".into()))
        }

        async fn logs(&self, _query: &RuntimeLogQuery) -> RuntimeResult<Vec<RuntimeLogChunk>> {
            Err(RuntimeError::Protocol("unused logs call".into()))
        }

        async fn exec(&self, _request: &RuntimeExecRequest) -> RuntimeResult<RuntimeExecResult> {
            Err(RuntimeError::Protocol("unused exec call".into()))
        }
    }

    fn command(
        node_id: Uuid,
        command_id: Uuid,
        lease_id: Uuid,
        not_after: chrono::DateTime<Utc>,
    ) -> NodeCommandEnvelope {
        let issued_at = Utc::now() - Duration::seconds(1);
        NodeCommandEnvelope::new(
            NodeCommandMetadata {
                command_id,
                lease_id,
                node_id,
                sequence: 1,
                aggregate_id: Uuid::now_v7(),
                issued_at,
                not_after,
                correlation_id: Uuid::now_v7(),
            },
            NodeCommandPayload::RuntimeInspect {
                unit_id: "service-1".into(),
                generation: 1,
            },
        )
        .expect("command")
    }

    fn claim_command(
        node_id: Uuid,
        aggregate_id: Uuid,
        sequence: u64,
        generation: u64,
        payload: NodeCommandPayload,
    ) -> NodeCommandEnvelope {
        assert_eq!(payload.generation(), generation);
        let issued_at = Utc::now();
        NodeCommandEnvelope::new(
            NodeCommandMetadata {
                command_id: Uuid::now_v7(),
                lease_id: Uuid::now_v7(),
                node_id,
                sequence,
                aggregate_id,
                issued_at,
                not_after: issued_at + Duration::minutes(1),
                correlation_id: Uuid::now_v7(),
            },
            payload,
        )
        .expect("resource claim command")
    }

    fn claim_inventory(node_id: Uuid, agent_instance_id: Uuid) -> NodeResourceInventory {
        NodeResourceInventory::new(
            node_id,
            agent_instance_id,
            1,
            Utc::now(),
            vec![
                NodeResourceSlot::new(
                    ResourceKind::Cpu,
                    "cpu/shared",
                    ResourceAllocation::Scalar {
                        amount: 1_000,
                        unit: ResourceUnit::MilliCpu,
                    },
                )
                .expect("CPU inventory"),
                NodeResourceSlot::new(
                    ResourceKind::Memory,
                    "memory/system",
                    ResourceAllocation::Scalar {
                        amount: 2 * 1024 * 1024,
                        unit: ResourceUnit::Byte,
                    },
                )
                .expect("memory inventory"),
            ],
        )
        .expect("resource inventory")
    }

    fn claim_binding(
        claim_id: Uuid,
        inventory: &NodeResourceInventory,
    ) -> NodeResourceClaimBinding {
        let spec = claim_runtime_spec();
        NodeResourceClaimBinding {
            schema: NodeResourceClaimBinding::SCHEMA.into(),
            claim_id,
            node_id: inventory.node_id,
            agent_instance_id: inventory.agent_instance_id,
            inventory_generation: inventory.generation,
            inventory_digest: inventory.digest.clone(),
            runtime_unit_id: spec.unit_id,
            runtime_generation: spec.generation,
            topology_digest: format!("sha256:{}", "b".repeat(64)),
            slots: vec![
                ResourceSlotBinding {
                    kind: ResourceKind::Cpu,
                    stable_resource_id: "cpu/shared".into(),
                    allocation: ResourceAllocation::Scalar {
                        amount: spec.resources.cpu_millis,
                        unit: ResourceUnit::MilliCpu,
                    },
                    slot_generation: 1,
                    fence_token: Uuid::now_v7(),
                },
                ResourceSlotBinding {
                    kind: ResourceKind::Memory,
                    stable_resource_id: "memory/system".into(),
                    allocation: ResourceAllocation::Scalar {
                        amount: spec.resources.memory_bytes,
                        unit: ResourceUnit::Byte,
                    },
                    slot_generation: 1,
                    fence_token: Uuid::now_v7(),
                },
            ],
        }
    }

    fn claim_runtime_spec() -> RuntimeUnitSpec {
        RuntimeUnitSpec {
            schema: RuntimeUnitSpec::SCHEMA.into(),
            unit_id: "service-resource-bound".into(),
            generation: 1,
            class: RuntimeUnitClass::Service,
            artifact: ArtifactRef {
                uri: format!("oci://registry.example/service@sha256:{}", "a".repeat(64)),
                digest: format!("sha256:{}", "a".repeat(64)),
                media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            },
            process: RuntimeProcessSpec {
                command: vec!["/bin/service".into()],
                args: Vec::new(),
                working_directory: None,
                environment: BTreeMap::new(),
            },
            mounts: Vec::new(),
            secrets: Vec::new(),
            network: RuntimeNetworkSpec {
                mode: NetworkMode::None,
                ports: Vec::new(),
            },
            resources: ResourceLimits {
                cpu_millis: 500,
                memory_bytes: 1024 * 1024,
                pids: 32,
                ephemeral_storage_bytes: None,
                execution_timeout_ms: None,
            },
            isolation: IsolationLevel::Container,
            health: None,
            restart: RestartPolicy::Always,
            outputs: Vec::new(),
            semantics_profile_digest: None,
        }
    }

    fn claim_observation(spec: &RuntimeUnitSpec, state: RuntimeUnitState) -> RuntimeObservation {
        let spec_digest = spec.digest().expect("spec digest");
        RuntimeObservation {
            schema: RuntimeObservation::SCHEMA.into(),
            unit_id: spec.unit_id.clone(),
            generation: spec.generation,
            spec_digest: spec_digest.clone(),
            class: spec.class,
            state,
            provider_resource_id: Some("provider-resource".into()),
            provider_build: Some("provider-build".into()),
            observed_at_ms: 2_000,
            started_at_ms: Some(1_000),
            finished_at_ms: state.is_terminal().then_some(2_000),
            health: None,
            outputs: Vec::new(),
            usage: None,
            evidence: Some(RuntimeEvidence {
                provider_build: "provider-build".into(),
                spec_digest,
                semantics_profile_digest: None,
                claims: BTreeMap::new(),
            }),
            provider_attestation: None,
            failure: None,
        }
    }

    #[tokio::test]
    async fn resource_claim_evidence_uses_the_command_time_floor() {
        let directory = tempfile::tempdir().expect("resource claim journal");
        let node_id = Uuid::now_v7();
        let agent_instance_id = Uuid::now_v7();
        let claim_id = Uuid::now_v7();
        let inventory = claim_inventory(node_id, agent_instance_id);
        let binding = claim_binding(claim_id, &inventory);
        let authority: Arc<dyn NodeResourceInventoryAuthority> =
            Arc::new(FixedInventoryAuthority { inventory });
        let executor = CommandExecutor::new(
            FileCommandJournal::new(directory.path(), node_id).expect("journal"),
            Arc::new(ClaimRuntime {
                apply_calls: AtomicUsize::new(0),
                stop_not_found: AtomicBool::new(false),
            }),
            gateway(),
        )
        .with_resource_inventory(authority);
        let mut command = claim_command(
            node_id,
            claim_id,
            1,
            1,
            NodeCommandPayload::ResourceClaimPrepare {
                request: Box::new(NodeResourceClaimPrepare {
                    schema: NodeResourceClaimPrepare::SCHEMA.into(),
                    claim_generation: 1,
                    claim_digest: format!("sha256:{}", "c".repeat(64)),
                    binding,
                }),
            },
        );
        command.issued_at = Utc::now() + Duration::seconds(2);
        command.not_after = command.issued_at + Duration::minutes(1);

        let acknowledgement = executor.execute(command.clone()).await.expect("prepare");
        acknowledgement
            .validate_against(&command)
            .expect("future-issued acknowledgement");
        let NodeCommandOutcome::Succeeded { result } = acknowledgement.outcome else {
            panic!("resource Claim prepare must succeed");
        };
        let NodeCommandResult::ResourceClaimPrepared { prepared } = result.as_ref() else {
            panic!("resource Claim prepare returned the wrong result");
        };
        assert_eq!(prepared.prepared_at, command.issued_at);
        assert_eq!(acknowledgement.completed_at, command.issued_at);
    }

    #[test]
    fn completion_time_never_predates_resource_claim_evidence() {
        let node_id = Uuid::now_v7();
        let agent_instance_id = Uuid::now_v7();
        let claim_id = Uuid::now_v7();
        let inventory = claim_inventory(node_id, agent_instance_id);
        let mut command = claim_command(
            node_id,
            claim_id,
            1,
            1,
            NodeCommandPayload::ResourceClaimPrepare {
                request: Box::new(NodeResourceClaimPrepare {
                    schema: NodeResourceClaimPrepare::SCHEMA.into(),
                    claim_generation: 1,
                    claim_digest: format!("sha256:{}", "c".repeat(64)),
                    binding: claim_binding(claim_id, &inventory),
                }),
            },
        );
        command.issued_at = Utc::now() + Duration::seconds(30);
        command.not_after = command.issued_at + Duration::minutes(1);
        let evidence_at = command.issued_at + Duration::seconds(1);
        let NodeCommandPayload::ResourceClaimPrepare { request } = &command.payload else {
            panic!("test command must prepare a resource Claim");
        };
        let outcome = NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::ResourceClaimPrepared {
                prepared: NodeResourceClaimPrepared::new(request, evidence_at)
                    .expect("prepared evidence"),
            }),
        };

        assert_eq!(completion_timestamp(&command, &outcome), evidence_at);
    }

    #[tokio::test]
    async fn resource_claim_prepare_bind_and_release_are_restart_safe_and_fenced() {
        let directory = tempfile::tempdir().expect("resource claim journal");
        let node_id = Uuid::now_v7();
        let agent_instance_id = Uuid::now_v7();
        let claim_id = Uuid::now_v7();
        let workload_id = Uuid::now_v7();
        let inventory = claim_inventory(node_id, agent_instance_id);
        let binding = claim_binding(claim_id, &inventory);
        let runtime = Arc::new(ClaimRuntime {
            apply_calls: AtomicUsize::new(0),
            stop_not_found: AtomicBool::new(false),
        });
        let authority: Arc<dyn NodeResourceInventoryAuthority> =
            Arc::new(FixedInventoryAuthority {
                inventory: inventory.clone(),
            });
        let executor = CommandExecutor::new(
            FileCommandJournal::new(directory.path(), node_id).expect("journal"),
            runtime.clone(),
            gateway(),
        )
        .with_resource_inventory(authority.clone());

        let prepare_request = NodeResourceClaimPrepare {
            schema: NodeResourceClaimPrepare::SCHEMA.into(),
            claim_generation: 1,
            claim_digest: format!("sha256:{}", "c".repeat(64)),
            binding: binding.clone(),
        };
        let prepare = claim_command(
            node_id,
            claim_id,
            1,
            1,
            NodeCommandPayload::ResourceClaimPrepare {
                request: Box::new(prepare_request),
            },
        );
        let prepared = executor
            .execute(prepare.clone())
            .await
            .expect("prepare command");
        assert!(matches!(
            &prepared.outcome,
            NodeCommandOutcome::Succeeded {
                result
            } if matches!(
                result.as_ref(),
                NodeCommandResult::ResourceClaimPrepared { .. }
            )
        ));

        let mut replay = prepare;
        replay.lease_id = Uuid::now_v7();
        let reopened = CommandExecutor::new(
            FileCommandJournal::new(directory.path(), node_id).expect("reopened journal"),
            runtime.clone(),
            gateway(),
        )
        .with_resource_inventory(authority.clone());
        assert_eq!(
            reopened
                .execute(replay)
                .await
                .expect("prepare replay")
                .outcome,
            prepared.outcome
        );

        let spec = claim_runtime_spec();
        let apply = claim_command(
            node_id,
            workload_id,
            2,
            spec.generation,
            NodeCommandPayload::RuntimeApply {
                request: Box::new(RuntimeApplyRequest {
                    schema: RuntimeApplyRequest::SCHEMA.into(),
                    request_id: "claim-bound-apply".into(),
                    deadline_at_ms: None,
                    spec: spec.clone(),
                }),
                resource_claim: Some(Box::new(binding.clone())),
            },
        );
        let applied = reopened.execute(apply).await.expect("bound Runtime apply");
        let NodeCommandOutcome::Succeeded { result } = applied.outcome else {
            panic!("bound apply must succeed");
        };
        let NodeCommandResult::RuntimeApplied { observation } = result.as_ref() else {
            panic!("bound apply returned the wrong result");
        };
        binding
            .validate_runtime_observation(observation)
            .expect("Runtime allocation-binding evidence");
        assert_eq!(runtime.apply_calls.load(Ordering::SeqCst), 1);
        drop(reopened);
        let after_apply = CommandExecutor::new(
            FileCommandJournal::new(directory.path(), node_id).expect("journal after apply"),
            runtime.clone(),
            gateway(),
        )
        .with_resource_inventory(authority.clone());

        let release_before_stop = claim_command(
            node_id,
            claim_id,
            3,
            2,
            NodeCommandPayload::ResourceClaimRelease {
                request: Box::new(NodeResourceClaimRelease {
                    schema: NodeResourceClaimRelease::SCHEMA.into(),
                    claim_generation: 2,
                    claim_digest: format!("sha256:{}", "d".repeat(64)),
                    binding: binding.clone(),
                }),
            },
        );
        assert!(matches!(
            after_apply
                .execute(release_before_stop)
                .await
                .expect("fenced release rejection")
                .outcome,
            NodeCommandOutcome::Rejected { .. }
        ));

        runtime.stop_not_found.store(true, Ordering::SeqCst);
        let rejected_stop = claim_command(
            node_id,
            workload_id,
            4,
            spec.generation,
            NodeCommandPayload::RuntimeStop {
                request: RuntimeActionRequest {
                    schema: RuntimeActionRequest::SCHEMA.into(),
                    request_id: "claim-bound-rejected-stop".into(),
                    unit_id: spec.unit_id.clone(),
                    generation: spec.generation,
                    deadline_at_ms: None,
                },
            },
        );
        assert!(matches!(
            after_apply
                .execute(rejected_stop)
                .await
                .expect("rejected Runtime stop")
                .outcome,
            NodeCommandOutcome::Rejected { .. }
        ));
        let release_after_rejected_stop = claim_command(
            node_id,
            claim_id,
            5,
            3,
            NodeCommandPayload::ResourceClaimRelease {
                request: Box::new(NodeResourceClaimRelease {
                    schema: NodeResourceClaimRelease::SCHEMA.into(),
                    claim_generation: 3,
                    claim_digest: format!("sha256:{}", "e".repeat(64)),
                    binding: binding.clone(),
                }),
            },
        );
        assert!(matches!(
            after_apply
                .execute(release_after_rejected_stop)
                .await
                .expect("release after rejected stop")
                .outcome,
            NodeCommandOutcome::Rejected { .. }
        ));

        runtime.stop_not_found.store(false, Ordering::SeqCst);
        let stop = claim_command(
            node_id,
            workload_id,
            6,
            spec.generation,
            NodeCommandPayload::RuntimeStop {
                request: RuntimeActionRequest {
                    schema: RuntimeActionRequest::SCHEMA.into(),
                    request_id: "claim-bound-stop".into(),
                    unit_id: spec.unit_id.clone(),
                    generation: spec.generation,
                    deadline_at_ms: None,
                },
            },
        );
        assert!(matches!(
            after_apply
                .execute(stop)
                .await
                .expect("Runtime stop")
                .outcome,
            NodeCommandOutcome::Succeeded { .. }
        ));
        drop(after_apply);
        let after_stop = CommandExecutor::new(
            FileCommandJournal::new(directory.path(), node_id).expect("journal after stop"),
            runtime,
            gateway(),
        )
        .with_resource_inventory(authority);

        let release = claim_command(
            node_id,
            claim_id,
            7,
            4,
            NodeCommandPayload::ResourceClaimRelease {
                request: Box::new(NodeResourceClaimRelease {
                    schema: NodeResourceClaimRelease::SCHEMA.into(),
                    claim_generation: 4,
                    claim_digest: format!("sha256:{}", "f".repeat(64)),
                    binding,
                }),
            },
        );
        assert!(matches!(
            after_stop
                .execute(release)
                .await
                .expect("resource claim release")
                .outcome,
            NodeCommandOutcome::Succeeded {
                result
            } if matches!(
                result.as_ref(),
                NodeCommandResult::ResourceClaimReleased { .. }
            )
        ));
        drop(after_stop);
        let after_release =
            FileCommandJournal::new(directory.path(), node_id).expect("journal after release");
        assert!(after_release
            .active_resource_claim_bindings()
            .await
            .expect("active claims")
            .is_empty());
    }

    #[tokio::test]
    async fn completed_command_replay_does_not_call_runtime_twice() {
        let directory = tempfile::tempdir().expect("journal directory");
        let node_id = Uuid::now_v7();
        let command_id = Uuid::now_v7();
        let runtime = Arc::new(InspectRuntime {
            calls: AtomicUsize::new(0),
            error: false,
        });
        let executor = CommandExecutor::new(
            FileCommandJournal::new(directory.path(), node_id).expect("journal"),
            runtime.clone(),
            gateway(),
        );
        let first = command(
            node_id,
            command_id,
            Uuid::now_v7(),
            Utc::now() + Duration::minutes(1),
        );
        let first_ack = executor.execute(first.clone()).await.expect("execute");
        let mut redelivered = first;
        redelivered.lease_id = Uuid::now_v7();
        let replayed = executor.execute(redelivered).await.expect("replay");
        assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
        assert_eq!(first_ack.outcome, replayed.outcome);
        assert_ne!(first_ack.lease_id, replayed.lease_id);
    }

    #[tokio::test]
    async fn expired_commands_do_not_reach_runtime_and_provider_errors_are_retryable() {
        let expired_directory = tempfile::tempdir().expect("expired journal directory");
        let expired_node = Uuid::now_v7();
        let runtime = Arc::new(InspectRuntime {
            calls: AtomicUsize::new(0),
            error: false,
        });
        let expired_executor = CommandExecutor::new(
            FileCommandJournal::new(expired_directory.path(), expired_node).expect("journal"),
            runtime.clone(),
            gateway(),
        );
        let expired = expired_executor
            .execute(command(
                expired_node,
                Uuid::now_v7(),
                Uuid::now_v7(),
                Utc::now() - Duration::milliseconds(1),
            ))
            .await
            .expect("expired acknowledgement");
        assert!(matches!(
            expired.outcome,
            NodeCommandOutcome::Rejected { .. }
        ));
        assert_eq!(runtime.calls.load(Ordering::SeqCst), 0);

        let failure_directory = tempfile::tempdir().expect("failure journal directory");
        let failure_node = Uuid::now_v7();
        let failing_runtime = Arc::new(InspectRuntime {
            calls: AtomicUsize::new(0),
            error: true,
        });
        let failure_executor = CommandExecutor::new(
            FileCommandJournal::new(failure_directory.path(), failure_node).expect("journal"),
            failing_runtime,
            gateway(),
        );
        let failed = failure_executor
            .execute(command(
                failure_node,
                Uuid::now_v7(),
                Uuid::now_v7(),
                Utc::now() + Duration::minutes(1),
            ))
            .await
            .expect("failure acknowledgement");
        assert!(matches!(
            failed.outcome,
            NodeCommandOutcome::Failed {
                failure: NodeCommandFailure {
                    retryable: true,
                    ..
                }
            }
        ));
    }

    #[tokio::test]
    async fn gateway_install_returns_an_exact_revision_acknowledgement() {
        let directory = tempfile::tempdir().expect("journal directory");
        let node_id = Uuid::now_v7();
        let issued_at = Utc::now() - Duration::seconds(1);
        let not_after = issued_at + Duration::minutes(1);
        let snapshot = GatewaySnapshot::new(
            node_id,
            3,
            Some(2),
            issued_at,
            not_after,
            "management { enabled = true }\n",
        )
        .expect("Gateway snapshot");
        let envelope = NodeCommandEnvelope::new(
            NodeCommandMetadata {
                command_id: Uuid::now_v7(),
                lease_id: Uuid::now_v7(),
                node_id,
                sequence: 1,
                aggregate_id: Uuid::now_v7(),
                issued_at,
                not_after,
                correlation_id: Uuid::now_v7(),
            },
            NodeCommandPayload::GatewaySnapshotInstall {
                snapshot: Box::new(snapshot.clone()),
            },
        )
        .expect("Gateway command");
        let gateway = gateway();
        let executor = CommandExecutor::new(
            FileCommandJournal::new(directory.path(), node_id).expect("journal"),
            Arc::new(InspectRuntime {
                calls: AtomicUsize::new(0),
                error: false,
            }),
            gateway.clone(),
        );
        let acknowledgement = executor
            .execute(envelope.clone())
            .await
            .expect("execute Gateway command");
        let NodeCommandOutcome::Succeeded { result } = &acknowledgement.outcome else {
            panic!("Gateway install must produce a result");
        };
        let NodeCommandResult::GatewaySnapshotInstalled { acknowledgement } = result.as_ref()
        else {
            panic!("Gateway install returned the wrong result kind");
        };
        acknowledgement
            .validate_for(envelope.command_id, node_id, &snapshot)
            .expect("exact Gateway acknowledgement");
        assert_eq!(gateway.calls.load(Ordering::SeqCst), 1);
    }
}
