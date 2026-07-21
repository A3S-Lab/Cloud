#[path = "docker_conformance/artifacts.rs"]
mod artifacts;
#[path = "docker_conformance/fixture.rs"]
mod fixture;
#[path = "docker_conformance/health.rs"]
mod health;
#[path = "docker_conformance/logs.rs"]
mod logs;
#[path = "docker_conformance/mounts.rs"]
mod mounts;
#[path = "docker_conformance/networking.rs"]
mod networking;
#[path = "docker_conformance/outputs.rs"]
mod outputs;
#[path = "docker_conformance/recovery.rs"]
mod recovery;
#[path = "docker_conformance/resources.rs"]
mod resources;
#[path = "docker_conformance/secrets.rs"]
mod secrets;
#[path = "docker_conformance/security.rs"]
mod security;
#[path = "docker_conformance/specs.rs"]
mod specs;

use a3s_runtime::{
    required_runtime_profiles, runtime_profile_requirements, verify_runtime_profiles,
    FileRuntimeStateStore, ManagedRuntimeClient, RuntimeClient, RuntimeConformanceFixture,
    RuntimeConformanceProfile, RuntimeStateStore,
};
use artifacts::DockerConformanceArtifacts;
use fixture::{connect_driver, DockerConformanceFixture};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires A3S_CLOUD_TEST_DOCKER=1 on a dedicated Docker provider runner"]
async fn real_docker_passes_all_advertised_runtime_profiles() {
    require_docker_gate();

    let state_directory = tempfile::tempdir().expect("Runtime state directory");
    let namespace = format!(
        "runtime-conformance-{}",
        &Uuid::now_v7().simple().to_string()[..12]
    );
    let node_id = Uuid::now_v7();
    let artifact_state_root = resolve_artifact_state_root(state_directory.path());
    let artifacts = Arc::new(
        DockerConformanceArtifacts::new(&artifact_state_root, node_id)
            .expect("create Docker conformance Artifact manager"),
    );
    let driver = Arc::new(
        connect_driver(&namespace, node_id, artifacts.manager())
            .await
            .expect("connect dedicated Docker conformance driver"),
    );
    let store = Arc::new(FileRuntimeStateStore::new(state_directory.path()));
    let runtime =
        ManagedRuntimeClient::new(store.clone() as Arc<dyn RuntimeStateStore>, driver.clone());
    let fixture = DockerConformanceFixture::new(namespace, node_id, driver, store, artifacts);

    let report = verify_runtime_profiles(&runtime, &fixture)
        .await
        .expect("real Docker Runtime profile conformance");
    let capabilities = runtime
        .capabilities()
        .await
        .expect("read Docker capabilities after conformance");
    let expected = required_runtime_profiles(&capabilities)
        .expect("derive required Docker conformance profiles");
    let actual = report
        .profiles
        .iter()
        .map(|evidence| evidence.profile)
        .collect::<BTreeSet<_>>();

    assert_eq!(actual, expected);
    assert!(actual.contains(&RuntimeConformanceProfile::Base));
    assert!(actual.contains(&RuntimeConformanceProfile::Recovery));
    assert_eq!(report.inventory_after, report.inventory_before);
}

/// Focused development probe for capability-specific behavior on a Docker
/// host that cannot safely restart its daemon. This is not certification:
/// only `real_docker_passes_all_advertised_runtime_profiles` runs mandatory
/// Base and Recovery together.
#[tokio::test]
#[ignore = "requires A3S_CLOUD_TEST_DOCKER=1; does not certify Base or Recovery"]
async fn real_docker_exercises_advertised_optional_profile_behavior() {
    require_docker_gate();
    let state_directory = tempfile::tempdir().expect("Runtime state directory");
    let namespace = format!(
        "runtime-profile-probe-{}",
        &Uuid::now_v7().simple().to_string()[..12]
    );
    let node_id = Uuid::now_v7();
    let artifact_state_root = resolve_artifact_state_root(state_directory.path());
    let artifacts = Arc::new(
        DockerConformanceArtifacts::new(&artifact_state_root, node_id)
            .expect("create Docker profile Artifact manager"),
    );
    let driver = Arc::new(
        connect_driver(&namespace, node_id, artifacts.manager())
            .await
            .expect("connect Docker profile probe driver"),
    );
    let store = Arc::new(FileRuntimeStateStore::new(state_directory.path()));
    let runtime =
        ManagedRuntimeClient::new(store.clone() as Arc<dyn RuntimeStateStore>, driver.clone());
    let fixture = DockerConformanceFixture::new(namespace, node_id, driver, store, artifacts);
    let before = fixture.inventory().await.expect("profile probe inventory");
    let capabilities = runtime
        .capabilities()
        .await
        .expect("Docker profile probe capabilities");

    let execution = async {
        for profile in optional_probe_profiles() {
            let evidence = fixture
                .run_profile(&runtime, &capabilities, profile)
                .await?;
            let requirements = runtime_profile_requirements(&capabilities, profile)?;
            assert_eq!(evidence.case_ids, requirements.case_ids);
            assert_eq!(evidence.capability_claims, requirements.capability_claims);
        }
        Ok::<(), a3s_runtime::RuntimeError>(())
    }
    .await;

    let cleanup = fixture.cleanup().await;
    let after = fixture.inventory().await;
    cleanup.expect("clean Docker profile probe resources");
    assert_eq!(after.expect("post-cleanup profile probe inventory"), before);
    execution.expect("real Docker optional profile behavior");
}

fn require_docker_gate() {
    assert_eq!(
        std::env::var("A3S_CLOUD_TEST_DOCKER").as_deref(),
        Ok("1"),
        "the dedicated Docker conformance gate must set A3S_CLOUD_TEST_DOCKER=1"
    );
}

fn resolve_artifact_state_root(default: &Path) -> PathBuf {
    let Some(configured) = std::env::var_os("A3S_CLOUD_TEST_ARTIFACT_STATE_ROOT") else {
        return default.to_path_buf();
    };
    let configured = PathBuf::from(configured);
    assert!(
        configured.is_absolute(),
        "A3S_CLOUD_TEST_ARTIFACT_STATE_ROOT must be absolute"
    );
    let canonical = std::fs::canonicalize(&configured).unwrap_or_else(|error| {
        panic!(
            "A3S_CLOUD_TEST_ARTIFACT_STATE_ROOT must exist: {}: {error}",
            configured.display()
        )
    });
    assert!(
        canonical.is_dir(),
        "A3S_CLOUD_TEST_ARTIFACT_STATE_ROOT must be a directory"
    );
    assert_eq!(
        canonical, configured,
        "A3S_CLOUD_TEST_ARTIFACT_STATE_ROOT must already be canonical"
    );
    configured
}

fn optional_probe_profiles() -> Vec<RuntimeConformanceProfile> {
    let all = vec![
        RuntimeConformanceProfile::Networking,
        RuntimeConformanceProfile::Mounts,
        RuntimeConformanceProfile::Health,
        RuntimeConformanceProfile::Resources,
        RuntimeConformanceProfile::Logs,
        RuntimeConformanceProfile::Security,
        RuntimeConformanceProfile::Outputs,
    ];
    let Ok(selected) = std::env::var("A3S_CLOUD_TEST_RUNTIME_PROFILE") else {
        return all;
    };
    let profile = all
        .into_iter()
        .find(|profile| profile.as_str() == selected)
        .unwrap_or_else(|| panic!("unsupported optional Docker profile {selected:?}"));
    vec![profile]
}
