use a3s_code_core::{AgentProtocolCommandReceiptV1, AgentProtocolCommandV1};
use a3s_runtime::contract::{
    RuntimeActionRequest, RuntimeApplyRequest, RuntimeInspection, RuntimeObservation,
    RuntimeRemoval,
};
use a3s_use_core::{
    PluginHostApplyRequest, PluginHostApplyResult, PluginHostCapabilities,
    PluginHostEnablementPlanRequest, PluginHostEnablementPlanResult, PluginHostObservationRequest,
    PluginHostObservationResult, PluginHostPlanRequest, PluginHostPlanResult,
    PLUGIN_HOST_APPLY_REQUEST_SCHEMA, PLUGIN_HOST_ENABLEMENT_PLAN_REQUEST_SCHEMA,
    PLUGIN_HOST_OBSERVATION_REQUEST_SCHEMA, PLUGIN_HOST_PLAN_REQUEST_SCHEMA,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
    validate_sha256, validate_single_line, validate_uuid, GatewaySnapshot,
    GatewaySnapshotObservationRequest, NodeBoxBuildCancelResult, NodeBoxBuildInspection,
    NodeBoxBuildRemoveResult, NodeBoxBuildRequest, NodeBoxBuildStartResult,
    NodeCodeAgentRuntimeBindingV1, NodeGatewayAck, NodeGatewaySnapshotObservation,
    NodePluginHostCapabilitiesRequest, NodeResourceClaimBinding, NodeResourceClaimPrepare,
    NodeResourceClaimPrepared, NodeResourceClaimRelease, NodeResourceClaimReleased,
    NODE_CODE_AGENT_COMMAND_SCHEMA_V1,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NodeCommandPayload {
    ResourceClaimPrepare {
        request: Box<NodeResourceClaimPrepare>,
    },
    RuntimeApply {
        request: Box<RuntimeApplyRequest>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resource_claim: Option<Box<NodeResourceClaimBinding>>,
    },
    RuntimeInspect {
        unit_id: String,
        generation: u64,
    },
    RuntimeStop {
        request: RuntimeActionRequest,
    },
    RuntimeRemove {
        request: RuntimeActionRequest,
    },
    CodeAgentCommand {
        binding: Box<NodeCodeAgentRuntimeBindingV1>,
        command: Box<AgentProtocolCommandV1>,
    },
    BoxBuildStart {
        request: Box<NodeBoxBuildRequest>,
    },
    BoxBuildInspect {
        request: Box<NodeBoxBuildRequest>,
    },
    BoxBuildCancel {
        request: Box<NodeBoxBuildRequest>,
    },
    BoxBuildRemove {
        request: Box<NodeBoxBuildRequest>,
    },
    ResourceClaimRelease {
        request: Box<NodeResourceClaimRelease>,
    },
    GatewaySnapshotInstall {
        snapshot: Box<GatewaySnapshot>,
    },
    GatewaySnapshotObserve {
        request: GatewaySnapshotObservationRequest,
    },
    PluginHostCapabilitiesInspect {
        request: NodePluginHostCapabilitiesRequest,
    },
    PluginHostPlan {
        request: Box<PluginHostPlanRequest>,
    },
    PluginHostApply {
        request: Box<PluginHostApplyRequest>,
    },
    PluginHostPlanEnablement {
        request: Box<PluginHostEnablementPlanRequest>,
    },
    PluginHostObserve {
        request: Box<PluginHostObservationRequest>,
    },
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
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ResourceClaimPrepare { .. } => "resource_claim_prepare",
            Self::RuntimeApply { .. } => "runtime_apply",
            Self::RuntimeInspect { .. } => "runtime_inspect",
            Self::RuntimeStop { .. } => "runtime_stop",
            Self::RuntimeRemove { .. } => "runtime_remove",
            Self::CodeAgentCommand { .. } => "code_agent_command",
            Self::BoxBuildStart { .. } => "box_build_start",
            Self::BoxBuildInspect { .. } => "box_build_inspect",
            Self::BoxBuildCancel { .. } => "box_build_cancel",
            Self::BoxBuildRemove { .. } => "box_build_remove",
            Self::ResourceClaimRelease { .. } => "resource_claim_release",
            Self::GatewaySnapshotInstall { .. } => "gateway_snapshot_install",
            Self::GatewaySnapshotObserve { .. } => "gateway_snapshot_observe",
            Self::PluginHostCapabilitiesInspect { .. } => "plugin_host_capabilities_inspect",
            Self::PluginHostPlan { .. } => "plugin_host_plan",
            Self::PluginHostApply { .. } => "plugin_host_apply",
            Self::PluginHostPlanEnablement { .. } => "plugin_host_plan_enablement",
            Self::PluginHostObserve { .. } => "plugin_host_observe",
        }
    }

    pub fn schema(&self) -> &'static str {
        match self {
            Self::ResourceClaimPrepare { .. } => NodeResourceClaimPrepare::SCHEMA,
            Self::RuntimeApply {
                resource_claim: Some(_),
                ..
            } => "a3s.cloud.runtime-resource-bound-apply.v1",
            Self::RuntimeApply {
                resource_claim: None,
                ..
            } => RuntimeApplyRequest::SCHEMA,
            Self::RuntimeInspect { .. } => "a3s.runtime.inspect-request.v1",
            Self::RuntimeStop { .. } => "a3s.runtime.stop-request.v1",
            Self::RuntimeRemove { .. } => "a3s.runtime.remove-request.v1",
            Self::CodeAgentCommand { .. } => NODE_CODE_AGENT_COMMAND_SCHEMA_V1,
            Self::BoxBuildStart { .. } => "a3s.cloud.box-build-start.v1",
            Self::BoxBuildInspect { .. } => "a3s.cloud.box-build-inspect.v1",
            Self::BoxBuildCancel { .. } => "a3s.cloud.box-build-cancel.v1",
            Self::BoxBuildRemove { .. } => "a3s.cloud.box-build-remove.v1",
            Self::ResourceClaimRelease { .. } => NodeResourceClaimRelease::SCHEMA,
            Self::GatewaySnapshotInstall { .. } => GatewaySnapshot::SCHEMA,
            Self::GatewaySnapshotObserve { .. } => GatewaySnapshotObservationRequest::SCHEMA,
            Self::PluginHostCapabilitiesInspect { .. } => NodePluginHostCapabilitiesRequest::SCHEMA,
            Self::PluginHostPlan { .. } => PLUGIN_HOST_PLAN_REQUEST_SCHEMA,
            Self::PluginHostApply { .. } => PLUGIN_HOST_APPLY_REQUEST_SCHEMA,
            Self::PluginHostPlanEnablement { .. } => PLUGIN_HOST_ENABLEMENT_PLAN_REQUEST_SCHEMA,
            Self::PluginHostObserve { .. } => PLUGIN_HOST_OBSERVATION_REQUEST_SCHEMA,
        }
    }

    pub fn generation(&self) -> u64 {
        match self {
            Self::ResourceClaimPrepare { request } => request.claim_generation,
            Self::RuntimeApply { request, .. } => request.spec.generation,
            Self::RuntimeInspect { generation, .. } => *generation,
            Self::RuntimeStop { request } | Self::RuntimeRemove { request } => request.generation,
            Self::CodeAgentCommand { binding, .. } => binding.runtime_generation,
            Self::BoxBuildStart { request }
            | Self::BoxBuildInspect { request }
            | Self::BoxBuildCancel { request }
            | Self::BoxBuildRemove { request } => request.generation,
            Self::ResourceClaimRelease { request } => request.claim_generation,
            Self::GatewaySnapshotInstall { snapshot } => snapshot.revision,
            Self::GatewaySnapshotObserve { request } => request.revision,
            Self::PluginHostCapabilitiesInspect { request } => request.generation,
            Self::PluginHostPlan { request } => request.assignment_generation,
            Self::PluginHostApply { request } => request.assignment_generation,
            Self::PluginHostPlanEnablement { request } => request.assignment_generation,
            Self::PluginHostObserve { request } => request.assignment_generation,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::ResourceClaimPrepare { request } => request.validate(),
            Self::RuntimeApply {
                request,
                resource_claim,
            } => {
                request.validate()?;
                if let Some(binding) = resource_claim {
                    binding.validate_runtime_spec(&request.spec)?;
                }
                Ok(())
            }
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
            Self::CodeAgentCommand { binding, command } => binding.validate_command(command),
            Self::BoxBuildStart { request }
            | Self::BoxBuildInspect { request }
            | Self::BoxBuildCancel { request }
            | Self::BoxBuildRemove { request } => request.validate(),
            Self::ResourceClaimRelease { request } => request.validate(),
            Self::GatewaySnapshotInstall { snapshot } => snapshot.validate(),
            Self::GatewaySnapshotObserve { request } => request.validate(),
            Self::PluginHostCapabilitiesInspect { request } => request.validate(),
            Self::PluginHostPlan { request } => request.validate().map_err(|error| {
                format!("invalid A3S Use Plugin Host plan request ({})", error.code)
            }),
            Self::PluginHostApply { request } => request.validate().map_err(|error| {
                format!("invalid A3S Use Plugin Host apply request ({})", error.code)
            }),
            Self::PluginHostPlanEnablement { request } => request.validate().map_err(|error| {
                format!(
                    "invalid A3S Use Plugin Host enablement plan request ({})",
                    error.code
                )
            }),
            Self::PluginHostObserve { request } => request.validate().map_err(|error| {
                format!(
                    "invalid A3S Use Plugin Host observation request ({})",
                    error.code
                )
            }),
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
        match &self.payload {
            NodeCommandPayload::GatewaySnapshotInstall { snapshot } => {
                if snapshot.gateway_id != self.node_id
                    || snapshot.issued_at != self.issued_at
                    || snapshot.expires_at < self.not_after
                {
                    return Err(
                        "Gateway snapshot identity and validity must contain its node command"
                            .into(),
                    );
                }
            }
            NodeCommandPayload::GatewaySnapshotObserve { request }
                if request.gateway_id != self.node_id =>
            {
                return Err("Gateway snapshot observation targets another command node".into());
            }
            _ => {}
        }
        match &self.payload {
            NodeCommandPayload::ResourceClaimPrepare { request } => {
                if self.node_id != request.binding.node_id
                    || self.aggregate_id != request.binding.claim_id
                {
                    return Err(
                        "resource claim prepare command identity does not match its binding".into(),
                    );
                }
            }
            NodeCommandPayload::RuntimeApply {
                resource_claim: Some(binding),
                ..
            } => {
                if self.node_id != binding.node_id {
                    return Err(
                        "Runtime resource binding belongs to a different command node".into(),
                    );
                }
            }
            NodeCommandPayload::ResourceClaimRelease { request } => {
                if self.node_id != request.binding.node_id
                    || self.aggregate_id != request.binding.claim_id
                {
                    return Err(
                        "resource claim release command identity does not match its binding".into(),
                    );
                }
            }
            NodeCommandPayload::CodeAgentCommand { binding, .. }
                if self.aggregate_id != binding.execution_id =>
            {
                return Err("Code Agent command aggregate does not match its execution".into());
            }
            NodeCommandPayload::RuntimeApply {
                resource_claim: None,
                ..
            }
            | NodeCommandPayload::RuntimeInspect { .. }
            | NodeCommandPayload::RuntimeStop { .. }
            | NodeCommandPayload::RuntimeRemove { .. }
            | NodeCommandPayload::CodeAgentCommand { .. }
            | NodeCommandPayload::BoxBuildStart { .. }
            | NodeCommandPayload::BoxBuildInspect { .. }
            | NodeCommandPayload::BoxBuildCancel { .. }
            | NodeCommandPayload::BoxBuildRemove { .. }
            | NodeCommandPayload::GatewaySnapshotInstall { .. }
            | NodeCommandPayload::GatewaySnapshotObserve { .. }
            | NodeCommandPayload::PluginHostCapabilitiesInspect { .. }
            | NodeCommandPayload::PluginHostPlan { .. }
            | NodeCommandPayload::PluginHostApply { .. }
            | NodeCommandPayload::PluginHostPlanEnablement { .. }
            | NodeCommandPayload::PluginHostObserve { .. } => {}
        }
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
    ResourceClaimPrepared {
        prepared: NodeResourceClaimPrepared,
    },
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
    CodeAgentCommandAccepted {
        receipt: Box<AgentProtocolCommandReceiptV1>,
    },
    BoxBuildStarted {
        started: NodeBoxBuildStartResult,
    },
    BoxBuildInspected {
        inspection: Box<NodeBoxBuildInspection>,
    },
    BoxBuildCancelled {
        cancelled: NodeBoxBuildCancelResult,
    },
    BoxBuildRemoved {
        removed: NodeBoxBuildRemoveResult,
    },
    ResourceClaimReleased {
        released: NodeResourceClaimReleased,
    },
    GatewaySnapshotInstalled {
        acknowledgement: NodeGatewayAck,
    },
    GatewaySnapshotObserved {
        observation: NodeGatewaySnapshotObservation,
    },
    PluginHostCapabilitiesInspected {
        capabilities: PluginHostCapabilities,
    },
    PluginHostPlanned {
        capabilities: PluginHostCapabilities,
        plan: Box<PluginHostPlanResult>,
    },
    PluginHostApplied {
        capabilities: PluginHostCapabilities,
        applied: Box<PluginHostApplyResult>,
    },
    PluginHostEnablementPlanned {
        capabilities: PluginHostCapabilities,
        enablement_plan: Box<PluginHostEnablementPlanResult>,
    },
    PluginHostObserved {
        capabilities: PluginHostCapabilities,
        observation: Box<PluginHostObservationResult>,
    },
}

impl NodeCommandResult {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::ResourceClaimPrepared { prepared } => prepared.validate(),
            Self::RuntimeApplied { observation } => observation.validate(),
            Self::RuntimeInspected { inspection } | Self::RuntimeStopped { inspection } => {
                inspection.validate()
            }
            Self::RuntimeRemoved { removal } => removal.validate(),
            Self::CodeAgentCommandAccepted { receipt } => receipt
                .validate()
                .map_err(|error| format!("invalid A3S Code command receipt ({})", error.code())),
            Self::BoxBuildStarted { started } => started.phase.validate(),
            Self::BoxBuildInspected { .. }
            | Self::BoxBuildCancelled { .. }
            | Self::BoxBuildRemoved { .. } => Ok(()),
            Self::ResourceClaimReleased { released } => released.validate(),
            Self::GatewaySnapshotInstalled { acknowledgement } => acknowledgement.validate(),
            Self::GatewaySnapshotObserved { observation } => observation.validate(),
            Self::PluginHostCapabilitiesInspected { capabilities } => capabilities
                .validate()
                .map_err(|error| format!("invalid Plugin Host capabilities ({})", error.code)),
            Self::PluginHostPlanned { capabilities, plan } => {
                capabilities.validate().map_err(|error| {
                    format!("invalid Plugin Host capabilities ({})", error.code)
                })?;
                plan.validate()
                    .map_err(|error| format!("invalid Plugin Host plan result ({})", error.code))
            }
            Self::PluginHostApplied {
                capabilities,
                applied,
            } => {
                capabilities.validate().map_err(|error| {
                    format!("invalid Plugin Host capabilities ({})", error.code)
                })?;
                applied
                    .validate()
                    .map_err(|error| format!("invalid Plugin Host apply result ({})", error.code))
            }
            Self::PluginHostEnablementPlanned {
                capabilities,
                enablement_plan,
            } => {
                capabilities.validate().map_err(|error| {
                    format!("invalid Plugin Host capabilities ({})", error.code)
                })?;
                enablement_plan.validate().map_err(|error| {
                    format!(
                        "invalid Plugin Host enablement plan result ({})",
                        error.code
                    )
                })
            }
            Self::PluginHostObserved {
                capabilities,
                observation,
            } => {
                capabilities.validate().map_err(|error| {
                    format!("invalid Plugin Host capabilities ({})", error.code)
                })?;
                observation.validate().map_err(|error| {
                    format!("invalid Plugin Host observation result ({})", error.code)
                })
            }
        }
    }

    fn validate_against(&self, command: &NodeCommandEnvelope) -> Result<(), String> {
        self.validate()?;
        match (&command.payload, self) {
            (
                NodeCommandPayload::ResourceClaimPrepare { request },
                Self::ResourceClaimPrepared { prepared },
            ) => prepared.validate_for(request),
            (
                NodeCommandPayload::RuntimeApply {
                    request,
                    resource_claim,
                },
                Self::RuntimeApplied { observation },
            ) => {
                observation.validate_against(&request.spec)?;
                if let Some(binding) = resource_claim {
                    binding.validate_runtime_observation(observation)?;
                }
                Ok(())
            }
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
                NodeCommandPayload::CodeAgentCommand { binding, command },
                Self::CodeAgentCommandAccepted { receipt },
            ) => {
                binding.validate_command(command)?;
                receipt.validate_for(command).map_err(|error| {
                    format!("A3S Code command receipt does not match ({})", error.code())
                })
            }
            (NodeCommandPayload::BoxBuildStart { request }, Self::BoxBuildStarted { started }) => {
                started.validate_for(request)
            }
            (
                NodeCommandPayload::BoxBuildInspect { request },
                Self::BoxBuildInspected { inspection },
            ) => inspection.validate_for(request),
            (
                NodeCommandPayload::BoxBuildCancel { request },
                Self::BoxBuildCancelled { cancelled },
            ) => cancelled.validate_for(request),
            (NodeCommandPayload::BoxBuildRemove { request }, Self::BoxBuildRemoved { removed }) => {
                removed.validate_for(request)
            }
            (
                NodeCommandPayload::ResourceClaimRelease { request },
                Self::ResourceClaimReleased { released },
            ) => released.validate_for(request),
            (
                NodeCommandPayload::GatewaySnapshotInstall { snapshot },
                Self::GatewaySnapshotInstalled { acknowledgement },
            ) => acknowledgement.validate_for(command.command_id, command.node_id, snapshot),
            (
                NodeCommandPayload::GatewaySnapshotObserve { request },
                Self::GatewaySnapshotObserved { observation },
            ) => observation.validate_for(command.command_id, command.node_id, request),
            (
                NodeCommandPayload::PluginHostCapabilitiesInspect { .. },
                Self::PluginHostCapabilitiesInspected { capabilities },
            ) => capabilities
                .validate()
                .map_err(|error| format!("invalid Plugin Host capabilities ({})", error.code)),
            (
                NodeCommandPayload::PluginHostPlan { request },
                Self::PluginHostPlanned { capabilities, plan },
            ) => plan.validate_for(request, capabilities).map_err(|error| {
                format!(
                    "Plugin Host plan result does not match its request ({})",
                    error.code
                )
            }),
            (
                NodeCommandPayload::PluginHostApply { request },
                Self::PluginHostApplied {
                    capabilities,
                    applied,
                },
            ) => applied
                .validate_for(request, capabilities)
                .map_err(|error| {
                    format!(
                        "Plugin Host apply result does not match its request ({})",
                        error.code
                    )
                }),
            (
                NodeCommandPayload::PluginHostPlanEnablement { request },
                Self::PluginHostEnablementPlanned {
                    capabilities,
                    enablement_plan,
                },
            ) => enablement_plan
                .validate_for(request, capabilities)
                .map_err(|error| {
                    format!(
                        "Plugin Host enablement plan result does not match its request ({})",
                        error.code
                    )
                }),
            (
                NodeCommandPayload::PluginHostObserve { request },
                Self::PluginHostObserved {
                    capabilities,
                    observation,
                },
            ) => observation
                .validate_for(request, capabilities)
                .map_err(|error| {
                    format!(
                        "Plugin Host observation result does not match its request ({})",
                        error.code
                    )
                }),
            _ => Err("node command result kind does not match its payload".into()),
        }
    }
}

fn plugin_host_timestamp(label: &str, milliseconds: u64) -> Result<DateTime<Utc>, String> {
    let milliseconds = i64::try_from(milliseconds)
        .map_err(|_| format!("Plugin Host {label} time exceeds supported bounds"))?;
    DateTime::from_timestamp_millis(milliseconds)
        .ok_or_else(|| format!("Plugin Host {label} time exceeds supported bounds"))
}

fn code_agent_timestamp(milliseconds: u64) -> Result<DateTime<Utc>, String> {
    let milliseconds = i64::try_from(milliseconds)
        .map_err(|_| "A3S Code receipt time exceeds supported bounds".to_string())?;
    DateTime::from_timestamp_millis(milliseconds)
        .ok_or_else(|| "A3S Code receipt time exceeds supported bounds".to_string())
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
    pub const SCHEMA: &'static str = "a3s.cloud.node-command-ack.v2";
    pub const LEGACY_SCHEMA: &'static str = "a3s.cloud.node-command-ack.v1";

    pub fn validate_against(&self, command: &NodeCommandEnvelope) -> Result<(), String> {
        command.validate()?;
        if self.schema != Self::SCHEMA && self.schema != Self::LEGACY_SCHEMA {
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
            let result_evidence = match result.as_ref() {
                NodeCommandResult::ResourceClaimPrepared { prepared } => {
                    Some((prepared.prepared_at, false))
                }
                NodeCommandResult::ResourceClaimReleased { released } => {
                    Some((released.released_at, false))
                }
                NodeCommandResult::PluginHostPlanned { plan, .. } => Some((
                    plugin_host_timestamp("plan creation", plan.plan.plan.created_at_ms)?,
                    plan.replayed,
                )),
                NodeCommandResult::PluginHostApplied { applied, .. } => Some((
                    plugin_host_timestamp("apply completion", applied.completed_at_ms)?,
                    applied.replayed,
                )),
                NodeCommandResult::PluginHostEnablementPlanned {
                    enablement_plan, ..
                } => Some((
                    plugin_host_timestamp(
                        "enablement plan creation",
                        enablement_plan.planned_at_ms,
                    )?,
                    enablement_plan.replayed,
                )),
                NodeCommandResult::PluginHostObserved { observation, .. } => Some((
                    plugin_host_timestamp("observation", observation.observed_at_ms)?,
                    false,
                )),
                NodeCommandResult::CodeAgentCommandAccepted { receipt } => Some((
                    code_agent_timestamp(receipt.observed_at_ms)?,
                    receipt.replayed,
                )),
                NodeCommandResult::RuntimeApplied { .. }
                | NodeCommandResult::RuntimeInspected { .. }
                | NodeCommandResult::RuntimeStopped { .. }
                | NodeCommandResult::RuntimeRemoved { .. }
                | NodeCommandResult::BoxBuildStarted { .. }
                | NodeCommandResult::BoxBuildInspected { .. }
                | NodeCommandResult::BoxBuildCancelled { .. }
                | NodeCommandResult::BoxBuildRemoved { .. }
                | NodeCommandResult::GatewaySnapshotInstalled { .. }
                | NodeCommandResult::GatewaySnapshotObserved { .. }
                | NodeCommandResult::PluginHostCapabilitiesInspected { .. } => None,
            };
            if result_evidence.is_some_and(|(at, replayed)| {
                (!replayed && at < command.issued_at) || at > self.completed_at
            }) {
                return Err("node command result evidence time falls outside execution".into());
            }
            if matches!(
                command.payload,
                NodeCommandPayload::ResourceClaimPrepare { .. }
                    | NodeCommandPayload::ResourceClaimRelease { .. }
                    | NodeCommandPayload::RuntimeApply {
                        resource_claim: Some(_),
                        ..
                    }
                    | NodeCommandPayload::CodeAgentCommand { .. }
                    | NodeCommandPayload::BoxBuildStart { .. }
                    | NodeCommandPayload::BoxBuildInspect { .. }
                    | NodeCommandPayload::BoxBuildCancel { .. }
                    | NodeCommandPayload::BoxBuildRemove { .. }
                    | NodeCommandPayload::PluginHostCapabilitiesInspect { .. }
                    | NodeCommandPayload::PluginHostPlan { .. }
                    | NodeCommandPayload::PluginHostApply { .. }
                    | NodeCommandPayload::PluginHostPlanEnablement { .. }
                    | NodeCommandPayload::PluginHostObserve { .. }
            ) && self.schema != Self::SCHEMA
            {
                return Err("this command requires the current acknowledgement schema".into());
            }
            if let NodeCommandResult::GatewaySnapshotInstalled { acknowledgement } = result.as_ref()
            {
                let expected_gateway_schema = if self.schema == Self::SCHEMA {
                    NodeGatewayAck::SCHEMA
                } else {
                    NodeGatewayAck::LEGACY_SCHEMA
                };
                if acknowledgement.schema != expected_gateway_schema {
                    return Err(
                        "Gateway acknowledgement schema does not match its command acknowledgement"
                            .into(),
                    );
                }
                if acknowledgement.acknowledged_at < command.issued_at
                    || acknowledgement.acknowledged_at > self.completed_at
                {
                    return Err(
                        "Gateway acknowledgement time falls outside command execution".into(),
                    );
                }
            }
            if let NodeCommandResult::GatewaySnapshotObserved { observation } = result.as_ref() {
                if self.schema != Self::SCHEMA {
                    return Err(
                        "Gateway snapshot observations require the current acknowledgement schema"
                            .into(),
                    );
                }
                if observation.observed_at < command.issued_at
                    || observation.observed_at > self.completed_at
                {
                    return Err(
                        "Gateway snapshot observation time falls outside command execution".into(),
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
