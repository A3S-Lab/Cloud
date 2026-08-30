use super::*;
use crate::durable_cell_operator::{
    DurableCellOperatorTransport, HttpDurableCellOperatorTransport,
};
use crate::{CommandExecutor, FileCommandJournal};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandEnvelope, NodeCommandMetadata, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult, NodeDurableCellOperatorBindingV1,
};
use a3s_runtime::contract::{
    ArtifactRef, HealthProbe, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy,
    RuntimeActionRequest, RuntimeApplyRequest, RuntimeHealthCheck, RuntimeHealthState,
    RuntimeInspection, RuntimeNetworkSpec, RuntimePort, RuntimeProcessSpec, RuntimeServiceEndpoint,
    RuntimeUnitClass, RuntimeUnitSpec, RuntimeUnitState, TransportProtocol,
};
use chrono::{Duration as ChronoDuration, Utc};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

const CELLD_SERVICE_PROFILE_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/cell0.3/celld-v0.2.1-service-profile.acl"
));
const PINNED_CELLD_IMAGE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/cell-conformance/celld-image"
));
const PINNED_CELLD_REVISION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/cell-conformance/celld-revision"
));

type GateResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// This is deliberately a Runtime-only provider gate. It proves that one exact
/// provider release is an ordinary Box Service and that Cloud consumes its
/// public readiness and private operator surfaces through the existing Fleet
/// journal. S0 durability, application behavior, Gateway publication, and the
/// fault matrix remain separate CELL0.2/CELL0.5 gates.
#[tokio::test]
#[ignore = "requires the dedicated Linux A3S Box provider runner"]
async fn real_celld_release_uses_the_existing_box_runtime_and_fleet_journal() -> GateResult<()> {
    if std::env::var("A3S_CLOUD_TEST_CELL_PROVIDER").as_deref() != Ok("1") {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "dedicated Durable Cell gate did not enable the real provider test",
        )
        .into());
    }
    let expected_image = PINNED_CELLD_IMAGE.trim();
    if std::env::var("A3S_CLOUD_TEST_CELL_PROVIDER_IMAGE").as_deref() != Ok(expected_image) {
        return Err(io::Error::other(
            "Durable Cell gate image does not match the checked-in immutable provider pin",
        )
        .into());
    }
    let provider_revision = PINNED_CELLD_REVISION.trim();
    if provider_revision.len() != 40
        || !provider_revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(io::Error::other("checked-in celld revision is invalid").into());
    }

    let profile = a3s_acl::parse(CELLD_SERVICE_PROFILE_ACL)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let canonical_profile = a3s_acl::generate_acl(&profile);
    if format!("{canonical_profile}\n") != CELLD_SERVICE_PROFILE_ACL.replace("\r\n", "\n") {
        return Err(io::Error::other("celld Service profile fixture is not canonical ACL").into());
    }
    let service_profile_digest =
        a3s_acl::canonical_digest(&profile).map_err(|error| io::Error::other(error.to_string()))?;

    let home = PathBuf::from(std::env::var("A3S_HOME")?).canonicalize()?;
    let secret_root = home.join("runtime-secrets").canonicalize()?;
    let runtime_state = tempfile::tempdir()?;
    let node_state = tempfile::tempdir()?;
    let config = BoxRuntimeConfig {
        home_dir: home,
        secret_root,
        isolation: BoxRuntimeIsolation::Sandbox,
        control_timeout_ms: 120_000,
        task_poll_interval_ms: 25,
        sev_snp: None,
    };
    let node_id = Uuid::now_v7();
    let artifacts = artifact_manager(node_state.path().join("artifacts"), node_id)?;
    let runtime = build_box_runtime_provider(&config, runtime_state.path())?
        .into_artifact_bound_client(artifacts)
        .await?;
    let spec = celld_runtime_spec(expected_image, &service_profile_digest)?;
    let workload_id = Uuid::now_v7();
    let application_id = Uuid::now_v7();
    let journal_root = node_state.path().join("journal");
    let transport: Arc<dyn DurableCellOperatorTransport> =
        Arc::new(HttpDurableCellOperatorTransport::new()?);
    let executor = CommandExecutor::runtime_only(
        FileCommandJournal::new(&journal_root, node_id)?,
        runtime.clone(),
    )
    .with_durable_cell_operator(transport);

    let apply = command(
        node_id,
        workload_id,
        1,
        NodeCommandPayload::RuntimeApply {
            request: Box::new(RuntimeApplyRequest {
                schema: RuntimeApplyRequest::SCHEMA.into(),
                request_id: format!("cell-provider-apply-{}", Uuid::now_v7()),
                deadline_at_ms: None,
                spec: spec.clone(),
            }),
            resource_claim: None,
        },
    )?;
    let applied = executor.execute(apply.clone()).await?;
    let observation = match succeeded_result(&applied)? {
        NodeCommandResult::RuntimeApplied { observation } => observation.as_ref(),
        result => {
            return Err(io::Error::other(format!(
                "Durable Cell apply returned an unexpected result: {result:?}"
            ))
            .into())
        }
    };
    if observation.state != RuntimeUnitState::Running
        || observation
            .health
            .as_ref()
            .is_none_or(|health| health.state != RuntimeHealthState::Healthy)
    {
        return Err(io::Error::other("real celld Service did not become healthy").into());
    }
    let public =
        RuntimeServiceEndpoint::from_observation(observation, "cell-public").map_err(invalid)?;
    let internal =
        RuntimeServiceEndpoint::from_observation(observation, "cell-internal").map_err(invalid)?;
    if public.socket_addr() == internal.socket_addr() {
        return Err(io::Error::other(
            "real celld public and internal Runtime endpoints were not isolated",
        )
        .into());
    }

    let runtime_spec_digest = spec.digest().map_err(invalid)?;
    let binding = NodeDurableCellOperatorBindingV1 {
        schema: NodeDurableCellOperatorBindingV1::SCHEMA.into(),
        application_id,
        application_revision_id: Uuid::now_v7(),
        application_revision_number: 1,
        workload_id,
        workload_revision_id: Uuid::now_v7(),
        runtime_unit_id: spec.unit_id.clone(),
        runtime_generation: spec.generation,
        runtime_spec_digest,
        service_profile_digest: service_profile_digest.clone(),
        service_template_digest: format!(
            "sha256:{:x}",
            Sha256::digest(b"a3s-cloud-celld-v0.2.1-box-service-template-v1")
        ),
        provider_artifact_digest: spec.artifact.digest.clone(),
        internal_service_port_name: "cell-internal".into(),
    };
    binding.validate().map_err(invalid)?;
    let observe = command(
        node_id,
        application_id,
        2,
        NodeCommandPayload::DurableCellOperatorObserve {
            binding: Box::new(binding.clone()),
        },
    )?;
    let observed = executor.execute(observe.clone()).await?;
    let operator_observation = match succeeded_result(&observed)? {
        NodeCommandResult::DurableCellOperatorObserved { observation } => observation,
        result => {
            return Err(io::Error::other(format!(
                "Durable Cell observation returned an unexpected result: {result:?}"
            ))
            .into())
        }
    };
    operator_observation
        .validate_for(&binding)
        .map_err(invalid)?;
    if [
        operator_observation.occupied,
        operator_observation.evicting,
        operator_observation.restoring,
        operator_observation.activating,
        operator_observation.activation_waiting,
        operator_observation.capacity_waiting,
    ]
    .into_iter()
    .any(|counter| counter != 0)
    {
        return Err(
            io::Error::other("empty celld provider reported occupied Cell capacity").into(),
        );
    }
    let encoded_observation = serde_json::to_string(operator_observation)?;
    if encoded_observation.contains("residents")
        || encoded_observation.contains("published")
        || encoded_observation.contains("phases")
        || encoded_observation.contains("ownership")
    {
        return Err(io::Error::other("provider-native state escaped the node adapter").into());
    }
    let mut redelivered = observe;
    redelivered.lease_id = Uuid::now_v7();
    let replayed = executor.execute(redelivered).await?;
    if replayed.outcome != observed.outcome {
        return Err(
            io::Error::other("Fleet journal changed the provider observation replay").into(),
        );
    }

    let stop = command(
        node_id,
        workload_id,
        3,
        NodeCommandPayload::RuntimeStop {
            request: action_request("stop", &spec),
        },
    )?;
    let stopped = executor.execute(stop).await?;
    match succeeded_result(&stopped)? {
        NodeCommandResult::RuntimeStopped {
            inspection: RuntimeInspection::Found { observation, .. },
        } if observation.state == RuntimeUnitState::Stopped => {}
        result => {
            return Err(io::Error::other(format!(
                "ordinary RuntimeStop did not gracefully stop celld: {result:?}"
            ))
            .into())
        }
    }

    let remove = command(
        node_id,
        workload_id,
        4,
        NodeCommandPayload::RuntimeRemove {
            request: action_request("remove", &spec),
        },
    )?;
    let removed = executor.execute(remove).await?;
    match succeeded_result(&removed)? {
        NodeCommandResult::RuntimeRemoved { removal }
            if removal.unit_id == spec.unit_id && removal.generation == spec.generation => {}
        result => {
            return Err(io::Error::other(format!(
                "ordinary RuntimeRemove did not clean the celld process: {result:?}"
            ))
            .into())
        }
    }
    if !matches!(
        runtime.inspect(&spec.unit_id).await?,
        RuntimeInspection::NotFound { .. }
    ) {
        return Err(io::Error::other("removed celld Runtime unit remained inspectable").into());
    }

    drop(executor);
    drop(runtime);
    let recovered = build_box_runtime_provider(&config, runtime_state.path())?
        .into_artifact_bound_client(artifact_manager(
            node_state.path().join("recovered-artifacts"),
            node_id,
        )?)
        .await?;
    if !matches!(
        recovered.inspect(&spec.unit_id).await?,
        RuntimeInspection::NotFound { .. }
    ) {
        return Err(io::Error::other(
            "Box reconstruction resurrected the removed celld Runtime generation",
        )
        .into());
    }

    println!(
        "A3S_CLOUD_CELL0_3_PROVIDER_RUNTIME_CERTIFIED provider=celld revision={} image_digest={} profile_digest={} apply=healthy operator=sanitized replay=exact stop=graceful remove=verified storage=not-certified",
        provider_revision,
        spec.artifact.digest,
        service_profile_digest,
    );
    Ok(())
}

fn celld_runtime_spec(image: &str, service_profile_digest: &str) -> GateResult<RuntimeUnitSpec> {
    let (_, image_digest) = image
        .rsplit_once('@')
        .filter(|(_, digest)| digest.starts_with("sha256:") && digest.len() == 71)
        .ok_or_else(|| io::Error::other("celld image is not digest-pinned"))?;
    let spec = RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id: format!("cloud-cell-provider-{}", Uuid::now_v7().simple()),
        generation: 1,
        class: RuntimeUnitClass::Service,
        artifact: ArtifactRef {
            uri: format!("oci://{image}"),
            digest: image_digest.into(),
            media_type: "application/vnd.oci.image.index.v1+json".into(),
        },
        process: RuntimeProcessSpec {
            command: vec!["/usr/local/bin/celld".into()],
            args: vec![
                "--listen".into(),
                "0.0.0.0:8080".into(),
                "--internal-listen".into(),
                "0.0.0.0:8081".into(),
                "--advertise".into(),
                "127.0.0.1:8081".into(),
            ],
            working_directory: Some("/".into()),
            environment: BTreeMap::new(),
        },
        mounts: Vec::new(),
        secrets: Vec::new(),
        network: RuntimeNetworkSpec {
            mode: NetworkMode::Service,
            ports: vec![
                RuntimePort {
                    name: "cell-public".into(),
                    container_port: 8080,
                    protocol: TransportProtocol::Tcp,
                },
                RuntimePort {
                    name: "cell-internal".into(),
                    container_port: 8081,
                    protocol: TransportProtocol::Tcp,
                },
            ],
        },
        resources: ResourceLimits {
            cpu_millis: 1_000,
            memory_bytes: 512 * 1024 * 1024,
            pids: 256,
            // This runtime-only gate must request only capabilities advertised
            // by the ordinary Box provider. S0 and storage certification stay
            // in their own retained gates.
            ephemeral_storage_bytes: None,
            execution_timeout_ms: None,
        },
        isolation: IsolationLevel::Sandbox,
        health: Some(RuntimeHealthCheck {
            probe: HealthProbe::Http {
                port: "cell-public".into(),
                path: "/__celld/health".into(),
                expected_statuses: vec![200],
            },
            interval_ms: 500,
            timeout_ms: 250,
            start_period_ms: 2_000,
            success_threshold: 1,
            failure_threshold: 20,
        }),
        restart: RestartPolicy::Always,
        outputs: Vec::new(),
        semantics_profile_digest: Some(service_profile_digest.into()),
        identity_attachment_digest: None,
    };
    spec.validate().map_err(invalid)?;
    Ok(spec)
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
    .map_err(|error| io::Error::other(error).into())
}

fn action_request(prefix: &str, spec: &RuntimeUnitSpec) -> RuntimeActionRequest {
    RuntimeActionRequest {
        schema: RuntimeActionRequest::SCHEMA.into(),
        request_id: format!("cell-provider-{prefix}-{}", Uuid::now_v7()),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        deadline_at_ms: None,
    }
}

fn succeeded_result(acknowledgement: &NodeCommandAck) -> GateResult<&NodeCommandResult> {
    match &acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => Ok(result),
        outcome => Err(io::Error::other(format!(
            "Durable Cell provider command did not succeed: {outcome:?}"
        ))
        .into()),
    }
}

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
