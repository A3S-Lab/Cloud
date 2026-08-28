use crate::migrate_and_connect_for_test;
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use a3s_cloud_contracts::{
    AgentProtocolEventPageV1, AgentProtocolEventRecordV1, AgentProtocolRunIdentityV1,
    AgentProtocolRunStateV1, AgentProviderApprovalOutcomeV1, AgentProviderCapabilityV1,
    AgentProviderCommandReceiptV1, AgentProviderCommandV1, AgentProviderEventPageV1,
    AgentProviderEventRecordV1, AgentProviderRunIdentityV1, AgentProviderRunStateV1,
    AgentProviderSemanticEventV1, AgentProviderToolPayloadIdentityV1, DomainEventEnvelope,
    HarnessAgentReleaseBindingV1, HarnessInvocationProfileV1, HarnessProviderBindingV1,
    HarnessToolBindingV1, HarnessWorkspaceBindingV1, NodeAgentProviderEventBatchV1,
    NodeCodeAgentEventBatchV1, NodeCommandAck, NodeCommandEnvelope, NodeCommandLeaseRequest,
    NodeCommandOutcome, NodeCommandPayload, NodeCommandResult, NodeHeartbeat, NodeObservationBatch,
    RuntimeObservationReport, RuntimeServiceEndpoint, HARNESS_INVOCATION_PROFILE_MAX_BYTES,
    REFERENCE_ECHO_AGENT_PROVIDER_KIND,
};
use a3s_cloud_control_plane::infrastructure::connect_postgres;
use a3s_cloud_control_plane::modules::agents::{
    AcceptAgentCodeEventBatchWrite, AcceptAgentProviderEventBatchWrite, AgentApprovalCheckpoint,
    AgentApprovalCheckpointStatus, AgentCodeRunBinding, AgentExecution,
    AgentExecutionCancellationRequested, AgentExecutionCheckpointObjectError,
    AgentExecutionCheckpointObjectReference, AgentExecutionCheckpointObjectWrite,
    AgentExecutionEventKind, AgentExecutionFlowConfig, AgentExecutionFlowConfigOptions,
    AgentExecutionFlowRuntime, AgentExecutionFlowRuntimeDependencies, AgentExecutionStatus,
    AssetsAgentReleaseAdmissionAdapter, BindAgentCodeRunWrite,
    BuiltInAgentExecutionProviderRegistry, CreateAgentConversation, CreateAgentConversationHandler,
    DecideAgentApprovalCheckpoint, DecideAgentApprovalCheckpointHandler,
    IAgentApprovalCheckpointRepository, IAgentExecutionCheckpointObjectStore, IAgentRepository,
    PostgresAgentRepository, RequestAgentExecutionCancellationWrite, StartAgentExecution,
    StartAgentExecutionHandler, NATIVE_CODE_AGENT_PROVIDER_KIND,
};
use a3s_cloud_control_plane::modules::artifacts::{
    HostedArtifactQueryService, PostgresBuildRunRepository,
};
use a3s_cloud_control_plane::modules::assets::{
    Asset, AssetCreated, AssetKind, AssetRelease, AssetReleaseDrafted, AssetReleaseVersion,
    CreateAssetReleaseWrite, CreateAssetWrite, HostedAssetBuildRequested, IAssetRepository,
    PostgresAssetRepository,
};
use a3s_cloud_control_plane::modules::fleet::domain::entities::{
    EnrollmentToken, NodeCommandDraft,
};
use a3s_cloud_control_plane::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository, NodeEnrollmentDraft, NodeHeartbeatUpdate,
};
use a3s_cloud_control_plane::modules::fleet::domain::value_objects::{
    EnrollmentTokenCredential, NodeCapabilities, NodeName,
};
use a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository;
use a3s_cloud_control_plane::modules::identity::domain::services::ResourceAccessEvaluator;
use a3s_cloud_control_plane::modules::identity::{
    IResourceAuthorizationDecisionRepository, ResourceAuthorizationDecisionRequest,
};
use a3s_cloud_control_plane::modules::projects::PostgresProjectsRepository;
use a3s_cloud_control_plane::modules::secrets::PostgresSecretRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    canonical_json_bounded, sha256_digest, AgentConversationId, AgentExecutionId, ApiTokenId,
    AssetId, AssetReleaseId, AuthorizationDecisionRef, EnrollmentTokenId, EnvironmentId,
    GitCommitSha, IdempotencyRequest, NodeCommandId, NodeId, OrganizationId, PrincipalId,
    ProjectId, RepositoryError, ResourceName, Sha256Digest,
};
use a3s_cloud_control_plane::modules::workloads::{
    project_runtime_spec, CreateAgentWorkloadDeployment, CreateAgentWorkloadDeploymentHandler,
    Deployment, DeploymentReplicaBinding, HttpHealthCheck, IWorkloadRepository,
    IWorkloadRuntimeTargetRepository, PostgresWorkloadRepository, ServicePort, ServiceProcess,
    ServiceResources, SourceWorkloadTemplate, Workload, WorkloadRevision,
};
use a3s_flow::{FlowRuntime, StepInvocation};
use a3s_orm::{
    sql_query, Database, DatabaseError, PostgresDialect, PostgresError, PostgresExecutor,
};
use a3s_runtime::contract::{
    HealthCheckKind, IsolationLevel, NetworkMode, ResourceControl, RuntimeApplyRequest,
    RuntimeCapabilities, RuntimeEvidence, RuntimeFeature, RuntimeHealthObservation,
    RuntimeHealthState, RuntimeObservation, RuntimeUnitClass, RuntimeUnitSpec, RuntimeUnitState,
    TransportProtocol,
};
use chrono::{DateTime, Duration, Timelike, Utc};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const PREPARE_STEP: &str = "agent_execution_prepare";
const DISPATCH_STEP: &str = "agent_execution_dispatch";
const OBSERVE_STEP: &str = "agent_execution_observe";

struct ScenarioState {
    organization_id: OrganizationId,
    conversation_id: AgentConversationId,
    execution_id: AgentExecutionId,
    run_id: String,
    node_id: NodeId,
    agent_instance_id: Uuid,
    runtime_spec: RuntimeUnitSpec,
    runtime_capabilities: RuntimeCapabilities,
    initial_runtime_received_at: DateTime<Utc>,
    initial_runtime_started_at_ms: u64,
    initial_run_id: String,
    retention_successor_run_id: String,
    start_dispatched: Value,
    start_sequence: u64,
}

struct StartedScenarioState {
    organization_id: OrganizationId,
    conversation_id: AgentConversationId,
    execution_id: AgentExecutionId,
    run_id: String,
    node_id: NodeId,
    agent_instance_id: Uuid,
    runtime_spec: RuntimeUnitSpec,
    runtime_capabilities: RuntimeCapabilities,
    initial_runtime_received_at: DateTime<Utc>,
    initial_runtime_started_at_ms: u64,
    start_dispatched: Value,
    start_sequence: u64,
}

struct StartedProviderScenario {
    state: StartedScenarioState,
    agents: Arc<PostgresAgentRepository>,
    initial_binding: AgentCodeRunBinding,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct CheckpointRecoveryScenario {
    pub(super) organization_id: OrganizationId,
    pub(super) conversation_id: AgentConversationId,
    pub(super) execution_id: AgentExecutionId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpectedCommand {
    Start,
    Recover,
    Resume,
    Cancel,
}

struct UnavailableCheckpointObjects;

#[async_trait::async_trait]
impl IAgentExecutionCheckpointObjectStore for UnavailableCheckpointObjects {
    async fn put(
        &self,
        _reference: &AgentExecutionCheckpointObjectReference,
        _body: Vec<u8>,
    ) -> Result<AgentExecutionCheckpointObjectWrite, AgentExecutionCheckpointObjectError> {
        Err(AgentExecutionCheckpointObjectError::Unavailable(
            "checkpoint objects are outside the Agent Code recovery fixture".into(),
        ))
    }

    async fn get(
        &self,
        _reference: &AgentExecutionCheckpointObjectReference,
    ) -> Result<Vec<u8>, AgentExecutionCheckpointObjectError> {
        Err(AgentExecutionCheckpointObjectError::Unavailable(
            "checkpoint objects are outside the Agent Code recovery fixture".into(),
        ))
    }
}

pub async fn exercise_agent_code_recovery(postgres_url: String) -> TestResult {
    let state = prepare_persisted_scenario(&postgres_url).await?;

    // Reconnect every repository and Flow runtime to prove the recovery path
    // depends only on PostgreSQL and Fleet observations, not process memory.
    let executor = connect_postgres(&postgres_url, 8).await?;
    let agents = Arc::new(PostgresAgentRepository::new(executor.clone()));
    let nodes = Arc::new(PostgresNodeRepository::new(executor.clone()));
    let workloads = Arc::new(PostgresWorkloadRepository::new(executor.clone()));
    let restored = agents
        .find_execution(state.organization_id, state.execution_id)
        .await?
        .ok_or_else(|| invalid("Agent execution disappeared across PostgreSQL reconnect"))?;
    let restored_binding = restored
        .code
        .as_ref()
        .ok_or_else(|| invalid("Agent provider binding disappeared across PostgreSQL reconnect"))?;
    let restored_invocation = restored_binding
        .require_invocation_profile()
        .map_err(invalid)?;
    let restored_invocation_digest = restored_invocation.digest().map_err(invalid)?;
    assert_eq!(
        restored_invocation.workspace.runtime_spec_digest,
        state.runtime_spec.digest()?
    );
    assert_eq!(
        restored_binding
            .provider_identity()?
            .invocation_profile_digest
            .as_deref(),
        Some(restored_invocation_digest.as_str())
    );
    let invariant_probe = Database::new(PostgresDialect, executor.clone());
    let changed_digest = format!("sha256:{}", "f".repeat(64));
    let mutation_error = invariant_probe
        .execute(
            sql_query::<()>("update agent_executions set invocation_profile_digest = ")
                .bind(changed_digest)
                .append(" where organization_id = ")
                .bind(state.organization_id.as_uuid())
                .append(" and id = ")
                .bind(state.execution_id.as_uuid()),
        )
        .await
        .expect_err("PostgreSQL must reject invocation-profile mutation");
    let mutation_message = match &mutation_error {
        DatabaseError::Execute(PostgresError::Database(source)) => source
            .as_db_error()
            .map(|error| error.message())
            .unwrap_or_default(),
        _ => "",
    };
    assert!(
        mutation_message.contains("Agent Harness invocation profile is immutable"),
        "unexpected invocation-profile mutation error: {mutation_error}"
    );
    let runtime = flow_runtime(agents.clone(), workloads, nodes.clone())?;

    let retention_recovery = run_step(
        &runtime,
        &state.run_id,
        OBSERVE_STEP,
        json!({"dispatched": state.start_dispatched}),
    )
    .await?;
    let retention_dispatched = pending_dispatched(&retention_recovery)?.clone();
    let first_recovery = lease_and_ack_code_command(
        nodes.as_ref(),
        state.node_id,
        state.agent_instance_id,
        state.start_sequence,
        ExpectedCommand::Recover,
        AgentProviderRunStateV1::Planning,
    )
    .await?;
    let (first_recovery_identity, first_checkpoint) = recovery_identity(&first_recovery)?;
    assert_eq!(first_checkpoint, state.initial_run_id);
    assert_eq!(first_recovery_identity, state.retention_successor_run_id);

    let settled_retention_recovery = run_step(
        &runtime,
        &state.run_id,
        OBSERVE_STEP,
        json!({"dispatched": retention_dispatched.clone()}),
    )
    .await?;
    assert_pending_without_dispatch(&settled_retention_recovery)?;

    let restarted_started_at_ms = state
        .initial_runtime_started_at_ms
        .checked_add(2_000)
        .ok_or_else(|| invalid("Runtime process start timestamp overflowed"))?;
    let restarted_observed_at_ms = restarted_started_at_ms
        .checked_add(1)
        .ok_or_else(|| invalid("Runtime observation timestamp overflowed"))?;
    let restarted_received_at = canonical_timestamp(Utc::now())
        .max(state.initial_runtime_received_at + Duration::milliseconds(1));
    record_runtime_observation(
        nodes.as_ref(),
        state.node_id,
        state.agent_instance_id,
        &state.runtime_spec,
        &state.runtime_capabilities,
        RuntimeObservationTiming {
            started_at_ms: restarted_started_at_ms,
            observed_at_ms: restarted_observed_at_ms,
            received_at: restarted_received_at,
        },
    )
    .await?;

    let current = agents
        .find_execution(state.organization_id, state.execution_id)
        .await?
        .ok_or_else(|| invalid("Agent execution disappeared before cancellation"))?;
    assert_eq!(current.status, AgentExecutionStatus::Running);
    let expected_version = current.aggregate_version;
    let mut cancelling = current;
    let cancellation_at = canonical_timestamp(Utc::now()).max(cancelling.updated_at);
    cancelling.request_cancellation(cancellation_at)?;
    agents
        .request_cancellation(RequestAgentExecutionCancellationWrite {
            event: AgentExecutionCancellationRequested::envelope(&cancelling, Uuid::now_v7())?,
            execution: cancelling,
            expected_version,
            idempotency: idempotency(
                "test.agent-code-recovery.cancellation",
                "cancel-after-runtime-restart",
                b"cancel-after-runtime-restart",
            )?,
        })
        .await?;

    // A provider restart and cancellation race must rotate the Code run first.
    let process_recovery = run_step(
        &runtime,
        &state.run_id,
        OBSERVE_STEP,
        json!({"dispatched": retention_dispatched}),
    )
    .await?;
    let process_dispatched = pending_dispatched(&process_recovery)?.clone();
    let second_recovery = lease_and_ack_code_command(
        nodes.as_ref(),
        state.node_id,
        state.agent_instance_id,
        first_recovery.sequence,
        ExpectedCommand::Recover,
        AgentProviderRunStateV1::Planning,
    )
    .await?;
    let (process_recovery_run_id, process_checkpoint) = recovery_identity(&second_recovery)?;
    assert_eq!(process_checkpoint, state.retention_successor_run_id);
    assert_eq!(
        process_recovery_run_id,
        AgentCodeRunBinding::recovery_run_id(state.execution_id, &state.retention_successor_run_id,)
    );

    let cancellation_dispatch = run_step(
        &runtime,
        &state.run_id,
        OBSERVE_STEP,
        json!({"dispatched": process_dispatched.clone()}),
    )
    .await?;
    assert_pending_without_dispatch(&cancellation_dispatch)?;
    let cancel = lease_and_ack_code_command(
        nodes.as_ref(),
        state.node_id,
        state.agent_instance_id,
        second_recovery.sequence,
        ExpectedCommand::Cancel,
        AgentProviderRunStateV1::Cancelled,
    )
    .await?;
    let cancel_run_id = command_identity(&cancel)?.run_id.clone();
    assert_eq!(cancel_run_id, process_recovery_run_id);

    let replayed_cancellation = run_step(
        &runtime,
        &state.run_id,
        OBSERVE_STEP,
        json!({"dispatched": process_dispatched}),
    )
    .await?;
    assert_pending_without_dispatch(&replayed_cancellation)?;

    let events = agents
        .list_events(state.organization_id, state.conversation_id, None, 100)
        .await?;
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].kind, AgentExecutionEventKind::ExecutionRequested);
    assert_eq!(events[1].kind, AgentExecutionEventKind::ModelOutput);
    assert_eq!(events[2].kind, AgentExecutionEventKind::ModelOutput);
    let execution = agents
        .find_execution(state.organization_id, state.execution_id)
        .await?
        .ok_or_else(|| invalid("Agent execution disappeared after recovery"))?;
    assert_eq!(execution.status, AgentExecutionStatus::Cancelling);
    assert_eq!(
        execution
            .code
            .as_ref()
            .ok_or_else(|| invalid("recovered execution omitted its Code binding"))?
            .identity()
            .run_id,
        process_recovery_run_id
    );

    let database = Database::new(PostgresDialect, executor);
    let command_counts = database
        .fetch_one_as(
            sql_query::<(i64, i64)>(
                "select count(*), count(*) filter (where acknowledgement is not null) from node_commands where node_id = ",
            )
            .bind(state.node_id.as_uuid())
            .append(" and command_kind = 'agent_provider_command'"),
        )
        .await?;
    assert_eq!(command_counts, (4, 4));
    assert_eq!(
        [
            first_recovery.sequence,
            second_recovery.sequence,
            cancel.sequence,
        ],
        [
            state.start_sequence + 1,
            state.start_sequence + 2,
            state.start_sequence + 3
        ]
    );

    println!(
        "A3S_CLOUD_A1_POSTGRES_RECOVERY_CERTIFIED store=postgresql commands=4 acknowledgements=4 semantic_events=3 run_rotations=2 control_plane_restarts=1 invocation_profile=immutable runtime_generation={} cancellation_order=recover_then_cancel",
        state.runtime_spec.generation
    );
    Ok(())
}

pub(super) async fn prepare_checkpoint_recovery_scenario(
    postgres_url: &str,
) -> Result<CheckpointRecoveryScenario, Box<dyn Error>> {
    let state = prepare_persisted_scenario(postgres_url).await?;
    Ok(CheckpointRecoveryScenario {
        organization_id: state.organization_id,
        conversation_id: state.conversation_id,
        execution_id: state.execution_id,
    })
}

include!("agent_code_recovery/scenario.rs");
include!("agent_code_recovery/non_code_recovery.rs");
include!("agent_code_recovery/approval_recovery.rs");
include!("agent_code_recovery/runtime_fixture.rs");
include!("agent_code_recovery/protocol.rs");
