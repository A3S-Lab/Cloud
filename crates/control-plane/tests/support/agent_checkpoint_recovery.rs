#[path = "agent_checkpoint_recovery/object_store.rs"]
mod object_store;
#[path = "agent_checkpoint_recovery/process.rs"]
mod process;
#[path = "agent_checkpoint_recovery/verification.rs"]
mod verification;

use crate::agent_code_recovery_support::prepare_checkpoint_recovery_scenario;
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use a3s_cloud_control_plane::infrastructure::connect_postgres;
use a3s_cloud_control_plane::modules::agents::{
    AgentExecutionCheckpoint, AgentExecutionCheckpointSnapshot,
    BuiltInAgentExecutionProviderRegistry, CaptureAgentExecutionCheckpoint,
    CaptureAgentExecutionCheckpointHandler, CaptureAgentExecutionCheckpointResult,
    CapturedAgentExecutionCheckpoint, ForkAgentExecution, ForkAgentExecutionHandler,
    ForkAgentExecutionResult, IAgentExecutionCheckpointObjectStore, IAgentRepository,
    PostgresAgentRepository,
};
use a3s_cloud_control_plane::modules::artifacts::{
    HostedArtifactQueryService, IHostedArtifactQueryPort, PostgresBuildRunRepository,
};
use a3s_cloud_control_plane::modules::assets::{IAssetRepository, PostgresAssetRepository};
use a3s_cloud_control_plane::modules::identity::domain::services::ResourceAccessEvaluator;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    AgentConversationId, AgentExecutionId, OrganizationId,
};
use chrono::Duration;
use object_store::DurableCheckpointObjectStore;
use process::ProbeMode;
use serde_json::{json, Value};
use std::error::Error;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const CHECKPOINT_IDEMPOTENCY_KEY: &str = "checkpoint-after-object-process-death";
const FORK_IDEMPOTENCY_KEY: &str = "fork-after-commit-process-death";

struct Fixture {
    postgres_url: String,
    state_dir: PathBuf,
    objects_dir: PathBuf,
    organization_id: OrganizationId,
    conversation_id: AgentConversationId,
    execution_id: AgentExecutionId,
}

struct Dependencies {
    agents: Arc<dyn IAgentRepository>,
    objects: Arc<DurableCheckpointObjectStore>,
    assets: Arc<dyn IAssetRepository>,
    artifacts: Arc<dyn IHostedArtifactQueryPort>,
    providers: Arc<BuiltInAgentExecutionProviderRegistry>,
}

pub async fn exercise_agent_checkpoint_process_death_recovery(postgres_url: String) -> TestResult {
    let scenario = prepare_checkpoint_recovery_scenario(&postgres_url).await?;
    let state = tempfile::tempdir()?;
    let objects_dir = state.path().join("objects");
    std::fs::create_dir_all(&objects_dir)?;
    let fixture = Fixture {
        postgres_url,
        state_dir: state.path().to_path_buf(),
        objects_dir,
        organization_id: scenario.organization_id,
        conversation_id: scenario.conversation_id,
        execution_id: scenario.execution_id,
    };

    let object_marker = process::crash_at(&fixture, 1, ProbeMode::ObjectCommitted, None).await?;
    let dependencies = build_dependencies(&fixture).await?;
    let orphan = materialize_root_checkpoint(&dependencies, &fixture).await?;
    require_marker_value(&object_marker, "mode", ProbeMode::ObjectCommitted.as_str())?;
    require_marker_value(
        &object_marker,
        "checkpointId",
        &orphan.checkpoint.id.to_string(),
    )?;
    require_marker_value(
        &object_marker,
        "objectDigest",
        orphan.checkpoint.object.digest.as_str(),
    )?;
    require_marker_u64(
        &object_marker,
        "objectSizeBytes",
        orphan.checkpoint.object.size_bytes,
    )?;
    require_marker_u64(
        &object_marker,
        "throughEventSequence",
        orphan.checkpoint.through_event_sequence,
    )?;
    assert!(
        dependencies
            .agents
            .find_execution_checkpoint(fixture.organization_id, orphan.checkpoint.id)
            .await?
            .is_none(),
        "object-before-projection crash unexpectedly committed a checkpoint projection"
    );
    let object_path = dependencies
        .objects
        .object_path(&orphan.checkpoint.object)?;
    assert!(object_path.is_file());
    let orphan_bytes = dependencies.objects.get(&orphan.checkpoint.object).await?;
    assert_eq!(orphan_bytes, orphan.bytes);
    verification::assert_pre_projection_gap(&fixture, orphan.checkpoint.id).await?;

    let captured = execute_capture(&dependencies, &fixture).await?;
    assert!(!captured.replayed);
    assert_eq!(captured.checkpoint, orphan.checkpoint);
    assert_eq!(
        dependencies
            .objects
            .get(&captured.checkpoint.object)
            .await?,
        orphan_bytes
    );

    let reconnected = build_dependencies(&fixture).await?;
    let capture_replay = execute_capture(&reconnected, &fixture).await?;
    assert!(capture_replay.replayed);
    assert_eq!(capture_replay.checkpoint, captured.checkpoint);
    let snapshot: AgentExecutionCheckpointSnapshot = serde_json::from_slice(
        &reconnected
            .objects
            .get(&capture_replay.checkpoint.object)
            .await?,
    )?;
    capture_replay
        .checkpoint
        .validate_snapshot(&snapshot)
        .map_err(invalid)?;
    assert_eq!(usize::from(snapshot.event_count), snapshot.events.len());
    assert_eq!(snapshot.events.len(), 3);

    let parent_before_fork = reconnected
        .agents
        .find_execution(fixture.organization_id, fixture.execution_id)
        .await?
        .ok_or_else(|| invalid("Agent fork parent disappeared before crash probe"))?;
    let fork_marker = process::crash_at(
        &fixture,
        2,
        ProbeMode::ForkCommitted,
        Some(capture_replay.checkpoint.id),
    )
    .await?;
    require_marker_value(&fork_marker, "mode", ProbeMode::ForkCommitted.as_str())?;
    require_marker_value(
        &fork_marker,
        "parentExecutionId",
        &fixture.execution_id.to_string(),
    )?;
    require_marker_value(
        &fork_marker,
        "checkpointId",
        &capture_replay.checkpoint.id.to_string(),
    )?;
    require_marker_value(
        &fork_marker,
        "checkpointDigest",
        capture_replay.checkpoint.object.digest.as_str(),
    )?;
    let committed_child_id = parse_execution_id(&fork_marker, "childExecutionId")?;

    let after_fork_restart = build_dependencies(&fixture).await?;
    let committed_child = after_fork_restart
        .agents
        .find_execution(fixture.organization_id, committed_child_id)
        .await?
        .ok_or_else(|| invalid("committed Agent fork disappeared across process death"))?;
    require_marker_value(
        &fork_marker,
        "childOperationId",
        &committed_child.operation_id.to_string(),
    )?;
    let fork_replay =
        execute_fork(&after_fork_restart, &fixture, &capture_replay.checkpoint).await?;
    assert!(fork_replay.replayed);
    assert_eq!(fork_replay.execution, committed_child);
    let lineage = fork_replay
        .execution
        .lineage
        .as_ref()
        .ok_or_else(|| invalid("replayed Agent fork omitted its lineage"))?;
    assert_eq!(lineage.parent_execution_id, fixture.execution_id);
    assert_eq!(lineage.parent_checkpoint_id, capture_replay.checkpoint.id);
    assert_eq!(
        lineage.parent_checkpoint_digest,
        capture_replay.checkpoint.object.digest
    );
    assert_eq!(lineage.depth, 1);
    let parent_after_fork = after_fork_restart
        .agents
        .find_execution(fixture.organization_id, fixture.execution_id)
        .await?
        .ok_or_else(|| invalid("Agent fork parent disappeared after crash recovery"))?;
    assert_eq!(parent_after_fork, parent_before_fork);
    let child_events = after_fork_restart
        .agents
        .list_execution_trajectory_events(
            fixture.organization_id,
            committed_child_id,
            None,
            None,
            10,
        )
        .await?;
    assert_eq!(child_events.len(), 1);
    assert_eq!(
        child_events[0].kind,
        a3s_cloud_control_plane::modules::agents::AgentExecutionEventKind::ExecutionRequested
    );

    verification::assert_committed_recovery_state(
        &fixture,
        capture_replay.checkpoint.id,
        committed_child_id,
    )
    .await?;
    println!(
        "A3S_CLOUD_A1_CHECKPOINT_POSTGRES_RECOVERY_CERTIFIED store=postgresql object_authority=process-shared checkpoint_crashes=1 fork_crashes=1 checkpoints=1 forks=1 checkpoint_replays=1 fork_replays=1 lineage=exact"
    );
    Ok(())
}

pub async fn run_probe() -> TestResult {
    let environment = process::probe_environment()?;
    if !environment.state_dir.is_dir() {
        return Err("Agent checkpoint crash probe state directory is missing".into());
    }
    let fixture = Fixture {
        postgres_url: environment.postgres_url,
        state_dir: environment.state_dir,
        objects_dir: environment.objects_dir,
        organization_id: environment.organization_id,
        conversation_id: environment.conversation_id,
        execution_id: environment.execution_id,
    };
    let dependencies = build_dependencies(&fixture).await?;
    let marker = match environment.mode {
        ProbeMode::ObjectCommitted => {
            let captured = materialize_root_checkpoint(&dependencies, &fixture).await?;
            let write = dependencies
                .objects
                .put(&captured.checkpoint.object, captured.bytes)
                .await?;
            if write.replayed {
                return Err(
                    "Agent checkpoint crash probe unexpectedly replayed its first object".into(),
                );
            }
            json!({
                "schema": "a3s.cloud.agent-checkpoint-process-death-marker.v1",
                "mode": environment.mode.as_str(),
                "checkpointId": captured.checkpoint.id,
                "throughEventSequence": captured.checkpoint.through_event_sequence,
                "objectDigest": captured.checkpoint.object.digest,
                "objectSizeBytes": captured.checkpoint.object.size_bytes,
            })
        }
        ProbeMode::ForkCommitted => {
            let checkpoint_id = environment.checkpoint_id.ok_or_else(|| {
                invalid("Agent fork crash probe omitted its committed checkpoint identity")
            })?;
            let checkpoint = dependencies
                .agents
                .find_execution_checkpoint(fixture.organization_id, checkpoint_id)
                .await?
                .ok_or_else(|| invalid("Agent fork crash probe checkpoint is missing"))?;
            let result = execute_fork(&dependencies, &fixture, &checkpoint).await?;
            if result.replayed {
                return Err("Agent fork crash probe unexpectedly replayed its first commit".into());
            }
            json!({
                "schema": "a3s.cloud.agent-checkpoint-process-death-marker.v1",
                "mode": environment.mode.as_str(),
                "parentExecutionId": fixture.execution_id,
                "checkpointId": checkpoint.id,
                "checkpointDigest": checkpoint.object.digest,
                "childExecutionId": result.execution.id,
                "childOperationId": result.execution.operation_id,
            })
        }
    };
    process::publish_marker(&environment.marker, &marker)?;
    std::future::pending::<()>().await;
    Ok(())
}

async fn build_dependencies(fixture: &Fixture) -> TestResult<Dependencies> {
    let executor = connect_postgres(&fixture.postgres_url, 8).await?;
    let agents: Arc<dyn IAgentRepository> =
        Arc::new(PostgresAgentRepository::new(executor.clone()));
    let assets: Arc<dyn IAssetRepository> =
        Arc::new(PostgresAssetRepository::new(executor.clone()));
    let artifacts: Arc<dyn IHostedArtifactQueryPort> = Arc::new(HostedArtifactQueryService::new(
        Arc::new(PostgresBuildRunRepository::new(executor)),
    ));
    Ok(Dependencies {
        agents,
        objects: Arc::new(DurableCheckpointObjectStore::new(&fixture.objects_dir)?),
        assets,
        artifacts,
        providers: Arc::new(BuiltInAgentExecutionProviderRegistry::new().map_err(invalid)?),
    })
}

async fn materialize_root_checkpoint(
    dependencies: &Dependencies,
    fixture: &Fixture,
) -> TestResult<CapturedAgentExecutionCheckpoint> {
    let conversation = dependencies
        .agents
        .find_conversation(fixture.organization_id, fixture.conversation_id)
        .await?
        .ok_or_else(|| invalid("Agent checkpoint conversation is missing"))?;
    let execution = dependencies
        .agents
        .find_execution(fixture.organization_id, fixture.execution_id)
        .await?
        .ok_or_else(|| invalid("Agent checkpoint execution is missing"))?;
    let events = dependencies
        .agents
        .list_execution_trajectory_events(
            fixture.organization_id,
            fixture.execution_id,
            None,
            None,
            1_001,
        )
        .await?;
    Ok(
        AgentExecutionCheckpoint::capture(&conversation, &execution, &events)
            .map_err(|error| invalid(format!("could not materialize Agent checkpoint: {error}")))?,
    )
}

async fn execute_capture(
    dependencies: &Dependencies,
    fixture: &Fixture,
) -> TestResult<CaptureAgentExecutionCheckpointResult> {
    Ok(CaptureAgentExecutionCheckpointHandler::new(
        Arc::clone(&dependencies.agents),
        dependencies.objects.clone(),
    )
    .execute(
        CaptureAgentExecutionCheckpoint {
            organization_id: fixture.organization_id,
            execution_id: fixture.execution_id,
            resource_access: ResourceAccessEvaluator::organization_wide(),
            through_event_sequence: None,
            idempotency_key: CHECKPOINT_IDEMPOTENCY_KEY.into(),
            request_id: Uuid::new_v5(
                &fixture.execution_id.as_uuid(),
                b"a1.6-checkpoint-process-death",
            ),
        },
        context(),
    )
    .await?
    .map_err(|error| invalid(format!("could not capture Agent checkpoint: {error}")))?)
}

async fn execute_fork(
    dependencies: &Dependencies,
    fixture: &Fixture,
    checkpoint: &AgentExecutionCheckpoint,
) -> TestResult<ForkAgentExecutionResult> {
    Ok(ForkAgentExecutionHandler::new(
        Arc::clone(&dependencies.agents),
        dependencies.objects.clone(),
        Arc::clone(&dependencies.assets),
        Arc::clone(&dependencies.artifacts),
        dependencies.providers.clone(),
    )
    .execute(
        ForkAgentExecution {
            organization_id: fixture.organization_id,
            parent_execution_id: fixture.execution_id,
            checkpoint_id: checkpoint.id,
            resource_access: ResourceAccessEvaluator::organization_wide(),
            input: fork_input(),
            idempotency_key: FORK_IDEMPOTENCY_KEY.into(),
            request_id: Uuid::new_v5(&checkpoint.id.as_uuid(), b"a1.6-fork-process-death"),
            requested_at: checkpoint.captured_at + Duration::milliseconds(1),
        },
        context(),
    )
    .await?
    .map_err(|error| invalid(format!("could not fork Agent execution: {error}")))?)
}

fn fork_input() -> Value {
    json!({
        "prompt": "continue from the exact durable checkpoint",
        "recoveryProbe": true,
    })
}

fn require_marker_value(marker: &Value, field: &str, expected: &str) -> TestResult {
    let actual = marker
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("Agent checkpoint crash marker omitted {field}")))?;
    if actual != expected {
        return Err(format!(
            "Agent checkpoint crash marker {field} changed: expected {expected:?}, got {actual:?}"
        )
        .into());
    }
    Ok(())
}

fn require_marker_u64(marker: &Value, field: &str, expected: u64) -> TestResult {
    let actual = marker
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid(format!("Agent checkpoint crash marker omitted {field}")))?;
    if actual != expected {
        return Err(format!(
            "Agent checkpoint crash marker {field} changed: expected {expected}, got {actual}"
        )
        .into());
    }
    Ok(())
}

fn parse_execution_id(marker: &Value, field: &str) -> TestResult<AgentExecutionId> {
    let value = marker
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("Agent checkpoint crash marker omitted {field}")))?;
    Ok(AgentExecutionId::from_uuid(Uuid::parse_str(value)?))
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
