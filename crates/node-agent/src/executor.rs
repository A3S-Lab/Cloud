use crate::box_build::{NodeBoxBuildError, NodeBoxBuildExecutor};
use crate::code_harness::{self, CodeHarnessError, SharedCodeHarnessTransport};
use crate::plugin_host;
use crate::resource_claim::{self, ResourceClaimExecutionError};
use crate::{
    CommandJournalError, FileCommandJournal, GatewaySnapshotInstallError,
    GatewaySnapshotInstallOutcome, GatewaySnapshotInstaller, JournalDecision, NodeArtifactError,
    NodeArtifactManager, NodeResourceInventoryAuthority,
};
use a3s_cloud_contracts::{
    GatewayAckState, NodeCommandAck, NodeCommandEnvelope, NodeCommandFailure, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult, NodeGatewayAck, NodeGatewaySnapshotObservation,
    NodeResourceClaimPrepared, NodeResourceClaimReleased,
};
use a3s_runtime::contract::RuntimeInspection;
use a3s_runtime::{RuntimeClient, RuntimeError};
use a3s_use_core::{PluginHostManager, UseError};
use chrono::{DateTime, Utc};
use std::sync::Arc;

pub struct CommandExecutor {
    journal: FileCommandJournal,
    runtime: Arc<dyn RuntimeClient>,
    gateway: Arc<dyn GatewaySnapshotInstaller>,
    artifacts: Option<Arc<NodeArtifactManager>>,
    box_build: Option<Arc<dyn NodeBoxBuildExecutor>>,
    resource_inventory: Option<Arc<dyn NodeResourceInventoryAuthority>>,
    plugin_host: Option<Arc<dyn PluginHostManager>>,
    code_harness: Option<SharedCodeHarnessTransport>,
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
            box_build: None,
            resource_inventory: None,
            plugin_host: None,
            code_harness: None,
        }
    }

    pub fn with_artifacts(mut self, artifacts: Arc<NodeArtifactManager>) -> Self {
        self.artifacts = Some(artifacts);
        self
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn with_box_build(mut self, box_build: Arc<dyn NodeBoxBuildExecutor>) -> Self {
        self.box_build = Some(box_build);
        self
    }

    pub fn with_resource_inventory(
        mut self,
        resource_inventory: Arc<dyn NodeResourceInventoryAuthority>,
    ) -> Self {
        self.resource_inventory = Some(resource_inventory);
        self
    }

    pub fn with_plugin_host(mut self, plugin_host: Arc<dyn PluginHostManager>) -> Self {
        self.plugin_host = Some(plugin_host);
        self
    }

    pub(crate) fn with_code_harness(mut self, code_harness: SharedCodeHarnessTransport) -> Self {
        self.code_harness = Some(code_harness);
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
            NodeCommandPayload::CodeAgentCommand { binding, command } => {
                binding
                    .validate_command(command)
                    .map_err(CodeHarnessError::Invalid)?;
                let endpoint =
                    code_harness::resolve_runtime_endpoint(self.runtime.as_ref(), binding).await?;
                let transport = self.code_harness.as_deref().ok_or_else(|| {
                    CodeHarnessError::Unavailable(
                        "the node-local A3S Code Harness transport is not configured".into(),
                    )
                })?;
                let timeout = envelope
                    .not_after
                    .signed_duration_since(Utc::now())
                    .to_std()
                    .map_err(|_| {
                        CodeHarnessError::Invalid(
                            "node command deadline elapsed before Harness dispatch".into(),
                        )
                    })?;
                let receipt = transport.send_command(&endpoint, command, timeout).await?;
                Ok(NodeCommandResult::CodeAgentCommandAccepted {
                    receipt: Box::new(receipt),
                })
            }
            NodeCommandPayload::BoxBuildStart { request } => {
                let started = self.box_build()?.start(envelope, request).await?;
                Ok(NodeCommandResult::BoxBuildStarted { started })
            }
            NodeCommandPayload::BoxBuildInspect { request } => {
                let inspection = self.box_build()?.inspect(envelope, request).await?;
                Ok(NodeCommandResult::BoxBuildInspected {
                    inspection: Box::new(inspection),
                })
            }
            NodeCommandPayload::BoxBuildCancel { request } => {
                let cancelled = self.box_build()?.cancel(envelope, request).await?;
                Ok(NodeCommandResult::BoxBuildCancelled { cancelled })
            }
            NodeCommandPayload::BoxBuildRemove { request } => {
                let removed = self.box_build()?.remove(envelope, request).await?;
                Ok(NodeCommandResult::BoxBuildRemoved { removed })
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
            NodeCommandPayload::GatewaySnapshotObserve { request } => {
                let observed = self.gateway.observe(request).await?;
                let observation = NodeGatewaySnapshotObservation {
                    schema: NodeGatewaySnapshotObservation::SCHEMA.into(),
                    observation_id: uuid::Uuid::now_v7(),
                    command_id: envelope.command_id,
                    node_id: envelope.node_id,
                    gateway_id: request.gateway_id,
                    revision: request.revision,
                    snapshot_digest: request.snapshot_digest.clone(),
                    state: observed.state,
                    ready: observed.ready,
                    applied: observed.applied,
                    observed_at: observed.observed_at.max(envelope.issued_at),
                    management_protocol: observed.protocol,
                };
                observation
                    .validate_for(envelope.command_id, envelope.node_id, request)
                    .map_err(|error| {
                        DispatchError::Gateway(GatewaySnapshotInstallError::Protocol(error))
                    })?;
                Ok(NodeCommandResult::GatewaySnapshotObserved { observation })
            }
            NodeCommandPayload::PluginHostCapabilitiesInspect { .. } => {
                let capabilities = plugin_host::inspect(self.plugin_host()?).await?;
                Ok(NodeCommandResult::PluginHostCapabilitiesInspected { capabilities })
            }
            NodeCommandPayload::PluginHostPlan { request } => {
                let (capabilities, plan) = plugin_host::plan(self.plugin_host()?, request).await?;
                Ok(NodeCommandResult::PluginHostPlanned {
                    capabilities,
                    plan: Box::new(plan),
                })
            }
            NodeCommandPayload::PluginHostApply { request } => {
                let (capabilities, applied) =
                    plugin_host::apply(self.plugin_host()?, request).await?;
                Ok(NodeCommandResult::PluginHostApplied {
                    capabilities,
                    applied: Box::new(applied),
                })
            }
            NodeCommandPayload::PluginHostSetEnablement { request } => {
                let (capabilities, enablement) =
                    plugin_host::set_enablement(self.plugin_host()?, request).await?;
                Ok(NodeCommandResult::PluginHostEnablementSet {
                    capabilities,
                    enablement: Box::new(enablement),
                })
            }
            NodeCommandPayload::PluginHostObserve { request } => {
                let (capabilities, observation) =
                    plugin_host::observe(self.plugin_host()?, request).await?;
                Ok(NodeCommandResult::PluginHostObserved {
                    capabilities,
                    observation: Box::new(observation),
                })
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

    fn box_build(&self) -> Result<&Arc<dyn NodeBoxBuildExecutor>, NodeBoxBuildError> {
        self.box_build.as_ref().ok_or_else(|| {
            NodeBoxBuildError::State(
                "the sole Box build command adapter is not configured on this node".into(),
            )
        })
    }

    fn plugin_host(&self) -> Result<&dyn PluginHostManager, DispatchError> {
        self.plugin_host
            .as_deref()
            .ok_or(DispatchError::PluginHostUnavailable)
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
            NodeCommandResult::GatewaySnapshotObserved { observation } => {
                Some(observation.observed_at)
            }
            NodeCommandResult::CodeAgentCommandAccepted { receipt } => {
                i64::try_from(receipt.observed_at_ms)
                    .ok()
                    .and_then(DateTime::from_timestamp_millis)
            }
            NodeCommandResult::RuntimeApplied { .. }
            | NodeCommandResult::RuntimeInspected { .. }
            | NodeCommandResult::RuntimeStopped { .. }
            | NodeCommandResult::RuntimeRemoved { .. }
            | NodeCommandResult::BoxBuildStarted { .. }
            | NodeCommandResult::BoxBuildInspected { .. }
            | NodeCommandResult::BoxBuildCancelled { .. }
            | NodeCommandResult::BoxBuildRemoved { .. }
            | NodeCommandResult::PluginHostCapabilitiesInspected { .. }
            | NodeCommandResult::PluginHostPlanned { .. }
            | NodeCommandResult::PluginHostApplied { .. }
            | NodeCommandResult::PluginHostEnablementSet { .. }
            | NodeCommandResult::PluginHostObserved { .. } => None,
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
    BoxBuild(NodeBoxBuildError),
    PluginHost(UseError),
    PluginHostUnavailable,
    CodeHarness(CodeHarnessError),
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

impl From<NodeBoxBuildError> for DispatchError {
    fn from(error: NodeBoxBuildError) -> Self {
        Self::BoxBuild(error)
    }
}

impl From<CommandJournalError> for DispatchError {
    fn from(error: CommandJournalError) -> Self {
        Self::ResourceClaim(error.into())
    }
}

impl From<UseError> for DispatchError {
    fn from(error: UseError) -> Self {
        Self::PluginHost(error)
    }
}

impl From<CodeHarnessError> for DispatchError {
    fn from(error: CodeHarnessError) -> Self {
        Self::CodeHarness(error)
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
        DispatchError::BoxBuild(error) => {
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
        DispatchError::PluginHost(error) => NodeCommandOutcome::Failed {
            failure: NodeCommandFailure {
                code: sanitize_plugin_host_error_code(&error.code),
                message: sanitize_error(&error.message),
                retryable: false,
            },
        },
        DispatchError::PluginHostUnavailable => rejected(
            "plugin_host_unavailable",
            "A3S Use Plugin Manager is not configured on this node",
        ),
        DispatchError::CodeHarness(error) => {
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

fn sanitize_plugin_host_error_code(code: &str) -> String {
    if !code.is_empty()
        && code.len() <= 127
        && code.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
    {
        code.to_owned()
    } else {
        "plugin_host_operation_failed".into()
    }
}

#[cfg(test)]
#[path = "executor_tests.rs"]
mod tests;
