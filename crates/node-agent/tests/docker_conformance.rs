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
#[path = "docker_conformance/recovery.rs"]
mod recovery;
#[path = "docker_conformance/resources.rs"]
mod resources;
#[path = "docker_conformance/security.rs"]
mod security;
#[path = "docker_conformance/specs.rs"]
mod specs;

use a3s_runtime::{
    required_runtime_profiles, verify_runtime_profiles, FileRuntimeStateStore,
    ManagedRuntimeClient, RuntimeClient, RuntimeConformanceProfile, RuntimeStateStore,
};
use fixture::{connect_driver, DockerConformanceFixture};
use std::collections::BTreeSet;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires A3S_CLOUD_TEST_DOCKER=1 on a dedicated Docker provider runner"]
async fn real_docker_passes_all_advertised_runtime_profiles() {
    assert_eq!(
        std::env::var("A3S_CLOUD_TEST_DOCKER").as_deref(),
        Ok("1"),
        "the dedicated Docker conformance gate must set A3S_CLOUD_TEST_DOCKER=1"
    );

    let state_directory = tempfile::tempdir().expect("Runtime state directory");
    let namespace = format!(
        "runtime-conformance-{}",
        &Uuid::now_v7().simple().to_string()[..12]
    );
    let node_id = Uuid::now_v7();
    let driver = Arc::new(
        connect_driver(&namespace, node_id)
            .await
            .expect("connect dedicated Docker conformance driver"),
    );
    let store = Arc::new(FileRuntimeStateStore::new(state_directory.path()));
    let runtime =
        ManagedRuntimeClient::new(store.clone() as Arc<dyn RuntimeStateStore>, driver.clone());
    let fixture = DockerConformanceFixture::new(namespace, node_id, driver, store);

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
