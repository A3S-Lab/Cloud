#[path = "secret_rotation_provider_crash/process.rs"]
mod process;

use self::process::{
    bound_docker_driver, docker_secret_memory_dir, docker_socket, managed_container_ids,
    require_exact_container, restart_isolated_provider, wait_for_provider_apply_marker,
    write_durable_file, CrashProbeProcess,
};
use crate::deployment_flow_support::{assert_secret_file_modes, assert_tree_excludes_plaintext};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandEnvelope, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{NodeId, OrganizationId};
use a3s_orm::PostgresExecutor;
use a3s_runtime::contract::{
    RuntimeActionRequest, RuntimeApplyRequest, RuntimeHealthState, RuntimeLogQuery,
    RuntimeObservation, RuntimeUnitSpec, RuntimeUnitState,
};
use a3s_runtime::{
    FileRuntimeStateStore, ManagedRuntimeClient, RuntimeClient, RuntimeRequestState,
    RuntimeStateStore,
};
use chrono::Utc;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub struct RecoveredProviderApply {
    acknowledgement: NodeCommandAck,
    runtime: Arc<dyn RuntimeClient>,
    state_directory: TempDir,
    secret_namespace_directory: PathBuf,
    provider_socket: String,
    namespace: String,
    node_id: NodeId,
    spec: RuntimeUnitSpec,
}

impl RecoveredProviderApply {
    pub fn acknowledgement(&self) -> &NodeCommandAck {
        &self.acknowledgement
    }

    pub async fn cleanup(self, sensitive_plaintexts: &[&str]) -> TestResult {
        self.runtime
            .remove(&RuntimeActionRequest {
                schema: RuntimeActionRequest::SCHEMA.into(),
                request_id: format!("secret-rotation-crash-cleanup-{}", Uuid::now_v7()),
                unit_id: self.spec.unit_id.clone(),
                generation: self.spec.generation,
                deadline_at_ms: None,
            })
            .await?;
        let remaining = managed_container_ids(
            &self.provider_socket,
            &self.namespace,
            self.node_id,
            &self.spec,
        )
        .await?;
        if !remaining.is_empty() {
            return Err("Secret-rotation provider recovery left a managed container".into());
        }
        match tokio::fs::remove_dir(&self.secret_namespace_directory).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        assert_tree_excludes_plaintext(self.state_directory.path(), sensitive_plaintexts)?;
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn recover_provider_apply(
    executor: &PostgresExecutor,
    postgres_url: &str,
    organization_id: OrganizationId,
    node_id: NodeId,
    security_state_dir: &Path,
    command: NodeCommandEnvelope,
    sensitive_plaintexts: &[&str],
) -> TestResult<RecoveredProviderApply> {
    if std::env::var("A3S_CLOUD_TEST_DOCKER").as_deref() != Ok("1") {
        return Err("Secret-rotation provider crash gate requires real Docker".into());
    }
    command.validate()?;
    let NodeCommandPayload::RuntimeApply { request } = &command.payload else {
        return Err("Secret-rotation provider crash command is not Runtime apply".into());
    };
    let request = request.clone();
    let provider_socket = docker_socket();
    let state_directory = tempfile::tempdir()?;
    let request_path = state_directory.path().join("runtime-apply-request.json");
    let marker_path = state_directory.path().join("provider-apply-complete.json");
    let namespace = format!(
        "cloud-rotation-{}",
        &Uuid::now_v7().simple().to_string()[..12]
    );
    write_durable_file(&request_path, &serde_json::to_vec(&request)?)?;

    let mut crash_probe = CrashProbeProcess::start(
        &std::env::current_exe()?,
        postgres_url,
        organization_id,
        node_id,
        state_directory.path(),
        &request_path,
        &marker_path,
        security_state_dir,
        &namespace,
    )?;
    let provider_observation =
        wait_for_provider_apply_marker(&marker_path, &mut crash_probe).await?;
    provider_observation.validate_against(&request.spec)?;
    if provider_observation.state != RuntimeUnitState::Running
        || provider_observation
            .health
            .as_ref()
            .map(|health| health.state)
            != Some(RuntimeHealthState::Healthy)
    {
        return Err(
            "Secret-rotation provider crash probe did not reach healthy Runtime apply".into(),
        );
    }
    let provider_resource_id = provider_observation
        .provider_resource_id
        .clone()
        .ok_or("Secret-rotation provider crash probe omitted its container identity")?;

    let state = Arc::new(FileRuntimeStateStore::new(
        state_directory.path().join("runtime"),
    ));
    require_request_state(&state, &request, RuntimeRequestState::Pending, None).await?;
    require_exact_container(
        &provider_socket,
        &namespace,
        node_id,
        &request.spec,
        &provider_resource_id,
    )
    .await?;

    restart_isolated_provider(&provider_socket).await?;
    if let Some(status) = crash_probe.try_wait()? {
        return Err(format!(
            "Secret-rotation provider crash probe exited during provider restart with {status}"
        )
        .into());
    }
    require_exact_container(
        &provider_socket,
        &namespace,
        node_id,
        &request.spec,
        &provider_resource_id,
    )
    .await?;

    let crash_status = crash_probe.kill_and_wait()?;
    if crash_status.success() {
        return Err("Secret-rotation provider crash probe exited successfully".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if crash_status.signal() != Some(9) {
            return Err(format!(
                "Secret-rotation provider crash probe exited with {crash_status} instead of SIGKILL"
            )
            .into());
        }
    }
    require_request_state(&state, &request, RuntimeRequestState::Pending, None).await?;

    let driver = bound_docker_driver(
        executor,
        organization_id,
        node_id,
        security_state_dir,
        &namespace,
    )
    .await?;
    let runtime: Arc<dyn RuntimeClient> =
        Arc::new(ManagedRuntimeClient::new(state.clone(), driver));
    let recovered_observation = runtime.apply(&request).await?;
    recovered_observation.validate_against(&request.spec)?;
    if recovered_observation.provider_resource_id.as_deref() != Some(&provider_resource_id) {
        return Err("Secret-rotation recovery did not reattach the original container".into());
    }
    require_request_state(
        &state,
        &request,
        RuntimeRequestState::Completed,
        Some(&recovered_observation),
    )
    .await?;
    if runtime.apply(&request).await? != recovered_observation {
        return Err(
            "completed Secret-rotation Runtime apply replay changed its observation".into(),
        );
    }
    require_exact_container(
        &provider_socket,
        &namespace,
        node_id,
        &request.spec,
        &provider_resource_id,
    )
    .await?;

    let secret_namespace_directory = docker_secret_memory_dir().join(&namespace);
    assert_secret_file_modes(&secret_namespace_directory, &[0o400])?;
    require_redacted_secret_logs(runtime.as_ref(), &request.spec, sensitive_plaintexts).await?;
    assert_tree_excludes_plaintext(state_directory.path(), sensitive_plaintexts)?;

    let acknowledgement = NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: command.command_id,
        lease_id: command.lease_id,
        node_id: command.node_id,
        sequence: command.sequence,
        payload_digest: command.payload_digest.clone(),
        completed_at: Utc::now(),
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::RuntimeApplied {
                observation: Box::new(recovered_observation),
            }),
        },
    };
    acknowledgement.validate_against(&command)?;

    Ok(RecoveredProviderApply {
        acknowledgement,
        runtime,
        state_directory,
        secret_namespace_directory,
        provider_socket,
        namespace,
        node_id,
        spec: request.spec,
    })
}

pub async fn run_provider_crash_probe() -> TestResult {
    process::run_provider_crash_probe().await
}

async fn require_request_state(
    state: &Arc<FileRuntimeStateStore>,
    request: &RuntimeApplyRequest,
    expected_state: RuntimeRequestState,
    expected_observation: Option<&RuntimeObservation>,
) -> TestResult {
    let receipt = state
        .load_request(&request.spec.unit_id, &request.request_id)
        .await?;
    if receipt.state != expected_state || receipt.observation.as_ref() != expected_observation {
        return Err(format!(
            "Secret-rotation Runtime receipt was {:?} with observation {:?}, expected {expected_state:?}",
            receipt.state, receipt.observation
        )
        .into());
    }
    Ok(())
}

async fn require_redacted_secret_logs(
    runtime: &dyn RuntimeClient,
    spec: &RuntimeUnitSpec,
    sensitive_plaintexts: &[&str],
) -> TestResult {
    let query = RuntimeLogQuery {
        schema: RuntimeLogQuery::SCHEMA.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        cursor: None,
        limit: 32,
        stream: None,
    };
    for attempt in 0..40 {
        let chunks = runtime.logs(&query).await?;
        let no_plaintext = chunks.iter().all(|chunk| {
            sensitive_plaintexts
                .iter()
                .all(|plaintext| !chunk.data.contains(plaintext))
        });
        let environment_redacted = chunks
            .iter()
            .any(|chunk| chunk.data.contains("env-secret=[REDACTED]"));
        let file_redacted = chunks
            .iter()
            .any(|chunk| chunk.data.contains("file-secret=[REDACTED]"));
        if no_plaintext && environment_redacted && file_redacted {
            return Ok(());
        }
        if attempt < 39 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    Err("recovered Secret-rotation provider logs were not completely redacted".into())
}
