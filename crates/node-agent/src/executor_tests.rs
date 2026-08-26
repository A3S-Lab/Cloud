use super::*;
use crate::code_harness::CodeHarnessTransport;
use crate::durable_cell_operator::{
    DurableCellOperatorCounters, DurableCellOperatorError, DurableCellOperatorTransport,
};
use a3s_cloud_contracts::{
    AgentProtocolCommandActionV1, AgentProtocolCommandReceiptV1, AgentProtocolCommandV1,
    AgentProtocolEventPageRequestV1, AgentProtocolEventPageV1, AgentProtocolRunIdentityV1,
    AgentProtocolRunStartV1, AgentProtocolRunStateV1, AgentProviderCommandV1, AgentProviderProfile,
    AgentProviderRunIdentityV1, AgentProviderRunStartV1, AppliedGatewaySnapshot, GatewaySnapshot,
    GatewaySnapshotObservationRequest, GatewaySnapshotObservationState,
    NodeAgentProviderRuntimeBindingV1, NodeCodeAgentRuntimeBindingV1, NodeCommandMetadata,
    NodeCommandPayload, NodeDurableCellOperatorBindingV1, NodeResourceClaimBinding,
    NodeResourceClaimPrepare, NodeResourceClaimRelease, NodeResourceInventory, NodeResourceSlot,
    ResourceAllocation, ResourceKind, ResourceSlotBinding, ResourceUnit, AGENT_PROTOCOL_V1,
};
use a3s_runtime::contract::{
    ArtifactRef, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy, RuntimeActionRequest,
    RuntimeApplyRequest, RuntimeCapabilities, RuntimeEvidence, RuntimeExecRequest,
    RuntimeExecResult, RuntimeLogChunk, RuntimeLogQuery, RuntimeNetworkSpec, RuntimeObservation,
    RuntimeProcessSpec, RuntimeRemoval, RuntimeServiceEndpoint, RuntimeUnitClass, RuntimeUnitSpec,
    RuntimeUnitState,
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
    observation_calls: AtomicUsize,
    outcome: GatewaySnapshotInstallOutcome,
    observation: Option<crate::GatewaySnapshotObservationOutcome>,
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

struct CodeHarnessRuntime {
    calls: AtomicUsize,
    observation: RuntimeObservation,
}

struct RecordingCodeHarness {
    calls: AtomicUsize,
    expected_endpoint: RuntimeServiceEndpoint,
}

struct RecordingDurableCellOperator {
    calls: AtomicUsize,
    expected_endpoint: RuntimeServiceEndpoint,
    counters: DurableCellOperatorCounters,
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
impl RuntimeClient for CodeHarnessRuntime {
    async fn capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
        Err(RuntimeError::Protocol("unused capabilities call".into()))
    }

    async fn apply(&self, _request: &RuntimeApplyRequest) -> RuntimeResult<RuntimeObservation> {
        Err(RuntimeError::Protocol("unused apply call".into()))
    }

    async fn inspect(&self, unit_id: &str) -> RuntimeResult<RuntimeInspection> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(unit_id, self.observation.unit_id);
        Ok(RuntimeInspection::Found {
            schema: RuntimeInspection::SCHEMA.into(),
            observation: Box::new(self.observation.clone()),
        })
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

#[async_trait]
impl CodeHarnessTransport for RecordingCodeHarness {
    async fn send_command(
        &self,
        endpoint: &RuntimeServiceEndpoint,
        command: &AgentProtocolCommandV1,
        timeout: std::time::Duration,
    ) -> Result<AgentProtocolCommandReceiptV1, CodeHarnessError> {
        assert_eq!(endpoint, &self.expected_endpoint);
        assert!(!timeout.is_zero());
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(AgentProtocolCommandReceiptV1 {
            schema: AgentProtocolCommandReceiptV1::SCHEMA.into(),
            action: command.action(),
            request_id: command.request_id().into(),
            identity: command.identity().clone(),
            command_digest: command
                .digest()
                .map_err(|error| CodeHarnessError::Protocol(error.code().into()))?,
            state: AgentProtocolRunStateV1::Created,
            latest_event_sequence_exclusive: 0,
            observed_at_ms: u64::try_from(Utc::now().timestamp_millis())
                .map_err(|error| CodeHarnessError::Protocol(error.to_string()))?,
            replayed: false,
        })
    }

    async fn event_page(
        &self,
        _endpoint: &RuntimeServiceEndpoint,
        _request: &AgentProtocolEventPageRequestV1,
        _timeout: std::time::Duration,
    ) -> Result<AgentProtocolEventPageV1, CodeHarnessError> {
        Err(CodeHarnessError::Invalid(
            "unexpected Code event page request".into(),
        ))
    }
}

#[async_trait]
impl DurableCellOperatorTransport for RecordingDurableCellOperator {
    async fn observe(
        &self,
        endpoint: &RuntimeServiceEndpoint,
        timeout: std::time::Duration,
    ) -> Result<DurableCellOperatorCounters, DurableCellOperatorError> {
        assert_eq!(endpoint, &self.expected_endpoint);
        assert!(!timeout.is_zero());
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.counters)
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

    async fn observe(
        &self,
        _request: &GatewaySnapshotObservationRequest,
    ) -> Result<crate::GatewaySnapshotObservationOutcome, GatewaySnapshotInstallError> {
        self.observation_calls.fetch_add(1, Ordering::SeqCst);
        self.observation.clone().ok_or_else(|| {
            GatewaySnapshotInstallError::Protocol("test Gateway observation is unavailable".into())
        })
    }
}

fn gateway() -> Arc<InspectGateway> {
    Arc::new(InspectGateway {
        calls: AtomicUsize::new(0),
        observation_calls: AtomicUsize::new(0),
        outcome: GatewaySnapshotInstallOutcome::Applied {
            protocol: a3s_cloud_contracts::GatewayManagementProtocol::v1(
                a3s_cloud_contracts::GatewayManagementProtocolDiscovery::Advertised,
            ),
        },
        observation: None,
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
                "A3S Box is offline".into(),
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

fn claim_binding(claim_id: Uuid, inventory: &NodeResourceInventory) -> NodeResourceClaimBinding {
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
    let authority: Arc<dyn NodeResourceInventoryAuthority> = Arc::new(FixedInventoryAuthority {
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
    let NodeCommandResult::GatewaySnapshotInstalled { acknowledgement } = result.as_ref() else {
        panic!("Gateway install returned the wrong result kind");
    };
    acknowledgement
        .validate_for(envelope.command_id, node_id, &snapshot)
        .expect("exact Gateway acknowledgement");
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn gateway_observation_is_read_only_exact_and_journaled_for_replay() {
    let directory = tempfile::tempdir().expect("journal directory");
    let node_id = Uuid::now_v7();
    let issued_at = Utc::now() - Duration::seconds(1);
    let not_after = issued_at + Duration::minutes(1);
    let request =
        GatewaySnapshotObservationRequest::new(node_id, 4, format!("sha256:{}", "d".repeat(64)))
            .expect("Gateway observation request");
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
        NodeCommandPayload::GatewaySnapshotObserve {
            request: request.clone(),
        },
    )
    .expect("Gateway observation command");
    let gateway = Arc::new(InspectGateway {
        calls: AtomicUsize::new(0),
        observation_calls: AtomicUsize::new(0),
        outcome: GatewaySnapshotInstallOutcome::Applied {
            protocol: a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1(),
        },
        observation: Some(crate::GatewaySnapshotObservationOutcome {
            state: GatewaySnapshotObservationState::NotApplied,
            ready: false,
            applied: Some(AppliedGatewaySnapshot {
                gateway_id: node_id,
                revision: 3,
                expected_revision: Some(2),
                snapshot_digest: format!("sha256:{}", "c".repeat(64)),
                issued_at: issued_at - Duration::minutes(2),
                expires_at: issued_at + Duration::hours(1),
                applied_at: issued_at - Duration::minutes(1),
            }),
            observed_at: issued_at + Duration::milliseconds(10),
            protocol: a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1(),
        }),
    });
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
        .expect("execute Gateway observation");
    let NodeCommandOutcome::Succeeded { result } = &acknowledgement.outcome else {
        panic!("Gateway observation must produce a result");
    };
    let NodeCommandResult::GatewaySnapshotObserved { observation } = result.as_ref() else {
        panic!("Gateway observation returned the wrong result kind");
    };
    observation
        .validate_for(envelope.command_id, node_id, &request)
        .expect("exact Gateway observation");
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 0);
    assert_eq!(gateway.observation_calls.load(Ordering::SeqCst), 1);

    let mut redelivered = envelope;
    redelivered.lease_id = Uuid::now_v7();
    let replayed = executor
        .execute(redelivered)
        .await
        .expect("replay Gateway observation");
    assert_eq!(replayed.outcome, acknowledgement.outcome);
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 0);
    assert_eq!(gateway.observation_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn provider_and_legacy_code_commands_are_forwarded_once() {
    let directory = tempfile::tempdir().expect("journal directory");
    let node_id = Uuid::now_v7();
    let execution_id = Uuid::now_v7();
    let release_identity = format!("sha256:{}", "a".repeat(64));
    let runtime_spec_digest = format!("sha256:{}", "b".repeat(64));
    let binding = NodeCodeAgentRuntimeBindingV1 {
        schema: NodeCodeAgentRuntimeBindingV1::SCHEMA.into(),
        execution_id,
        workload_id: Uuid::now_v7(),
        workload_revision_id: Uuid::now_v7(),
        deployment_id: Uuid::now_v7(),
        replica_id: Uuid::now_v7(),
        runtime_unit_id: "agent-workload:revision:7".into(),
        runtime_generation: 7,
        runtime_spec_digest: runtime_spec_digest.clone(),
        service_port_name: "agent".into(),
        code_run_identity: AgentProtocolRunIdentityV1 {
            schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
            protocol: AGENT_PROTOCOL_V1.into(),
            agent_release_identity: release_identity,
            session_id: "conversation-1".into(),
            run_id: "execution-1-attempt-1".into(),
        },
    };
    let endpoint = RuntimeServiceEndpoint::node_local_tcp("agent", 49_152)
        .expect("node-local Code Harness endpoint");
    let mut claims = BTreeMap::new();
    endpoint
        .insert_claim(&mut claims)
        .expect("Runtime endpoint claim");
    let now_ms = u64::try_from(Utc::now().timestamp_millis()).expect("current timestamp");
    let observation = RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: binding.runtime_unit_id.clone(),
        generation: binding.runtime_generation,
        spec_digest: runtime_spec_digest.clone(),
        class: RuntimeUnitClass::Service,
        state: RuntimeUnitState::Running,
        provider_resource_id: Some("provider-agent-service".into()),
        provider_build: Some("a3s-box-test".into()),
        observed_at_ms: now_ms,
        started_at_ms: Some(now_ms.saturating_sub(1)),
        finished_at_ms: None,
        health: None,
        outputs: Vec::new(),
        usage: None,
        evidence: Some(RuntimeEvidence {
            provider_build: "a3s-box-test".into(),
            spec_digest: runtime_spec_digest,
            semantics_profile_digest: None,
            claims,
        }),
        provider_attestation: None,
        failure: None,
    };
    observation.validate().expect("valid Runtime observation");
    let code_command = AgentProtocolCommandV1::Start {
        request: AgentProtocolRunStartV1 {
            schema: AgentProtocolRunStartV1::SCHEMA.into(),
            request_id: "execution-1:start".into(),
            identity: binding.code_run_identity.clone(),
            prompt: "Fix the failing test.".into(),
        },
    };
    assert_eq!(code_command.action(), AgentProtocolCommandActionV1::Start);
    let envelope = claim_command(
        node_id,
        execution_id,
        1,
        binding.runtime_generation,
        NodeCommandPayload::CodeAgentCommand {
            binding: Box::new(binding.clone()),
            command: Box::new(code_command.clone()),
        },
    );
    let runtime = Arc::new(CodeHarnessRuntime {
        calls: AtomicUsize::new(0),
        observation,
    });
    let harness = Arc::new(RecordingCodeHarness {
        calls: AtomicUsize::new(0),
        expected_endpoint: endpoint,
    });
    let harness_transport: Arc<dyn CodeHarnessTransport> = harness.clone();
    let executor = CommandExecutor::runtime_only(
        FileCommandJournal::new(directory.path(), node_id).expect("journal"),
        runtime.clone(),
    )
    .with_code_harness(harness_transport);

    let acknowledgement = executor
        .execute(envelope.clone())
        .await
        .expect("forward Code command");
    acknowledgement
        .validate_against(&envelope)
        .expect("exact Code command acknowledgement");
    let NodeCommandOutcome::Succeeded { result } = &acknowledgement.outcome else {
        panic!("Code command must succeed");
    };
    let NodeCommandResult::CodeAgentCommandAccepted { receipt } = result.as_ref() else {
        panic!("Code command returned another result kind");
    };
    receipt
        .validate_for(&code_command)
        .expect("exact Code-owned receipt");
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
    assert_eq!(harness.calls.load(Ordering::SeqCst), 1);

    let mut redelivered = envelope;
    redelivered.lease_id = Uuid::now_v7();
    let replayed = executor
        .execute(redelivered)
        .await
        .expect("replay Code node command");
    assert_eq!(replayed.outcome, acknowledgement.outcome);
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
    assert_eq!(harness.calls.load(Ordering::SeqCst), 1);

    let profile = AgentProviderProfile::parse_acl(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/a1.3/a3s-code-provider-profile.acl"
    )))
    .expect("Code provider profile");
    let provider_identity = AgentProviderRunIdentityV1::new(
        profile.digest().into(),
        profile.capability_digest().into(),
        binding.code_run_identity.agent_release_identity.clone(),
        binding.code_run_identity.session_id.clone(),
        binding.code_run_identity.run_id.clone(),
    )
    .expect("provider run identity");
    let provider_binding = NodeAgentProviderRuntimeBindingV1 {
        schema: NodeAgentProviderRuntimeBindingV1::SCHEMA.into(),
        execution_id,
        workload_id: binding.workload_id,
        workload_revision_id: binding.workload_revision_id,
        deployment_id: binding.deployment_id,
        replica_id: binding.replica_id,
        runtime_unit_id: binding.runtime_unit_id.clone(),
        runtime_generation: binding.runtime_generation,
        runtime_spec_digest: binding.runtime_spec_digest.clone(),
        service_port_name: binding.service_port_name.clone(),
        provider_profile_acl: profile.canonical_acl().into(),
        provider_profile_digest: profile.digest().into(),
        provider_run_identity: provider_identity.clone(),
    };
    let provider_command = AgentProviderCommandV1::Start {
        request: AgentProviderRunStartV1::new(
            "execution-1:provider-start".into(),
            provider_identity,
            "Fix the failing test.".into(),
        )
        .expect("provider start command"),
    };
    let provider_envelope = claim_command(
        node_id,
        execution_id,
        2,
        provider_binding.runtime_generation,
        NodeCommandPayload::AgentProviderCommand {
            binding: Box::new(provider_binding.clone()),
            command: Box::new(provider_command.clone()),
        },
    );
    let provider_acknowledgement = executor
        .execute(provider_envelope.clone())
        .await
        .expect("forward provider-neutral command");
    provider_acknowledgement
        .validate_against(&provider_envelope)
        .expect("exact provider command acknowledgement");
    let NodeCommandOutcome::Succeeded { result } = &provider_acknowledgement.outcome else {
        panic!("provider command must succeed");
    };
    let NodeCommandResult::AgentProviderCommandAccepted { receipt } = result.as_ref() else {
        panic!("provider command returned another result kind");
    };
    receipt
        .validate_for(&profile, &provider_command)
        .expect("provider-neutral receipt");
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 2);
    assert_eq!(harness.calls.load(Ordering::SeqCst), 2);

    let mut provider_redelivery = provider_envelope;
    provider_redelivery.lease_id = Uuid::now_v7();
    let provider_replay = executor
        .execute(provider_redelivery)
        .await
        .expect("replay provider command");
    assert_eq!(provider_replay.outcome, provider_acknowledgement.outcome);
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 2);
    assert_eq!(harness.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn durable_cell_operator_observation_is_sanitized_and_journaled_once() {
    let directory = tempfile::tempdir().expect("journal directory");
    let node_id = Uuid::now_v7();
    let application_id = Uuid::now_v7();
    let runtime_spec_digest = format!("sha256:{}", "a".repeat(64));
    let service_profile_digest = format!("sha256:{}", "b".repeat(64));
    let binding = NodeDurableCellOperatorBindingV1 {
        schema: NodeDurableCellOperatorBindingV1::SCHEMA.into(),
        application_id,
        application_revision_id: Uuid::now_v7(),
        application_revision_number: 7,
        workload_id: Uuid::now_v7(),
        workload_revision_id: Uuid::now_v7(),
        runtime_unit_id: "durable-cell-workload:revision:7".into(),
        runtime_generation: 7,
        runtime_spec_digest: runtime_spec_digest.clone(),
        service_profile_digest: service_profile_digest.clone(),
        service_template_digest: format!("sha256:{}", "c".repeat(64)),
        provider_artifact_digest: format!("sha256:{}", "d".repeat(64)),
        internal_service_port_name: "cell-internal".into(),
    };
    let endpoint = RuntimeServiceEndpoint::node_local_tcp("cell-internal", 49_153)
        .expect("node-local Durable Cell operator endpoint");
    let mut claims = BTreeMap::new();
    endpoint
        .insert_claim(&mut claims)
        .expect("Runtime endpoint claim");
    let now_ms = u64::try_from(Utc::now().timestamp_millis()).expect("current timestamp");
    let observation = RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: binding.runtime_unit_id.clone(),
        generation: binding.runtime_generation,
        spec_digest: runtime_spec_digest.clone(),
        class: RuntimeUnitClass::Service,
        state: RuntimeUnitState::Running,
        provider_resource_id: Some("provider-durable-cell-service".into()),
        provider_build: Some("a3s-box-test".into()),
        observed_at_ms: now_ms,
        started_at_ms: Some(now_ms.saturating_sub(1)),
        finished_at_ms: None,
        health: Some(a3s_runtime::contract::RuntimeHealthObservation {
            state: a3s_runtime::contract::RuntimeHealthState::Healthy,
            checked_at_ms: now_ms,
            message: None,
        }),
        outputs: Vec::new(),
        usage: None,
        evidence: Some(RuntimeEvidence {
            provider_build: "a3s-box-test".into(),
            spec_digest: runtime_spec_digest,
            semantics_profile_digest: Some(service_profile_digest),
            claims,
        }),
        provider_attestation: None,
        failure: None,
    };
    observation.validate().expect("valid Runtime observation");
    let envelope = claim_command(
        node_id,
        application_id,
        1,
        binding.runtime_generation,
        NodeCommandPayload::DurableCellOperatorObserve {
            binding: Box::new(binding.clone()),
        },
    );
    let runtime = Arc::new(CodeHarnessRuntime {
        calls: AtomicUsize::new(0),
        observation,
    });
    let operator = Arc::new(RecordingDurableCellOperator {
        calls: AtomicUsize::new(0),
        expected_endpoint: endpoint,
        counters: DurableCellOperatorCounters {
            occupied: 3,
            evicting: 1,
            restoring: 2,
            activating: 4,
            activation_waiting: 5,
            capacity_waiting: 6,
        },
    });
    let transport: Arc<dyn DurableCellOperatorTransport> = operator.clone();
    let executor = CommandExecutor::runtime_only(
        FileCommandJournal::new(directory.path(), node_id).expect("journal"),
        runtime.clone(),
    )
    .with_durable_cell_operator(transport);

    let acknowledgement = executor
        .execute(envelope.clone())
        .await
        .expect("observe Durable Cell operator");
    acknowledgement
        .validate_against(&envelope)
        .expect("exact Durable Cell operator acknowledgement");
    let NodeCommandOutcome::Succeeded { result } = &acknowledgement.outcome else {
        panic!("Durable Cell operator observation must succeed");
    };
    let NodeCommandResult::DurableCellOperatorObserved { observation } = result.as_ref() else {
        panic!("Durable Cell operator returned another result kind");
    };
    observation
        .validate_for(&binding)
        .expect("exact sanitized operator observation");
    assert_eq!(observation.occupied, 3);
    assert_eq!(observation.capacity_waiting, 6);
    let encoded = serde_json::to_string(observation).expect("operator observation JSON");
    assert!(!encoded.contains("residents"));
    assert!(!encoded.contains("published"));
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
    assert_eq!(operator.calls.load(Ordering::SeqCst), 1);

    let mut redelivered = envelope;
    redelivered.lease_id = Uuid::now_v7();
    let replayed = executor
        .execute(redelivered)
        .await
        .expect("replay Durable Cell operator command");
    assert_eq!(replayed.outcome, acknowledgement.outcome);
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
    assert_eq!(operator.calls.load(Ordering::SeqCst), 1);
}
