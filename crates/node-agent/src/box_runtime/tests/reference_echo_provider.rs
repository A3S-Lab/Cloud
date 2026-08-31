use crate::agent_provider_harness::{
    self, AgentProviderHarnessError, AgentProviderHarnessTransport,
    HttpAgentProviderHarnessTransport,
};
use crate::{
    ArtifactConfig, BoxRuntimeConfig, BoxRuntimeIsolation, CommandExecutor, DownloadedNodeArtifact,
    FileCommandJournal, NodeArtifactManager, NodeArtifactTransport, NodeControlClientError,
};
use a3s_box_runtime::BoxStateStore;
use a3s_cloud_contracts::{
    artifact_uri, AgentProviderCommandV1, AgentProviderEventPageRequestV1, AgentProviderProfile,
    AgentProviderRunIdentityV1, AgentProviderRunStartV1, AgentProviderRunStateV1,
    AgentProviderSemanticEventV1, NodeAgentProviderRuntimeBindingV1, NodeArtifactDownloadRequest,
    NodeArtifactUploadReceipt, NodeArtifactUploadRequest, NodeCommandAck, NodeCommandEnvelope,
    NodeCommandMetadata, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
    NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
use a3s_runtime::contract::{
    ArtifactRef, HealthProbe, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy,
    RuntimeActionRequest, RuntimeApplyRequest, RuntimeHealthCheck, RuntimeHealthState,
    RuntimeInspection, RuntimeMount, RuntimeMountSource, RuntimeNetworkSpec, RuntimePort,
    RuntimeProcessSpec, RuntimeUnitClass, RuntimeUnitSpec, RuntimeUnitState, TransportProtocol,
};
use a3s_runtime::RuntimeClient;
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[path = "reference_echo_provider/approval.rs"]
mod approval;

const REFERENCE_ECHO_PROVIDER_PROFILE_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/a1.3/reference-echo-provider-profile.acl"
));
const PROVIDER_BINARY_NAME: &str = "a3s-reference-echo-provider";
const PROVIDER_MOUNT: &str = "/opt/a3s-reference-provider";
const PROVIDER_PORT: u16 = 49_152;
const MAX_PROVIDER_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;

type GateResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
struct ProviderArtifactTransport {
    artifact: ArtifactRef,
    archive: Vec<u8>,
    downloads: AtomicUsize,
}

#[async_trait]
impl NodeArtifactTransport for ProviderArtifactTransport {
    async fn download(
        &self,
        request: &NodeArtifactDownloadRequest,
        destination: &Path,
        maximum_bytes: u64,
    ) -> Result<DownloadedNodeArtifact, NodeControlClientError> {
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        if request
            .artifact()
            .map_err(NodeControlClientError::Invalid)?
            != self.artifact
            || self.archive.len() as u64 > maximum_bytes
        {
            return Err(NodeControlClientError::Invalid(
                "real provider gate requested an unexpected Artifact".into(),
            ));
        }
        tokio::fs::write(destination, &self.archive)
            .await
            .map_err(|error| NodeControlClientError::Transport(error.to_string()))?;
        self.downloads.fetch_add(1, Ordering::SeqCst);
        Ok(DownloadedNodeArtifact {
            size_bytes: self.archive.len() as u64,
        })
    }

    async fn upload(
        &self,
        _request: &NodeArtifactUploadRequest,
        _source: &Path,
    ) -> Result<NodeArtifactUploadReceipt, NodeControlClientError> {
        Err(NodeControlClientError::Invalid(
            "reference provider gate has no output Artifact".into(),
        ))
    }
}

#[tokio::test]
#[ignore = "requires the dedicated Linux A3S Box provider runner"]
async fn real_box_hosts_restarts_and_cleans_the_reference_echo_provider() -> GateResult {
    require_gate()?;
    let home = PathBuf::from(std::env::var("A3S_HOME")?).canonicalize()?;
    let provider_binary =
        PathBuf::from(std::env::var("A3S_CLOUD_REFERENCE_ECHO_PROVIDER_PATH")?).canonicalize()?;
    let provider_bytes = std::fs::read(&provider_binary)?;
    if provider_bytes.is_empty() || provider_bytes.len() as u64 > MAX_PROVIDER_ARTIFACT_BYTES {
        return Err(invalid("reference provider binary has an invalid bounded size").into());
    }
    let provider_archive = executable_archive(PROVIDER_BINARY_NAME, &provider_bytes)?;
    if provider_archive.len() as u64 > MAX_PROVIDER_ARTIFACT_BYTES {
        return Err(invalid("reference provider archive exceeds the gate bound").into());
    }
    let provider_artifact = cloud_artifact(&provider_archive)?;
    let runtime_state = tempfile::tempdir()?;
    let node_state = tempfile::tempdir()?;
    let node_id = Uuid::now_v7();
    let execution_id = Uuid::now_v7();
    let transport = Arc::new(ProviderArtifactTransport {
        artifact: provider_artifact.clone(),
        archive: provider_archive,
        downloads: AtomicUsize::new(0),
    });
    let artifacts = Arc::new(
        NodeArtifactManager::new(
            node_state.path(),
            ArtifactConfig {
                max_blob_bytes: MAX_PROVIDER_ARTIFACT_BYTES,
                max_entries: 16,
                max_file_bytes: MAX_PROVIDER_ARTIFACT_BYTES,
                max_expanded_bytes: MAX_PROVIDER_ARTIFACT_BYTES,
            },
            node_id,
            transport.clone(),
        )
        .map_err(invalid)?,
    );
    let provider = super::super::build_box_runtime_provider(
        &BoxRuntimeConfig {
            home_dir: home.clone(),
            secret_root: home.join("runtime-secrets").canonicalize()?,
            isolation: BoxRuntimeIsolation::Sandbox,
            control_timeout_ms: 120_000,
            task_poll_interval_ms: 25,
            sev_snp: None,
        },
        runtime_state.path(),
    )?;
    let runtime = provider
        .into_artifact_bound_client(artifacts.clone())
        .await?;
    let journal = FileCommandJournal::new(node_state.path(), node_id)?;
    let provider_harness = Arc::new(HttpAgentProviderHarnessTransport::new()?);
    let provider_transport: Arc<dyn AgentProviderHarnessTransport> = provider_harness.clone();
    let executor = CommandExecutor::runtime_only(journal, runtime.clone())
        .with_artifacts(artifacts)
        .with_agent_provider_harness(provider_transport);

    let profile =
        AgentProviderProfile::parse_acl(REFERENCE_ECHO_PROVIDER_PROFILE_ACL).map_err(invalid)?;
    let spec = provider_runtime_spec(provider_artifact)?;
    let apply = command(
        node_id,
        execution_id,
        1,
        NodeCommandPayload::RuntimeApply {
            request: Box::new(RuntimeApplyRequest {
                schema: RuntimeApplyRequest::SCHEMA.into(),
                request_id: format!("reference-provider-apply-{}", Uuid::now_v7()),
                deadline_at_ms: None,
                spec: spec.clone(),
            }),
            resource_claim: None,
        },
    )?;
    let applied = executor.execute(apply).await?;
    let first_observation = applied_observation(&applied)?;
    if first_observation.state != RuntimeUnitState::Running
        || first_observation
            .health
            .as_ref()
            .is_none_or(|health| health.state != RuntimeHealthState::Healthy)
    {
        return Err(invalid("reference provider Box Service did not become healthy").into());
    }
    let provider_resource_id = first_observation
        .provider_resource_id
        .clone()
        .ok_or_else(|| invalid("reference provider omitted its Box resource identity"))?;
    let first_started_at_ms = first_observation
        .started_at_ms
        .ok_or_else(|| invalid("reference provider omitted its process start time"))?;
    let identity = AgentProviderRunIdentityV1::new(
        profile.digest().into(),
        profile.capability_digest().into(),
        format!("sha256:{}", "a".repeat(64)),
        "reference-box-conversation".into(),
        "reference-box-run".into(),
    )?;
    let binding = NodeAgentProviderRuntimeBindingV1 {
        schema: NodeAgentProviderRuntimeBindingV1::SCHEMA.into(),
        execution_id,
        workload_id: Uuid::now_v7(),
        workload_revision_id: Uuid::now_v7(),
        deployment_id: Uuid::now_v7(),
        replica_id: Uuid::now_v7(),
        runtime_unit_id: spec.unit_id.clone(),
        runtime_generation: spec.generation,
        runtime_spec_digest: spec.digest()?,
        service_port_name: "agent".into(),
        provider_profile_acl: profile.canonical_acl().into(),
        provider_profile_digest: profile.digest().into(),
        provider_run_identity: identity.clone(),
    };
    let start = AgentProviderCommandV1::Start {
        request: AgentProviderRunStartV1::new(
            "reference-box:start".into(),
            identity.clone(),
            "Return one reference output.".into(),
        )?,
    };
    let start_envelope = command(
        node_id,
        execution_id,
        2,
        NodeCommandPayload::AgentProviderCommand {
            binding: Box::new(binding.clone()),
            command: Box::new(start.clone()),
        },
    )?;
    let started = executor.execute(start_envelope.clone()).await?;
    let NodeCommandResult::AgentProviderCommandAccepted { receipt } = succeeded_result(&started)?
    else {
        return Err(invalid("reference provider Start returned another result kind").into());
    };
    receipt.validate_for(&profile, &start).map_err(invalid)?;
    if receipt.state != AgentProviderRunStateV1::Executing || receipt.replayed {
        return Err(invalid("reference provider did not accept one fresh Start").into());
    }

    let endpoint =
        agent_provider_harness::resolve_runtime_endpoint(runtime.as_ref(), &binding).await?;
    let first_page_request = AgentProviderEventPageRequestV1 {
        schema: AgentProviderEventPageRequestV1::SCHEMA.into(),
        identity: identity.clone(),
        after_event_sequence: None,
        limit: 64,
    };
    let first_page = provider_harness
        .event_page(
            &endpoint,
            &binding,
            &first_page_request,
            Duration::from_secs(5),
        )
        .await?;
    if first_page.events.len() != 1
        || !matches!(
            &first_page.events[0].event,
            AgentProviderSemanticEventV1::ModelOutput { text }
                if text == "reference harness output"
        )
        || first_page.next_after_event_sequence != Some(0)
    {
        return Err(invalid("reference provider omitted its exact semantic event").into());
    }
    let settled_page = provider_harness
        .event_page(
            &endpoint,
            &binding,
            &AgentProviderEventPageRequestV1 {
                after_event_sequence: Some(0),
                ..first_page_request
            },
            Duration::from_secs(5),
        )
        .await?;
    if !settled_page.events.is_empty() || settled_page.next_after_event_sequence != Some(0) {
        return Err(invalid("reference provider event cursor did not settle exactly").into());
    }

    let mut redelivered_start = start_envelope;
    redelivered_start.lease_id = Uuid::now_v7();
    let replayed_start = executor.execute(redelivered_start).await?;
    if replayed_start.outcome != started.outcome {
        return Err(invalid("Fleet journal changed the replayed provider receipt").into());
    }

    let approval_matrix = approval::exercise_approval_matrix(
        &executor,
        provider_harness.as_ref(),
        runtime.as_ref(),
        &profile,
        &binding,
        node_id,
        3,
    )
    .await?;

    kill_box_process(&home, &provider_resource_id).await?;
    let recovered_started_at_ms = wait_for_restarted_provider(
        runtime.as_ref(),
        &spec,
        &provider_resource_id,
        first_started_at_ms,
    )
    .await?;
    let recovered_endpoint = agent_provider_harness::resolve_runtime_endpoint(
        runtime.as_ref(),
        &approval_matrix.pending_restart_binding,
    )
    .await?;
    match provider_harness
        .event_page(
            &recovered_endpoint,
            &approval_matrix.pending_restart_binding,
            &AgentProviderEventPageRequestV1 {
                schema: AgentProviderEventPageRequestV1::SCHEMA.into(),
                identity: approval_matrix
                    .pending_restart_binding
                    .provider_run_identity
                    .clone(),
                after_event_sequence: Some(0),
                limit: 64,
            },
            Duration::from_secs(5),
        )
        .await
    {
        Err(AgentProviderHarnessError::Rejected { status }) if status.as_u16() == 400 => {}
        result => {
            return Err(invalid(format!(
            "restarted non-recoverable provider retained unexpected pending approval: {result:?}"
        ))
            .into())
        }
    }

    let stopped = executor
        .execute(command(
            node_id,
            execution_id,
            approval_matrix.next_sequence,
            NodeCommandPayload::RuntimeStop {
                request: action_request("stop", &spec),
            },
        )?)
        .await?;
    let NodeCommandResult::RuntimeStopped {
        inspection: RuntimeInspection::Found { observation, .. },
    } = succeeded_result(&stopped)?
    else {
        return Err(invalid("reference provider cleanup did not stop its Runtime Service").into());
    };
    if observation.state != RuntimeUnitState::Stopped {
        return Err(
            invalid("reference provider Runtime Service remained running after stop").into(),
        );
    }
    let removed = executor
        .execute(command(
            node_id,
            execution_id,
            approval_matrix.next_sequence + 1,
            NodeCommandPayload::RuntimeRemove {
                request: action_request("remove", &spec),
            },
        )?)
        .await?;
    if !matches!(
        succeeded_result(&removed)?,
        NodeCommandResult::RuntimeRemoved { .. }
    ) || !matches!(
        runtime.inspect(&spec.unit_id).await?,
        RuntimeInspection::NotFound { .. }
    ) {
        return Err(invalid("reference provider Runtime cleanup did not converge").into());
    }
    require_clean_state(&home, node_state.path())?;
    if transport.downloads.load(Ordering::SeqCst) != 1 {
        return Err(
            invalid("reference provider Artifact was not materialized exactly once").into(),
        );
    }

    println!(
        "A3S_CLOUD_A1_NON_CODE_BOX_PROVIDER_CERTIFIED provider=reference.echo protocol=common-http start=accepted event_page=exact approvals=approved,denied,expired approval_resume=exact provider_cancel=accepted provider_process_restarts=1 pending_approval_restart=rejected cleanup=removed first_started_at_ms={first_started_at_ms} recovered_started_at_ms={recovered_started_at_ms}"
    );
    Ok(())
}

fn provider_runtime_spec(provider_artifact: ArtifactRef) -> GateResult<RuntimeUnitSpec> {
    let image = std::env::var("A3S_BOX_RUNTIME_CONFORMANCE_IMAGE")?;
    let (repository, digest) = image
        .rsplit_once('@')
        .filter(|(_, digest)| digest.starts_with("sha256:") && digest.len() == 71)
        .ok_or_else(|| invalid("Box conformance image is not digest-pinned"))?;
    let mut environment = BTreeMap::new();
    environment.insert(
        "A3S_REFERENCE_ECHO_LISTEN".into(),
        format!("0.0.0.0:{PROVIDER_PORT}"),
    );
    let spec = RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id: format!("cloud-reference-provider-{}", Uuid::now_v7().simple()),
        generation: 1,
        class: RuntimeUnitClass::Service,
        artifact: ArtifactRef {
            uri: format!("oci://{repository}@{digest}"),
            digest: digest.into(),
            media_type: std::env::var("A3S_BOX_RUNTIME_CONFORMANCE_MEDIA_TYPE")?,
        },
        process: RuntimeProcessSpec {
            command: vec![format!("{PROVIDER_MOUNT}/{PROVIDER_BINARY_NAME}")],
            args: Vec::new(),
            working_directory: Some("/".into()),
            environment,
        },
        mounts: vec![RuntimeMount {
            name: "reference-provider".into(),
            source: RuntimeMountSource::Artifact {
                artifact: provider_artifact,
            },
            target: PROVIDER_MOUNT.into(),
            read_only: true,
        }],
        secrets: Vec::new(),
        network: RuntimeNetworkSpec {
            mode: NetworkMode::Service,
            ports: vec![RuntimePort {
                name: "agent".into(),
                container_port: PROVIDER_PORT,
                protocol: TransportProtocol::Tcp,
            }],
        },
        resources: ResourceLimits {
            cpu_millis: 500,
            memory_bytes: 128 * 1024 * 1024,
            pids: 64,
            ephemeral_storage_bytes: None,
            execution_timeout_ms: None,
        },
        isolation: IsolationLevel::Sandbox,
        health: Some(RuntimeHealthCheck {
            probe: HealthProbe::Http {
                port: "agent".into(),
                path: "/health".into(),
                expected_statuses: vec![200],
            },
            interval_ms: 250,
            timeout_ms: 200,
            start_period_ms: 5_000,
            success_threshold: 1,
            failure_threshold: 40,
        }),
        service_lifecycle: None,
        restart: RestartPolicy::Always,
        outputs: Vec::new(),
        semantics_profile_digest: None,
        identity_attachment_digest: None,
    };
    spec.validate().map_err(invalid)?;
    Ok(spec)
}

async fn kill_box_process(home: &Path, provider_resource_id: &str) -> GateResult {
    let store = BoxStateStore::load_readonly(home.join("boxes.json"))?;
    let records = store
        .records()
        .iter()
        .filter(|record| record.id == provider_resource_id)
        .collect::<Vec<_>>();
    if records.len() != 1 {
        return Err(invalid(format!(
            "Box state contained {} reference provider records",
            records.len()
        ))
        .into());
    }
    let pid = records[0]
        .pid
        .ok_or_else(|| invalid("reference provider Box record omitted its process PID"))?;
    if pid <= 1 || pid == std::process::id() {
        return Err(invalid(format!("reference provider exposed unsafe PID {pid}")).into());
    }
    let status = tokio::process::Command::new("kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status()
        .await?;
    if !status.success() {
        return Err(invalid(format!("could not SIGKILL reference provider PID {pid}")).into());
    }
    Ok(())
}

async fn wait_for_restarted_provider(
    runtime: &dyn RuntimeClient,
    spec: &RuntimeUnitSpec,
    provider_resource_id: &str,
    first_started_at_ms: u64,
) -> GateResult<u64> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match runtime.inspect(&spec.unit_id).await? {
            RuntimeInspection::Found { observation, .. }
                if observation.state == RuntimeUnitState::Running
                    && observation.provider_resource_id.as_deref()
                        == Some(provider_resource_id)
                    && observation
                        .started_at_ms
                        .is_some_and(|started_at_ms| started_at_ms > first_started_at_ms)
                    && observation
                        .health
                        .as_ref()
                        .is_some_and(|health| health.state == RuntimeHealthState::Healthy) =>
            {
                return observation
                    .started_at_ms
                    .ok_or_else(|| invalid("restarted provider omitted its process start time"))
                    .map_err(Into::into);
            }
            RuntimeInspection::NotFound { .. } => {
                return Err(invalid("Box lost the reference provider Service identity").into())
            }
            RuntimeInspection::Found { .. } => {}
        }
        if Instant::now() >= deadline {
            return Err(
                invalid("Box did not restart the reference provider within 30 seconds").into(),
            );
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn command(
    node_id: Uuid,
    aggregate_id: Uuid,
    sequence: u64,
    payload: NodeCommandPayload,
) -> GateResult<NodeCommandEnvelope> {
    let issued_at = Utc::now() - ChronoDuration::seconds(1);
    NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id: Uuid::now_v7(),
            lease_id: Uuid::now_v7(),
            node_id,
            sequence,
            aggregate_id,
            issued_at,
            not_after: issued_at + ChronoDuration::minutes(5),
            correlation_id: Uuid::now_v7(),
        },
        payload,
    )
    .map_err(|error| invalid(error).into())
}

fn action_request(action: &str, spec: &RuntimeUnitSpec) -> RuntimeActionRequest {
    RuntimeActionRequest {
        schema: RuntimeActionRequest::SCHEMA.into(),
        request_id: format!("reference-provider-{action}-{}", Uuid::now_v7()),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        deadline_at_ms: None,
    }
}

fn applied_observation(
    acknowledgement: &NodeCommandAck,
) -> GateResult<&a3s_runtime::contract::RuntimeObservation> {
    match succeeded_result(acknowledgement)? {
        NodeCommandResult::RuntimeApplied { observation } => Ok(observation),
        result => Err(invalid(format!("reference provider apply returned {result:?}")).into()),
    }
}

fn succeeded_result(acknowledgement: &NodeCommandAck) -> GateResult<&NodeCommandResult> {
    match &acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => Ok(result),
        outcome => Err(invalid(format!("reference provider command failed: {outcome:?}")).into()),
    }
}

fn cloud_artifact(bytes: &[u8]) -> GateResult<ArtifactRef> {
    let digest = format!("sha256:{:x}", Sha256::digest(bytes));
    Ok(ArtifactRef {
        uri: artifact_uri(&digest).map_err(invalid)?,
        digest,
        media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
    })
}

fn executable_archive(path: &str, bytes: &[u8]) -> GateResult<Vec<u8>> {
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_mode(0o755);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_size(bytes.len() as u64);
    header.set_cksum();
    builder.append_data(&mut header, path, bytes)?;
    builder.finish()?;
    Ok(builder.into_inner()?)
}

fn require_clean_state(home: &Path, node_state: &Path) -> GateResult {
    let store = BoxStateStore::load_readonly(home.join("boxes.json"))?;
    if !store.records().is_empty()
        || directory_has_entries(&home.join("boxes"))?
        || directory_has_entries(&node_state.join("artifacts/mounts"))?
        || directory_has_entries(&node_state.join("artifacts/outputs"))?
        || directory_has_entries(&node_state.join("artifacts/blobs/sha256"))?
        || directory_has_entries(&node_state.join("artifacts/staging"))?
    {
        return Err(invalid("reference provider cleanup retained Box or Artifact state").into());
    }
    Ok(())
}

fn directory_has_entries(path: &Path) -> io::Result<bool> {
    match std::fs::read_dir(path) {
        Ok(mut entries) => Ok(entries.next().transpose()?.is_some()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn require_gate() -> io::Result<()> {
    if std::env::var("A3S_CLOUD_TEST_BOX").as_deref() != Ok("1") {
        return Err(invalid(
            "dedicated Box gate did not enable the real provider test",
        ));
    }
    Ok(())
}

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
