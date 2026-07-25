#[path = "resource_claims/contract.rs"]
mod contract;
#[path = "resource_claims/process.rs"]
mod process;

use super::artifacts::DockerConformanceArtifacts;
use super::fixture::{connect_driver, require, resource_id, DockerConformanceFixture};
use super::{require_docker_gate, resolve_artifact_state_root, specs};
use a3s_cloud_contracts::{NodeCommandPayload, NodeResourceClaimPrepare, NodeResourceClaimRelease};
use a3s_cloud_node_agent::{CommandExecutor, FileCommandJournal};
use a3s_runtime::contract::RuntimeApplyRequest;
use a3s_runtime::{
    FileRuntimeStateStore, ManagedRuntimeClient, RuntimeClient, RuntimeError, RuntimeResult,
    RuntimeStateStore,
};
use std::sync::Arc;
use uuid::Uuid;

use self::contract::{
    applied_observation, claim_binding, claim_inventory, command, fixture_error,
    inventory_authority, require_exact_unit, require_pending_apply, require_rejected,
    require_success, runtime_action, sha256,
};
use self::process::{wait_for_provider_apply_marker, write_durable_file, ResourceClaimCrashProbe};

const CERTIFICATION_MARKER: &str = "A3S_RESOURCE_CLAIM_CRASH_CERTIFICATION_PASS";

#[tokio::test]
#[ignore = "requires A3S_CLOUD_TEST_DOCKER=1 on the isolated Docker provider gate"]
async fn real_docker_claim_journal_survives_agent_and_provider_process_death() {
    require_docker_gate();
    certify_resource_claim_process_recovery()
        .await
        .expect("real Docker resource Claim process recovery certification");
}

#[tokio::test]
#[ignore = "private child process for the resource Claim crash certification"]
async fn resource_claim_provider_crash_probe() {
    require_docker_gate();
    process::run_provider_crash_probe()
        .await
        .expect("resource Claim provider crash probe must be killed by its parent");
}

async fn certify_resource_claim_process_recovery() -> RuntimeResult<()> {
    let state_directory = tempfile::tempdir()
        .map_err(|error| fixture_error("create resource Claim state directory", error))?;
    let namespace = format!("claim-crash-{}", &Uuid::now_v7().simple().to_string()[..12]);
    let node_id = Uuid::now_v7();
    let agent_instance_id = Uuid::now_v7();
    let claim_id = Uuid::now_v7();
    let competing_claim_id = Uuid::now_v7();
    let workload_id = Uuid::now_v7();
    let spec = specs::service_spec(
        specs::unit_id(&namespace, "resource-claim"),
        "exec sleep 300",
    );
    let inventory = claim_inventory(node_id, agent_instance_id, &spec)?;
    let binding = claim_binding(claim_id, &inventory, &spec.unit_id, spec.generation, &spec);
    let competing_binding = claim_binding(
        competing_claim_id,
        &inventory,
        &format!("{}-competitor", spec.unit_id),
        1,
        &spec,
    );
    let authority = inventory_authority(inventory.clone());

    let artifact_state_root = resolve_artifact_state_root(state_directory.path());
    let artifacts = Arc::new(DockerConformanceArtifacts::new(
        &artifact_state_root,
        node_id,
    )?);
    let driver = Arc::new(connect_driver(&namespace, node_id, artifacts.manager()).await?);
    let runtime_store = Arc::new(FileRuntimeStateStore::new(
        state_directory.path().join("runtime"),
    ));
    let runtime: Arc<dyn RuntimeClient> = Arc::new(ManagedRuntimeClient::new(
        runtime_store.clone() as Arc<dyn RuntimeStateStore>,
        driver.clone(),
    ));
    let fixture = DockerConformanceFixture::new(
        namespace.clone(),
        node_id,
        driver,
        runtime_store.clone(),
        artifacts.clone(),
    );
    let executor = CommandExecutor::runtime_only(
        FileCommandJournal::new(state_directory.path().join("journal"), node_id)
            .map_err(|error| RuntimeError::Protocol(error.to_string()))?,
        runtime,
    )
    .with_artifacts(artifacts.manager())
    .with_resource_inventory(authority.clone());

    let execution = async {
        let prepare_request = NodeResourceClaimPrepare {
            schema: NodeResourceClaimPrepare::SCHEMA.into(),
            claim_generation: 1,
            claim_digest: sha256('1'),
            binding: binding.clone(),
        };
        let prepare = command(
            node_id,
            claim_id,
            1,
            1,
            NodeCommandPayload::ResourceClaimPrepare {
                request: Box::new(prepare_request),
            },
        )?;
        require_success(
            &executor
                .execute(prepare)
                .await
                .map_err(|error| RuntimeError::Protocol(error.to_string()))?,
            "resource Claim prepare",
        )?;

        let apply = command(
            node_id,
            workload_id,
            2,
            spec.generation,
            NodeCommandPayload::RuntimeApply {
                request: Box::new(RuntimeApplyRequest {
                    schema: RuntimeApplyRequest::SCHEMA.into(),
                    request_id: format!("claim-crash-apply-{}", Uuid::now_v7()),
                    deadline_at_ms: None,
                    spec: spec.clone(),
                }),
                resource_claim: Some(Box::new(binding.clone())),
            },
        )?;
        let apply_path = state_directory.path().join("bound-runtime-apply.json");
        let marker_path = state_directory.path().join("provider-apply-complete.json");
        write_durable_file(
            &apply_path,
            &serde_json::to_vec(&apply).map_err(|error| {
                RuntimeError::Protocol(format!(
                    "could not encode resource Claim crash command: {error}"
                ))
            })?,
        )
        .map_err(|error| fixture_error("persist resource Claim crash command", error))?;
        drop(executor);

        let mut crash_probe = ResourceClaimCrashProbe::start(
            &std::env::current_exe()
                .map_err(|error| fixture_error("resolve resource Claim test binary", error))?,
            state_directory.path(),
            &apply_path,
            &marker_path,
            &namespace,
            node_id,
        )?;
        let provider_observation =
            wait_for_provider_apply_marker(&marker_path, &mut crash_probe).await?;
        provider_observation
            .validate_against(&spec)
            .map_err(RuntimeError::Protocol)?;
        let provider_resource_id = resource_id(&provider_observation)?.to_owned();
        require_exact_unit(
            &fixture,
            &spec.unit_id,
            &provider_resource_id,
            "provider apply before Agent acknowledgement",
        )
        .await?;
        require_pending_apply(&runtime_store, &apply).await?;

        fixture.restart_provider().await?;
        require(
            crash_probe
                .try_wait()
                .map_err(|error| fixture_error("inspect resource Claim crash probe", error))?
                .is_none(),
            "resource Claim crash probe exited during provider process restart",
        )?;
        require_exact_unit(
            &fixture,
            &spec.unit_id,
            &provider_resource_id,
            "provider restart before Agent process death",
        )
        .await?;

        let crash_status = crash_probe.kill_and_wait()?;
        require(
            !crash_status.success(),
            "resource Claim crash probe exited successfully instead of being killed",
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            require(
                crash_status.signal() == Some(9),
                format!("resource Claim crash probe exited with {crash_status} instead of SIGKILL"),
            )?;
        }
        require_pending_apply(&runtime_store, &apply).await?;

        let recovered_artifacts = Arc::new(DockerConformanceArtifacts::new(
            &artifact_state_root,
            node_id,
        )?);
        let recovered_driver =
            Arc::new(connect_driver(&namespace, node_id, recovered_artifacts.manager()).await?);
        let recovered_runtime: Arc<dyn RuntimeClient> = Arc::new(ManagedRuntimeClient::new(
            runtime_store.clone() as Arc<dyn RuntimeStateStore>,
            recovered_driver,
        ));
        let recovered = CommandExecutor::runtime_only(
            FileCommandJournal::new(state_directory.path().join("journal"), node_id)
                .map_err(|error| RuntimeError::Protocol(error.to_string()))?,
            recovered_runtime,
        )
        .with_artifacts(recovered_artifacts.manager())
        .with_resource_inventory(authority.clone());

        let applied = recovered
            .execute(apply.clone())
            .await
            .map_err(|error| RuntimeError::Protocol(error.to_string()))?;
        let observation = applied_observation(&applied)?;
        binding
            .validate_runtime_observation(observation)
            .map_err(RuntimeError::Protocol)?;
        require(
            resource_id(observation)? == provider_resource_id,
            "resource Claim journal recovery did not reattach the original provider unit",
        )?;
        require_exact_unit(
            &fixture,
            &spec.unit_id,
            &provider_resource_id,
            "Agent command replay after process death",
        )
        .await?;

        let mut redelivered_apply = apply;
        redelivered_apply.lease_id = Uuid::now_v7();
        let replayed = recovered
            .execute(redelivered_apply)
            .await
            .map_err(|error| RuntimeError::Protocol(error.to_string()))?;
        let replayed_observation = applied_observation(&replayed)?;
        binding
            .validate_runtime_observation(replayed_observation)
            .map_err(RuntimeError::Protocol)?;
        require(
            resource_id(replayed_observation)? == provider_resource_id,
            "completed resource Claim command replay changed the provider unit",
        )?;
        require_exact_unit(
            &fixture,
            &spec.unit_id,
            &provider_resource_id,
            "completed Agent command redelivery",
        )
        .await?;

        let release_before_stop = command(
            node_id,
            claim_id,
            3,
            2,
            NodeCommandPayload::ResourceClaimRelease {
                request: Box::new(NodeResourceClaimRelease {
                    schema: NodeResourceClaimRelease::SCHEMA.into(),
                    claim_generation: 2,
                    claim_digest: sha256('2'),
                    binding: binding.clone(),
                }),
            },
        )?;
        require_rejected(
            &recovered
                .execute(release_before_stop)
                .await
                .map_err(|error| RuntimeError::Protocol(error.to_string()))?,
            "resource_claim_journal",
            "cannot release before Runtime stop or removal evidence",
        )?;

        let competing_prepare_request = NodeResourceClaimPrepare {
            schema: NodeResourceClaimPrepare::SCHEMA.into(),
            claim_generation: 1,
            claim_digest: sha256('3'),
            binding: competing_binding.clone(),
        };
        let competing_prepare = command(
            node_id,
            competing_claim_id,
            4,
            1,
            NodeCommandPayload::ResourceClaimPrepare {
                request: Box::new(competing_prepare_request.clone()),
            },
        )?;
        require_rejected(
            &recovered
                .execute(competing_prepare)
                .await
                .map_err(|error| RuntimeError::Protocol(error.to_string()))?,
            "resource_claim_conflict",
            "insufficient current Agent capacity",
        )?;

        let stop = command(
            node_id,
            workload_id,
            5,
            spec.generation,
            NodeCommandPayload::RuntimeStop {
                request: runtime_action("claim-crash-stop", &spec),
            },
        )?;
        require_success(
            &recovered
                .execute(stop)
                .await
                .map_err(|error| RuntimeError::Protocol(error.to_string()))?,
            "resource-bound Runtime stop",
        )?;

        let remove = command(
            node_id,
            workload_id,
            6,
            spec.generation,
            NodeCommandPayload::RuntimeRemove {
                request: runtime_action("claim-crash-remove", &spec),
            },
        )?;
        require_success(
            &recovered
                .execute(remove)
                .await
                .map_err(|error| RuntimeError::Protocol(error.to_string()))?,
            "resource-bound Runtime removal",
        )?;
        require(
            fixture.unit_container_ids(&spec.unit_id).await?.is_empty(),
            "resource-bound Runtime removal left a provider unit",
        )?;

        let release = command(
            node_id,
            claim_id,
            7,
            3,
            NodeCommandPayload::ResourceClaimRelease {
                request: Box::new(NodeResourceClaimRelease {
                    schema: NodeResourceClaimRelease::SCHEMA.into(),
                    claim_generation: 3,
                    claim_digest: sha256('4'),
                    binding: binding.clone(),
                }),
            },
        )?;
        require_success(
            &recovered
                .execute(release)
                .await
                .map_err(|error| RuntimeError::Protocol(error.to_string()))?,
            "resource Claim release after Runtime fencing",
        )?;

        let competing_retry = command(
            node_id,
            competing_claim_id,
            8,
            1,
            NodeCommandPayload::ResourceClaimPrepare {
                request: Box::new(competing_prepare_request),
            },
        )?;
        require_success(
            &recovered
                .execute(competing_retry)
                .await
                .map_err(|error| RuntimeError::Protocol(error.to_string()))?,
            "competing resource Claim prepare after release",
        )?;
        let competing_release = command(
            node_id,
            competing_claim_id,
            9,
            2,
            NodeCommandPayload::ResourceClaimRelease {
                request: Box::new(NodeResourceClaimRelease {
                    schema: NodeResourceClaimRelease::SCHEMA.into(),
                    claim_generation: 2,
                    claim_digest: sha256('5'),
                    binding: competing_binding,
                }),
            },
        )?;
        require_success(
            &recovered
                .execute(competing_release)
                .await
                .map_err(|error| RuntimeError::Protocol(error.to_string()))?,
            "competing resource Claim cleanup release",
        )?;

        eprintln!(
            "{CERTIFICATION_MARKER} provider_units=1 journal_replay=1 runtime_fenced=1 \
             agent_release=1 claim_id={claim_id} provider_resource_id={provider_resource_id}"
        );
        Ok(())
    }
    .await;

    let cleanup = a3s_runtime::RuntimeConformanceFixture::cleanup(&fixture).await;
    cleanup?;
    execution
}
