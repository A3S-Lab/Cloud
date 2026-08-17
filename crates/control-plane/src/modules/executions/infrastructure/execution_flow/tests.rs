use super::cleanup;
use super::runtime;
use super::types::{
    CleanupDispatchInput, CleanupDispatchOutput, CleanupObserveInput, CleanupObserveOutput,
    DispatchInput, DispatchOutput, ExecutionFlowInput, ObserveInput, ObserveOutput, ScheduleOutput,
};
use super::{
    ExecutionFlowConfig, ExecutionFlowConfigOptions, ExecutionFlowRuntime,
    ExecutionFlowRuntimeDependencies,
};
use crate::modules::executions::domain::{
    CreateExecution, Execution, ExecutionArtifact, ExecutionOutcome, ExecutionProcess,
    ExecutionResources, ExecutionStatus, ExecutionTaskAuthority, ExecutionTaskPolicy,
    ExecutionTemplate, IExecutionRepository,
};
use crate::modules::executions::infrastructure::InMemoryExecutionRepository;
use crate::modules::fleet::domain::entities::EnrollmentToken;
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository, NodeEnrollmentDraft, NodeHeartbeatUpdate,
};
use crate::modules::fleet::domain::value_objects::{
    EnrollmentTokenCredential, NodeCapabilities, NodeName,
};
use crate::modules::fleet::infrastructure::persistence::InMemoryNodeRepository;
use crate::modules::shared_kernel::domain::{
    EnrollmentTokenId, EnvironmentId, ExecutionId, IdempotencyRequest, NodeId, OrganizationId,
    ProjectId, Sha256Digest,
};
use a3s_cloud_contracts::{
    artifact_uri, CloudSecretReference, DomainEventEnvelope, NodeCommandAck,
    NodeCommandLeaseRequest, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
    NodeHeartbeat, NodeObservationBatch, RuntimeObservationReport, DURABLE_CELL_BUNDLE_MEDIA_TYPE,
};
use a3s_runtime::contract::{
    ArtifactRef, IsolationLevel, MountKind, NetworkMode, ResourceControl, RuntimeCapabilities,
    RuntimeEvidence, RuntimeFeature, RuntimeMount, RuntimeMountSource, RuntimeObservation,
    RuntimeRemoval, RuntimeUnitClass, RuntimeUnitState, SecretReference, SecretTarget,
};
use chrono::{Duration, Utc};
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

struct Fixture {
    execution: Execution,
    executions: Arc<InMemoryExecutionRepository>,
    nodes: Arc<InMemoryNodeRepository>,
    node_id: NodeId,
    agent_instance_id: Uuid,
    capabilities: RuntimeCapabilities,
    runtime: ExecutionFlowRuntime,
}

impl Fixture {
    async fn create() -> Result<Self, Box<dyn std::error::Error>> {
        let now = Utc::now();
        let execution = execution(now)?;
        let executions = Arc::new(InMemoryExecutionRepository::new());
        executions
            .create(CreateExecution {
                execution: execution.clone(),
                idempotency: IdempotencyRequest::new(
                    "test/executions",
                    execution.id.to_string(),
                    b"execution",
                )?,
                event: event(execution.organization_id),
            })
            .await?;
        let nodes = Arc::new(InMemoryNodeRepository::new());
        ready_node(
            &nodes,
            execution.organization_id,
            now,
            "container-only",
            capabilities(IsolationLevel::Container),
        )
        .await?;
        let capabilities = capabilities(IsolationLevel::Sandbox);
        let (node_id, agent_instance_id) = ready_node(
            &nodes,
            execution.organization_id,
            now,
            "sandbox-ready",
            capabilities.clone(),
        )
        .await?;
        let execution_port: Arc<dyn IExecutionRepository> = executions.clone();
        let node_port: Arc<dyn INodeRepository> = nodes.clone();
        let control_port: Arc<dyn INodeControlRepository> = nodes.clone();
        let runtime = ExecutionFlowRuntime::new(
            ExecutionFlowRuntimeDependencies {
                executions: execution_port,
                nodes: node_port,
                node_control: control_port,
            },
            ExecutionFlowConfig::new(ExecutionFlowConfigOptions {
                heartbeat_timeout_ms: 5_000,
                command_ttl_ms: 900_000,
                observation_poll_ms: 1,
                convergence_timeout_ms: 60_000,
                cleanup_timeout_ms: 30_000,
            })?,
        );
        Ok(Self {
            execution,
            executions,
            nodes,
            node_id,
            agent_instance_id,
            capabilities,
            runtime,
        })
    }

    fn input(&self) -> ExecutionFlowInput {
        ExecutionFlowInput {
            organization_id: self.execution.organization_id,
            execution_id: self.execution.id,
        }
    }
}

#[tokio::test]
async fn runtime_task_runs_only_on_a_sandbox_node_and_completes_after_removal(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::create().await?;
    let run_id = fixture.execution.operation_id.to_string();
    let ScheduleOutput::Ready { scheduled } =
        runtime::schedule(&fixture.runtime, &run_id, fixture.input()).await?
    else {
        return Err("execution was not scheduled".into());
    };
    assert_eq!(scheduled.node_id, fixture.node_id);
    assert_eq!(scheduled.spec.class, RuntimeUnitClass::Task);
    assert_eq!(scheduled.spec.isolation, IsolationLevel::Sandbox);
    assert_eq!(scheduled.spec.network.mode, NetworkMode::None);

    let DispatchOutput::Ready { dispatched } =
        runtime::dispatch(&fixture.runtime, &run_id, DispatchInput { scheduled }).await?
    else {
        return Err("execution was not dispatched".into());
    };
    let apply_lease = lease(
        &fixture.nodes,
        fixture.node_id,
        fixture.agent_instance_id,
        0,
    )
    .await?;
    let apply = apply_lease
        .commands
        .first()
        .ok_or("missing execution Runtime apply")?;
    let NodeCommandPayload::RuntimeApply { request, .. } = &apply.payload else {
        return Err("execution command is not Runtime apply".into());
    };
    assert_eq!(request.spec, *dispatched.scheduled.spec);
    record_observation(
        &fixture.nodes,
        fixture.node_id,
        fixture.agent_instance_id,
        &fixture.capabilities,
        apply,
        succeeded_observation(&request.spec)?,
    )
    .await?;

    let ObserveOutput::Terminal { terminal } = runtime::observe(
        &fixture.runtime,
        &run_id,
        ObserveInput {
            dispatched: dispatched.clone(),
        },
    )
    .await?
    else {
        return Err("execution did not observe its terminal Task".into());
    };
    assert_eq!(
        terminal.outcome,
        ExecutionOutcome::Succeeded { exit_code: 0 }
    );
    assert_eq!(
        fixture
            .executions
            .find(fixture.execution.organization_id, fixture.execution.id)
            .await?
            .ok_or("missing execution")?
            .status,
        ExecutionStatus::CleanupPending
    );

    let cleanup_deadline = terminal.terminal_at + Duration::seconds(30);
    let CleanupDispatchOutput::Ready {
        dispatched: cleanup,
    } = cleanup::dispatch(
        &fixture.runtime,
        &run_id,
        CleanupDispatchInput {
            issued_at: terminal.terminal_at,
            terminal,
            attempt: 1,
            cleanup_deadline,
        },
    )
    .await?
    else {
        return Err("execution cleanup was not dispatched".into());
    };
    let cleanup_lease = lease(
        &fixture.nodes,
        fixture.node_id,
        fixture.agent_instance_id,
        apply.sequence,
    )
    .await?;
    let remove = cleanup_lease
        .commands
        .first()
        .ok_or("missing execution Runtime remove")?;
    acknowledge_removal(&fixture.nodes, remove).await?;

    let CleanupObserveOutput::Completed { execution } = cleanup::observe(
        &fixture.runtime,
        &run_id,
        CleanupObserveInput {
            dispatched: cleanup,
        },
    )
    .await?
    else {
        return Err("execution cleanup did not complete".into());
    };
    assert_eq!(execution.status, ExecutionStatus::Succeeded);
    let stored = fixture
        .executions
        .find(fixture.execution.organization_id, fixture.execution.id)
        .await?
        .ok_or("missing completed execution")?;
    assert_eq!(stored.status, ExecutionStatus::Succeeded);
    assert!(stored.finished_at.is_some());
    Ok(())
}

#[tokio::test]
async fn bound_runtime_task_schedules_only_on_its_exact_capable_node(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::create().await?;
    let now = Utc::now();
    let _other = ready_node(
        &fixture.nodes,
        fixture.execution.organization_id,
        now,
        "bound-other",
        bound_capabilities(),
    )
    .await?;
    let (target_node_id, _) = ready_node(
        &fixture.nodes,
        fixture.execution.organization_id,
        now,
        "bound-target",
        bound_capabilities(),
    )
    .await?;
    let bound = bound_execution(&fixture.execution, target_node_id, now)?;
    fixture
        .executions
        .create(CreateExecution {
            execution: bound.clone(),
            idempotency: IdempotencyRequest::new(
                "test/executions/internal-bound",
                bound.id.to_string(),
                b"bound-execution",
            )?,
            event: event(bound.organization_id),
        })
        .await?;

    let ScheduleOutput::Ready { scheduled } = runtime::schedule(
        &fixture.runtime,
        &bound.operation_id.to_string(),
        ExecutionFlowInput {
            organization_id: bound.organization_id,
            execution_id: bound.id,
        },
    )
    .await?
    else {
        return Err("bound execution was not scheduled".into());
    };
    assert_eq!(scheduled.node_id, target_node_id);
    assert_eq!(scheduled.spec.network.mode, NetworkMode::Outbound);
    assert_eq!(scheduled.spec.mounts.len(), 1);
    assert_eq!(scheduled.spec.secrets.len(), 1);
    Ok(())
}

#[tokio::test]
async fn cancellation_before_scheduling_finishes_without_a_runtime_command(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::create().await?;
    let mut execution = fixture
        .executions
        .find(fixture.execution.organization_id, fixture.execution.id)
        .await?
        .ok_or("execution")?;
    let expected = execution.aggregate_version;
    execution.request_cancellation(Utc::now())?;
    fixture.executions.save(execution, expected).await?;
    let run_id = fixture.execution.operation_id.to_string();
    let ScheduleOutput::Terminal { terminal } =
        runtime::schedule(&fixture.runtime, &run_id, fixture.input()).await?
    else {
        return Err("cancelled execution was scheduled".into());
    };
    assert_eq!(terminal.outcome, ExecutionOutcome::Cancelled);
    let CleanupDispatchOutput::Completed { execution } = cleanup::dispatch(
        &fixture.runtime,
        &run_id,
        CleanupDispatchInput {
            issued_at: terminal.terminal_at,
            cleanup_deadline: terminal.terminal_at + Duration::seconds(30),
            terminal,
            attempt: 1,
        },
    )
    .await?
    else {
        return Err("cancelled execution did not complete without Runtime state".into());
    };
    assert_eq!(execution.status, ExecutionStatus::Cancelled);
    assert!(lease(
        &fixture.nodes,
        fixture.node_id,
        fixture.agent_instance_id,
        0
    )
    .await?
    .commands
    .is_empty());
    Ok(())
}

fn execution(at: chrono::DateTime<Utc>) -> Result<Execution, String> {
    let digest = format!("sha256:{}", "a".repeat(64));
    Execution::create(
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        ExecutionId::new(),
        ExecutionTemplate {
            artifact: ExecutionArtifact {
                uri: format!("oci://registry.example/functions/echo@{digest}"),
                digest,
                media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            },
            process: ExecutionProcess {
                command: vec!["/app/echo".into()],
                args: Vec::new(),
                working_directory: None,
                environment: BTreeMap::new(),
            },
            input: serde_json::json!({"message": "hello"}),
            resources: ExecutionResources {
                cpu_millis: 250,
                memory_bytes: 128 * 1024 * 1024,
                pids: 64,
                ephemeral_storage_bytes: None,
                timeout_ms: 5_000,
            },
        },
        at,
    )
}

fn capabilities(isolation: IsolationLevel) -> RuntimeCapabilities {
    RuntimeCapabilities {
        schema: RuntimeCapabilities::SCHEMA.into(),
        provider_id: a3s_runtime::ProviderId::parse("test-execution-runtime").expect("provider ID"),
        provider_build: "test-execution-runtime-1".into(),
        unit_classes: vec![RuntimeUnitClass::Task],
        artifact_media_types: vec!["application/vnd.oci.image.manifest.v1+json".into()],
        isolation_levels: vec![isolation],
        network_modes: vec![NetworkMode::None],
        mount_kinds: Vec::new(),
        health_check_kinds: Vec::new(),
        resource_controls: vec![
            ResourceControl::Cpu,
            ResourceControl::Memory,
            ResourceControl::Pids,
            ResourceControl::ExecutionTimeout,
        ],
        features: vec![RuntimeFeature::DurableIdentity, RuntimeFeature::Remove],
    }
}

fn bound_capabilities() -> RuntimeCapabilities {
    let mut capabilities = capabilities(IsolationLevel::Sandbox);
    capabilities.network_modes = vec![NetworkMode::Outbound];
    capabilities.mount_kinds = vec![MountKind::Artifact];
    capabilities.features.push(RuntimeFeature::SecretReferences);
    capabilities
}

fn bound_execution(
    standard: &Execution,
    target_node_id: NodeId,
    at: chrono::DateTime<Utc>,
) -> Result<Execution, String> {
    let subject_id = Uuid::now_v7();
    let bundle_digest = format!("sha256:{}", "b".repeat(64));
    Execution::create_bound_task(
        standard.organization_id,
        standard.project_id,
        standard.environment_id,
        ExecutionId::new(),
        standard.template.clone(),
        target_node_id,
        ExecutionTaskPolicy {
            authority: ExecutionTaskAuthority {
                kind: "workload.prestart".into(),
                subject_id,
                digest: Sha256Digest::parse(format!("sha256:{}", "c".repeat(64)))?,
            },
            mounts: vec![RuntimeMount {
                name: "application-bundle".into(),
                source: RuntimeMountSource::Artifact {
                    artifact: ArtifactRef {
                        uri: artifact_uri(&bundle_digest)?,
                        digest: bundle_digest,
                        media_type: DURABLE_CELL_BUNDLE_MEDIA_TYPE.into(),
                    },
                },
                target: "/workspace/bundle".into(),
                read_only: true,
            }],
            secrets: vec![SecretReference {
                name: "s0-access-key-id".into(),
                reference: CloudSecretReference::new(subject_id, Uuid::now_v7(), 1)?.to_string(),
                target: SecretTarget::Environment {
                    variable: "AWS_ACCESS_KEY_ID".into(),
                },
            }],
            semantics_profile_digest: Sha256Digest::parse(format!("sha256:{}", "d".repeat(64)))?,
        },
        at,
    )
}

async fn ready_node(
    nodes: &InMemoryNodeRepository,
    organization_id: OrganizationId,
    enrolled_at: chrono::DateTime<Utc>,
    name: &str,
    capabilities: RuntimeCapabilities,
) -> Result<(NodeId, Uuid), Box<dyn std::error::Error>> {
    capabilities.validate()?;
    let token_id = EnrollmentTokenId::new();
    let secret = format!("a3sn_{}", token_id.as_uuid().simple().to_string().repeat(2));
    let credential = EnrollmentTokenCredential::from_secret(&secret)?;
    nodes
        .issue_enrollment_token(
            EnrollmentToken::new(
                token_id,
                organization_id,
                name,
                credential.clone(),
                enrolled_at,
                enrolled_at + Duration::minutes(5),
            )?,
            event(organization_id),
            IdempotencyRequest::new(
                "test.execution.enrollment",
                token_id.to_string(),
                token_id.to_string().as_bytes(),
            )?,
        )
        .await?;
    let stored = NodeCapabilities::new(
        capabilities.provider_id.to_string(),
        capabilities.provider_build.clone(),
        serde_json::to_value(&capabilities)?,
    )?;
    let agent_instance_id = Uuid::now_v7();
    let reservation = nodes
        .reserve_enrollment(
            &credential,
            NodeEnrollmentDraft {
                proposed_node_id: NodeId::new(),
                name: NodeName::new(name)?,
                agent_instance_id,
                agent_version: "0.1.0".into(),
                capabilities: stored.clone(),
                request_digest: format!("sha256:{}", "c".repeat(64)),
                requested_at: enrolled_at,
            },
        )
        .await?;
    nodes
        .record_heartbeat(NodeHeartbeatUpdate {
            node_id: reservation.node.id,
            agent_instance_id,
            agent_version: "0.1.0".into(),
            capabilities: stored,
            observed_at: enrolled_at + Duration::milliseconds(1),
        })
        .await?;
    Ok((reservation.node.id, agent_instance_id))
}

async fn lease(
    nodes: &InMemoryNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
    after_sequence: u64,
) -> Result<
    a3s_cloud_contracts::NodeCommandLeaseResponse,
    crate::modules::shared_kernel::domain::RepositoryError,
> {
    let now = Utc::now();
    nodes
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                after_sequence,
                max_commands: 10,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            now,
            now + Duration::seconds(1),
        )
        .await
}

async fn record_observation(
    nodes: &InMemoryNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
    capabilities: &RuntimeCapabilities,
    command: &a3s_cloud_contracts::NodeCommandEnvelope,
    observation: RuntimeObservation,
) -> Result<(), Box<dyn std::error::Error>> {
    let observed_at = Utc::now();
    nodes
        .record_observations(
            NodeObservationBatch {
                schema: NodeObservationBatch::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                sent_at: observed_at,
                heartbeat: NodeHeartbeat {
                    schema: NodeHeartbeat::SCHEMA.into(),
                    node_id: node_id.as_uuid(),
                    agent_instance_id,
                    observed_at,
                    agent_version: "0.1.0".into(),
                    runtime_capabilities: capabilities.clone(),
                },
                observations: vec![RuntimeObservationReport {
                    report_id: Uuid::now_v7(),
                    command_id: Some(command.command_id),
                    observed_at,
                    observation,
                }],
            }
            .into(),
            observed_at,
        )
        .await?;
    Ok(())
}

fn succeeded_observation(
    spec: &a3s_runtime::contract::RuntimeUnitSpec,
) -> Result<RuntimeObservation, String> {
    let now_ms = u64::try_from(Utc::now().timestamp_millis())
        .map_err(|_| "test clock predates Unix epoch")?;
    let observation = RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        spec_digest: spec.digest()?,
        class: RuntimeUnitClass::Task,
        state: RuntimeUnitState::Succeeded,
        provider_resource_id: Some("execution-sandbox-1".into()),
        provider_build: Some("test-execution-runtime-1".into()),
        observed_at_ms: now_ms,
        started_at_ms: Some(now_ms.saturating_sub(1)),
        finished_at_ms: Some(now_ms),
        health: None,
        outputs: Vec::new(),
        usage: None,
        evidence: Some(RuntimeEvidence {
            provider_build: "test-execution-runtime-1".into(),
            spec_digest: spec.digest()?,
            semantics_profile_digest: spec.semantics_profile_digest.clone(),
            claims: BTreeMap::new(),
        }),
        provider_attestation: None,
        failure: None,
    };
    observation.validate_against(spec)?;
    Ok(observation)
}

async fn acknowledge_removal(
    nodes: &InMemoryNodeRepository,
    command: &a3s_cloud_contracts::NodeCommandEnvelope,
) -> Result<(), Box<dyn std::error::Error>> {
    let NodeCommandPayload::RuntimeRemove { request } = &command.payload else {
        return Err("cleanup command is not Runtime remove".into());
    };
    let completed_at = Utc::now();
    nodes
        .acknowledge_command(
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: command.command_id,
                lease_id: command.lease_id,
                node_id: command.node_id,
                sequence: command.sequence,
                payload_digest: command.payload_digest.clone(),
                completed_at,
                outcome: NodeCommandOutcome::Succeeded {
                    result: Box::new(NodeCommandResult::RuntimeRemoved {
                        removal: RuntimeRemoval {
                            schema: RuntimeRemoval::SCHEMA.into(),
                            request_id: request.request_id.clone(),
                            unit_id: request.unit_id.clone(),
                            generation: request.generation,
                            removed_at_ms: u64::try_from(completed_at.timestamp_millis())?,
                            already_absent: false,
                        },
                    }),
                },
            },
            completed_at,
        )
        .await?;
    Ok(())
}

fn event(organization_id: OrganizationId) -> DomainEventEnvelope {
    DomainEventEnvelope {
        event_id: Uuid::now_v7(),
        event_key: "test.execution.fixture".into(),
        schema_version: 1,
        organization_id: organization_id.as_uuid(),
        aggregate_id: Uuid::now_v7(),
        aggregate_version: 1,
        occurred_at: Utc::now(),
        correlation_id: Uuid::now_v7(),
        causation_id: None,
        payload: serde_json::json!({}),
    }
}
