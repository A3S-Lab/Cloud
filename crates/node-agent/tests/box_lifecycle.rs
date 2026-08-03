#![cfg(unix)]

#[path = "box_lifecycle/artifact.rs"]
mod artifact;
#[path = "box_lifecycle/resource_claim.rs"]
mod resource_claim;

use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use a3s_box_runtime::BoxStateStore;
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandEnvelope, NodeCommandMetadata, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult,
};
#[cfg(target_os = "linux")]
use a3s_cloud_node_agent::{build_box_runtime_provider, BoxRuntimeConfig, BoxRuntimeIsolation};
use a3s_cloud_node_agent::{
    CommandExecutor, FileCommandJournal, JournalDecision, NodeArtifactManager,
};
use a3s_runtime::contract::{
    ArtifactRef, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy, RuntimeActionRequest,
    RuntimeApplyRequest, RuntimeInspection, RuntimeLogQuery, RuntimeNetworkSpec,
    RuntimeObservation, RuntimeProcessSpec, RuntimeUnitClass, RuntimeUnitSpec, RuntimeUnitState,
};
use a3s_runtime::RuntimeClient;
use chrono::{Duration as ChronoDuration, Utc};
use tempfile::TempDir;
use uuid::Uuid;

type TestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test]
#[ignore = "requires A3S_CLOUD_TEST_BOX=1 on the dedicated real Box provider runner"]
async fn real_box_recovers_journal_gaps_generation_and_resource_claims() -> TestResult<()> {
    require_real_box_gate()?;
    let home = dedicated_box_home()?;
    let runtime_state = tempfile::tempdir()?;
    let node_state = tempfile::tempdir()?;
    let artifact = conformance_artifact()?;
    let node_id = Uuid::now_v7();
    let journal = FileCommandJournal::new(node_state.path(), node_id)?;
    let artifacts = artifact::manager(node_state.path(), node_id)?;

    prove_task_apply_gap_recovery(
        &home,
        &runtime_state,
        &journal,
        node_id,
        artifact.clone(),
        artifacts.clone(),
    )
    .await?;
    prove_service_generation_lifecycle(
        &home,
        &runtime_state,
        &journal,
        node_id,
        artifact.clone(),
        artifacts.clone(),
    )
    .await?;
    resource_claim::prove_resource_claim_lifecycle(
        &home,
        &runtime_state,
        &journal,
        node_id,
        artifact,
        artifacts,
    )
    .await?;

    verify_box_state_and_remove_fixture_files(&home)?;
    Ok(())
}

async fn prove_task_apply_gap_recovery(
    home: &Path,
    runtime_state: &TempDir,
    journal: &FileCommandJournal,
    node_id: Uuid,
    artifact: ArtifactRef,
    artifacts: Arc<NodeArtifactManager>,
) -> TestResult<()> {
    let aggregate_id = Uuid::now_v7();
    let spec = runtime_spec(
        artifact,
        format!("cloud-box-task-{}", Uuid::now_v7().simple()),
        1,
        RuntimeUnitClass::Task,
        "printf 'cloud-box-task-ready\\n'",
    )?;
    let request = apply_request("cloud-box-task-apply", spec.clone());
    let apply = command(
        node_id,
        aggregate_id,
        1,
        NodeCommandPayload::RuntimeApply {
            request: Box::new(request.clone()),
            resource_claim: None,
        },
    )?;
    if !matches!(
        journal.begin(apply.clone()).await?,
        JournalDecision::Execute
    ) {
        return Err(invalid("new Cloud Task command did not enter execution").into());
    }

    // This is the durable crash boundary: Runtime and Box have completed the
    // apply, but the Agent has not persisted the command completion yet.
    let first_runtime = runtime(home, runtime_state.path(), artifacts.clone()).await?;
    let first = first_runtime.apply(&request).await?;
    if first.state != RuntimeUnitState::Succeeded {
        return Err(invalid(format!(
            "short-lived Box Task did not succeed before Agent restart: {:?}",
            first.state
        ))
        .into());
    }
    drop(first_runtime);

    let recovered_runtime = runtime(home, runtime_state.path(), artifacts.clone()).await?;
    let executor = CommandExecutor::runtime_only(journal.clone(), recovered_runtime.clone())
        .with_artifacts(artifacts);
    let mut rebound = apply;
    rebound.lease_id = Uuid::now_v7();
    let acknowledgement = executor.execute(rebound).await?;
    let recovered = applied_observation(&acknowledgement)?;
    if recovered.state != RuntimeUnitState::Succeeded
        || recovered.provider_resource_id != first.provider_resource_id
    {
        return Err(invalid("Cloud replay did not preserve the exact completed Box Task").into());
    }
    wait_for_log(&*recovered_runtime, &spec, "cloud-box-task-ready").await?;

    let removed = executor
        .execute(command(
            node_id,
            aggregate_id,
            2,
            NodeCommandPayload::RuntimeRemove {
                request: action_request("cloud-box-task-remove", &spec),
            },
        )?)
        .await?;
    expect_removed(&removed)?;
    expect_not_found(&*recovered_runtime, &spec.unit_id).await?;
    Ok(())
}

async fn prove_service_generation_lifecycle(
    home: &Path,
    runtime_state: &TempDir,
    journal: &FileCommandJournal,
    node_id: Uuid,
    artifact: ArtifactRef,
    artifacts: Arc<NodeArtifactManager>,
) -> TestResult<()> {
    let aggregate_id = Uuid::now_v7();
    let unit_id = format!("cloud-box-service-{}", Uuid::now_v7().simple());
    let first_spec = runtime_spec(
        artifact.clone(),
        unit_id.clone(),
        1,
        RuntimeUnitClass::Service,
        "printf 'cloud-box-service-v1\\n'; exec sleep 3600",
    )?;
    let first_request = apply_request("cloud-box-service-v1-apply", first_spec.clone());
    let first_command = command(
        node_id,
        aggregate_id,
        3,
        NodeCommandPayload::RuntimeApply {
            request: Box::new(first_request.clone()),
            resource_claim: None,
        },
    )?;
    if !matches!(
        journal.begin(first_command.clone()).await?,
        JournalDecision::Execute
    ) {
        return Err(invalid("new Cloud Service command did not enter execution").into());
    }
    let first_runtime = runtime(home, runtime_state.path(), artifacts.clone()).await?;
    let first = first_runtime.apply(&first_request).await?;
    if first.state != RuntimeUnitState::Running {
        return Err(invalid("first Box Service generation did not become running").into());
    }
    let first_provider_id = first
        .provider_resource_id
        .clone()
        .ok_or_else(|| invalid("first Box Service generation omitted provider identity"))?;
    drop(first_runtime);

    let recovered_runtime = runtime(home, runtime_state.path(), artifacts.clone()).await?;
    let recovered_executor =
        CommandExecutor::runtime_only(journal.clone(), recovered_runtime.clone())
            .with_artifacts(artifacts);
    let mut rebound = first_command;
    rebound.lease_id = Uuid::now_v7();
    let recovered_first = recovered_executor.execute(rebound).await?;
    if applied_observation(&recovered_first)?
        .provider_resource_id
        .as_ref()
        != Some(&first_provider_id)
    {
        return Err(invalid("Cloud replay did not preserve the exact running Box Service").into());
    }

    let second_spec = runtime_spec(
        artifact,
        unit_id.clone(),
        2,
        RuntimeUnitClass::Service,
        "printf 'cloud-box-service-v2\\n'; exec sleep 3600",
    )?;
    let second_acknowledgement = recovered_executor
        .execute(command(
            node_id,
            aggregate_id,
            4,
            NodeCommandPayload::RuntimeApply {
                request: Box::new(apply_request(
                    "cloud-box-service-v2-apply",
                    second_spec.clone(),
                )),
                resource_claim: None,
            },
        )?)
        .await?;
    let second = applied_observation(&second_acknowledgement)?;
    let second_provider_id = second
        .provider_resource_id
        .as_ref()
        .ok_or_else(|| invalid("second Box Service generation omitted provider identity"))?;
    if second.state != RuntimeUnitState::Running || second_provider_id == &first_provider_id {
        return Err(invalid(
            "Cloud generation update did not replace the exact Box Service resource",
        )
        .into());
    }
    wait_for_log(&*recovered_runtime, &second_spec, "cloud-box-service-v2").await?;

    let inspection = recovered_executor
        .execute(command(
            node_id,
            aggregate_id,
            5,
            NodeCommandPayload::RuntimeInspect {
                unit_id: unit_id.clone(),
                generation: 2,
            },
        )?)
        .await?;
    expect_inspected_generation(&inspection, 2)?;

    let stopped = recovered_executor
        .execute(command(
            node_id,
            aggregate_id,
            6,
            NodeCommandPayload::RuntimeStop {
                request: action_request("cloud-box-service-stop", &second_spec),
            },
        )?)
        .await?;
    expect_stopped(&stopped)?;

    let removed = recovered_executor
        .execute(command(
            node_id,
            aggregate_id,
            7,
            NodeCommandPayload::RuntimeRemove {
                request: action_request("cloud-box-service-remove", &second_spec),
            },
        )?)
        .await?;
    expect_removed(&removed)?;
    expect_not_found(&*recovered_runtime, &unit_id).await?;
    Ok(())
}

#[cfg(target_os = "linux")]
async fn runtime(
    home: &Path,
    state_root: &Path,
    artifacts: Arc<NodeArtifactManager>,
) -> TestResult<Arc<dyn RuntimeClient>> {
    let provider = build_box_runtime_provider(
        &BoxRuntimeConfig {
            home_dir: home.to_path_buf(),
            secret_root: home.join("runtime-secrets"),
            isolation: BoxRuntimeIsolation::Sandbox,
            control_timeout_ms: 120_000,
            task_poll_interval_ms: 25,
            sev_snp: None,
        },
        state_root,
    )?;
    Ok(provider.into_artifact_bound_client(artifacts).await?)
}

#[cfg(not(target_os = "linux"))]
async fn runtime(
    _home: &Path,
    _state_root: &Path,
    _artifacts: Arc<NodeArtifactManager>,
) -> TestResult<Arc<dyn RuntimeClient>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "real Box lifecycle validation requires Linux",
    )
    .into())
}

fn runtime_spec(
    artifact: ArtifactRef,
    unit_id: String,
    generation: u64,
    class: RuntimeUnitClass,
    script: &str,
) -> TestResult<RuntimeUnitSpec> {
    let spec = RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id,
        generation,
        class,
        artifact,
        process: RuntimeProcessSpec {
            command: vec!["/bin/sh".into(), "-c".into()],
            args: vec![script.into()],
            working_directory: Some("/".into()),
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
            memory_bytes: 128 * 1024 * 1024,
            pids: 64,
            ephemeral_storage_bytes: None,
            execution_timeout_ms: (class == RuntimeUnitClass::Task).then_some(10_000),
        },
        isolation: IsolationLevel::Sandbox,
        health: None,
        restart: if class == RuntimeUnitClass::Task {
            RestartPolicy::Never
        } else {
            RestartPolicy::Always
        },
        outputs: Vec::new(),
        semantics_profile_digest: None,
    };
    spec.validate().map_err(invalid)?;
    Ok(spec)
}

fn conformance_artifact() -> TestResult<ArtifactRef> {
    let image = std::env::var("A3S_BOX_RUNTIME_CONFORMANCE_IMAGE")?;
    let (repository, digest_hex) = image
        .rsplit_once("@sha256:")
        .ok_or_else(|| invalid("Box conformance image is not digest-pinned"))?;
    if repository.is_empty()
        || digest_hex.len() != 64
        || !digest_hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid("Box conformance image digest is invalid").into());
    }
    let digest = format!("sha256:{digest_hex}");
    let artifact = ArtifactRef {
        uri: format!("oci://{repository}@{digest}"),
        digest,
        media_type: std::env::var("A3S_BOX_RUNTIME_CONFORMANCE_MEDIA_TYPE")?,
    };
    artifact.validate().map_err(invalid)?;
    Ok(artifact)
}

fn apply_request(request_id: &str, spec: RuntimeUnitSpec) -> RuntimeApplyRequest {
    RuntimeApplyRequest {
        schema: RuntimeApplyRequest::SCHEMA.into(),
        request_id: request_id.into(),
        deadline_at_ms: None,
        spec,
    }
}

fn action_request(request_id: &str, spec: &RuntimeUnitSpec) -> RuntimeActionRequest {
    RuntimeActionRequest {
        schema: RuntimeActionRequest::SCHEMA.into(),
        request_id: request_id.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        deadline_at_ms: None,
    }
}

fn command(
    node_id: Uuid,
    aggregate_id: Uuid,
    sequence: u64,
    payload: NodeCommandPayload,
) -> TestResult<NodeCommandEnvelope> {
    let issued_at = Utc::now() - ChronoDuration::seconds(1);
    NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id: Uuid::now_v7(),
            lease_id: Uuid::now_v7(),
            node_id,
            sequence,
            aggregate_id,
            issued_at,
            not_after: issued_at + ChronoDuration::minutes(30),
            correlation_id: Uuid::now_v7(),
        },
        payload,
    )
    .map_err(|error| invalid(error).into())
}

fn applied_observation(acknowledgement: &NodeCommandAck) -> TestResult<&RuntimeObservation> {
    match succeeded_result(acknowledgement)? {
        NodeCommandResult::RuntimeApplied { observation } => Ok(observation),
        result => Err(invalid(format!("unexpected apply result: {result:?}")).into()),
    }
}

fn expect_inspected_generation(
    acknowledgement: &NodeCommandAck,
    generation: u64,
) -> TestResult<()> {
    match succeeded_result(acknowledgement)? {
        NodeCommandResult::RuntimeInspected {
            inspection: RuntimeInspection::Found { observation, .. },
        } if observation.generation == generation => Ok(()),
        result => Err(invalid(format!("unexpected inspect result: {result:?}")).into()),
    }
}

fn expect_stopped(acknowledgement: &NodeCommandAck) -> TestResult<()> {
    match succeeded_result(acknowledgement)? {
        NodeCommandResult::RuntimeStopped {
            inspection: RuntimeInspection::Found { observation, .. },
        } if observation.state == RuntimeUnitState::Stopped => Ok(()),
        result => Err(invalid(format!("unexpected stop result: {result:?}")).into()),
    }
}

fn expect_removed(acknowledgement: &NodeCommandAck) -> TestResult<()> {
    match succeeded_result(acknowledgement)? {
        NodeCommandResult::RuntimeRemoved { removal } if !removal.unit_id.is_empty() => Ok(()),
        result => Err(invalid(format!("unexpected remove result: {result:?}")).into()),
    }
}

fn succeeded_result(acknowledgement: &NodeCommandAck) -> TestResult<&NodeCommandResult> {
    match &acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => Ok(result),
        outcome => Err(invalid(format!("Cloud command did not succeed: {outcome:?}")).into()),
    }
}

async fn expect_not_found(runtime: &dyn RuntimeClient, unit_id: &str) -> TestResult<()> {
    if matches!(
        runtime.inspect(unit_id).await?,
        RuntimeInspection::NotFound { .. }
    ) {
        Ok(())
    } else {
        Err(invalid(format!(
            "removed Box Runtime unit {unit_id:?} remained inspectable"
        ))
        .into())
    }
}

async fn wait_for_log(
    runtime: &dyn RuntimeClient,
    spec: &RuntimeUnitSpec,
    expected: &str,
) -> TestResult<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let chunks = runtime
            .logs(&RuntimeLogQuery {
                schema: RuntimeLogQuery::SCHEMA.into(),
                unit_id: spec.unit_id.clone(),
                generation: spec.generation,
                cursor: None,
                limit: 100,
                stream: None,
            })
            .await?;
        if chunks.iter().any(|chunk| chunk.data.contains(expected)) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(invalid(format!(
                "Box logs omitted Cloud lifecycle marker {expected:?}"
            ))
            .into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn require_real_box_gate() -> TestResult<()> {
    if std::env::var("A3S_CLOUD_TEST_BOX").as_deref() != Ok("1") {
        return Err(invalid("dedicated Box gate did not set A3S_CLOUD_TEST_BOX=1").into());
    }
    Ok(())
}

fn dedicated_box_home() -> TestResult<PathBuf> {
    let configured = PathBuf::from(
        std::env::var_os("A3S_HOME")
            .ok_or_else(|| invalid("dedicated Box gate did not configure an absolute A3S_HOME"))?,
    );
    if !configured.is_absolute() {
        return Err(invalid("dedicated Box gate A3S_HOME is not absolute").into());
    }
    let canonical = configured.canonicalize()?;
    if canonical != configured {
        return Err(invalid("dedicated Box gate A3S_HOME is not canonical").into());
    }
    Ok(configured)
}

#[cfg(target_os = "linux")]
fn verify_box_state_and_remove_fixture_files(home: &Path) -> TestResult<()> {
    let state_path = home.join("boxes.json");
    let store = BoxStateStore::load_readonly(&state_path)?;
    if !store.records().is_empty() {
        let retained = store
            .records()
            .iter()
            .map(|record| format!("{}:{}", record.id, record.status))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(invalid(format!(
            "Box retained managed execution state after Cloud cleanup: {retained}"
        ))
        .into());
    }

    for path in [
        state_path,
        home.join("boxes.json.lock"),
        home.join("boxes.json.tmp"),
    ] {
        remove_file_if_exists(&path)?;
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn verify_box_state_and_remove_fixture_files(_home: &Path) -> TestResult<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "real Box lifecycle validation requires Linux",
    )
    .into())
}

#[cfg(target_os = "linux")]
fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn verifies_empty_box_state_before_fixture_housekeeping() -> TestResult<()> {
    let home = tempfile::tempdir()?;
    let state_path = home.path().join("boxes.json");
    let lock_path = home.path().join("boxes.json.lock");
    let temporary_path = home.path().join("boxes.json.tmp");
    std::fs::write(&state_path, b"[]")?;
    std::fs::write(&lock_path, b"")?;
    std::fs::write(&temporary_path, b"")?;

    verify_box_state_and_remove_fixture_files(home.path())?;

    assert!(!state_path.exists());
    assert!(!lock_path.exists());
    assert!(!temporary_path.exists());
    Ok(())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
