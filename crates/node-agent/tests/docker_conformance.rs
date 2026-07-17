use a3s_cloud_node_agent::{DockerConfig, DockerRuntimeDriver, NodeRuntimeBinding};
use a3s_runtime::contract::{
    ArtifactRef, HealthProbe, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy,
    RuntimeActionRequest, RuntimeApplyRequest, RuntimeHealthCheck, RuntimeInspection,
    RuntimeNetworkSpec, RuntimePort, RuntimeProcessSpec, RuntimeUnitClass, RuntimeUnitSpec,
    RuntimeUnitState, TransportProtocol,
};
use a3s_runtime::{
    verify_runtime_provider, FileRuntimeStateStore, ManagedRuntimeClient, RuntimeClient,
    RuntimeConformanceCase, RuntimeDriver, RuntimeStateStore,
};
use bollard::container::{ListContainersOptions, RemoveContainerOptions};
use bollard::Docker;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const BUSYBOX_DIGEST: &str =
    "sha256:73aaf090f3d85aa34ee199857f03fa3a95c8ede2ffd4cc2cdb5b94e566b11662";

#[tokio::test]
async fn real_docker_passes_runtime_task_and_service_conformance() {
    if !docker_tests_enabled() {
        return;
    }
    let directory = tempfile::tempdir().expect("Runtime state directory");
    let namespace = test_namespace("conformance");
    let driver = Arc::new(driver(&namespace).await);
    let store: Arc<dyn RuntimeStateStore> = Arc::new(FileRuntimeStateStore::new(directory.path()));
    let runtime = ManagedRuntimeClient::new(store, driver);
    let task_id = format!("task-{}", Uuid::now_v7());
    let service_id = format!("service-{}", Uuid::now_v7());
    let task = task_spec(task_id.clone());
    let service = service_spec(service_id.clone(), true);
    let case = RuntimeConformanceCase {
        task_apply: apply("task-apply", task),
        task_remove: action("task-remove", task_id),
        service_apply: apply("service-apply", service),
        service_stop: action("service-stop", service_id.clone()),
        service_remove: action("service-remove", service_id),
    };

    let report = verify_runtime_provider(&runtime, &case)
        .await
        .expect("real Docker Runtime conformance");

    assert!(report.task.converges(&case.task_apply.spec));
    assert!(report.service.converges(&case.service_apply.spec));
    assert!(report.task_removal.removed_at_ms > 0);
    assert!(report.service_removal.removed_at_ms > 0);
}

#[tokio::test]
async fn provider_create_before_state_update_reattaches_the_same_container() {
    if !docker_tests_enabled() {
        return;
    }
    let directory = tempfile::tempdir().expect("Runtime state directory");
    let namespace = test_namespace("crash");
    let driver = Arc::new(driver(&namespace).await);
    let store = Arc::new(FileRuntimeStateStore::new(directory.path()));
    let unit_id = format!("crash-service-{}", Uuid::now_v7());
    let request = apply("crash-apply", service_spec(unit_id.clone(), false));
    let reservation = store
        .reserve_apply(&request, now_ms())
        .await
        .expect("reserve apply before crash");

    let first = driver
        .apply(&request.spec, &reservation.record.observation)
        .await
        .expect("provider create before simulated crash");
    let first_resource = first
        .provider_resource_id
        .clone()
        .expect("first provider resource ID");
    drop(first);

    let runtime = ManagedRuntimeClient::new(store, driver.clone());
    let recovered = runtime
        .apply(&request)
        .await
        .expect("reattach after simulated crash");
    assert_eq!(
        recovered.provider_resource_id.as_deref(),
        Some(first_resource.as_str())
    );
    assert_eq!(managed_container_count(&namespace, &unit_id).await, 1);

    runtime
        .remove(&action("crash-remove", unit_id))
        .await
        .expect("remove recovered container");
}

#[tokio::test]
async fn lost_provider_container_is_recreated_once_for_the_same_generation() {
    if !docker_tests_enabled() {
        return;
    }
    let directory = tempfile::tempdir().expect("Runtime state directory");
    let namespace = test_namespace("lost-provider");
    let driver = Arc::new(driver(&namespace).await);
    let store: Arc<dyn RuntimeStateStore> = Arc::new(FileRuntimeStateStore::new(directory.path()));
    let runtime = ManagedRuntimeClient::new(store, driver);
    let unit_id = format!("lost-service-{}", Uuid::now_v7());
    let spec = service_spec(unit_id.clone(), false);
    let first = runtime
        .apply(&apply("lost-provider-initial", spec.clone()))
        .await
        .expect("initial provider apply");
    let first_resource = first
        .provider_resource_id
        .clone()
        .expect("initial provider resource ID");

    Docker::connect_with_unix_defaults()
        .expect("Docker client")
        .remove_container(
            &first_resource,
            Some(RemoveContainerOptions {
                force: true,
                v: false,
                link: false,
            }),
        )
        .await
        .expect("remove provider container outside Runtime");
    let RuntimeInspection::Found { observation, .. } = runtime
        .inspect(&unit_id)
        .await
        .expect("inspect lost provider")
    else {
        panic!("persisted Runtime identity must become unknown before recovery");
    };
    assert_eq!(observation.state, RuntimeUnitState::Unknown);

    let recovered = runtime
        .apply(&apply("lost-provider-recovery", spec))
        .await
        .expect("same-generation provider recovery");
    assert_eq!(recovered.state, RuntimeUnitState::Running);
    assert_ne!(
        recovered.provider_resource_id.as_deref(),
        Some(first_resource.as_str())
    );
    assert_eq!(managed_container_count(&namespace, &unit_id).await, 1);

    runtime
        .remove(&action("lost-provider-remove", unit_id))
        .await
        .expect("remove recovered provider container");
}

async fn driver(namespace: &str) -> DockerRuntimeDriver {
    let driver = DockerRuntimeDriver::connect(&DockerConfig {
        socket: "unix:///var/run/docker.sock".into(),
        namespace: namespace.into(),
        operation_timeout_ms: 30_000,
    })
    .expect("connect Docker driver");
    driver
        .bind_node(Uuid::now_v7())
        .await
        .expect("bind Docker node identity");
    driver
}

fn apply(prefix: &str, spec: RuntimeUnitSpec) -> RuntimeApplyRequest {
    RuntimeApplyRequest {
        schema: RuntimeApplyRequest::SCHEMA.into(),
        request_id: format!("{prefix}-{}", Uuid::now_v7()),
        deadline_at_ms: None,
        spec,
    }
}

fn action(prefix: &str, unit_id: String) -> RuntimeActionRequest {
    RuntimeActionRequest {
        schema: RuntimeActionRequest::SCHEMA.into(),
        request_id: format!("{prefix}-{}", Uuid::now_v7()),
        unit_id,
        generation: 1,
        deadline_at_ms: None,
    }
}

fn task_spec(unit_id: String) -> RuntimeUnitSpec {
    RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id,
        generation: 1,
        class: RuntimeUnitClass::Task,
        artifact: artifact(),
        process: RuntimeProcessSpec {
            command: vec!["/bin/sh".into()],
            args: vec!["-c".into(), "printf 'task-complete\\n'".into()],
            working_directory: None,
            environment: BTreeMap::new(),
        },
        mounts: Vec::new(),
        secrets: Vec::new(),
        network: RuntimeNetworkSpec {
            mode: NetworkMode::None,
            ports: Vec::new(),
        },
        resources: resources(Some(10_000)),
        isolation: IsolationLevel::Container,
        health: None,
        restart: RestartPolicy::Never,
        outputs: Vec::new(),
        semantics_profile_digest: None,
    }
}

fn service_spec(unit_id: String, health: bool) -> RuntimeUnitSpec {
    RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id,
        generation: 1,
        class: RuntimeUnitClass::Service,
        artifact: artifact(),
        process: RuntimeProcessSpec {
            command: vec!["/bin/sh".into()],
            args: vec![
                "-c".into(),
                if health {
                    "mkdir -p /www && printf 'healthy\\n' >/www/index.html && exec httpd -f -p 8080 -h /www"
                        .into()
                } else {
                    "exec sleep 300".into()
                },
            ],
            working_directory: None,
            environment: BTreeMap::new(),
        },
        mounts: Vec::new(),
        secrets: Vec::new(),
        network: RuntimeNetworkSpec {
            mode: if health {
                NetworkMode::Service
            } else {
                NetworkMode::Outbound
            },
            ports: if health {
                vec![RuntimePort {
                    name: "http".into(),
                    container_port: 8080,
                    protocol: TransportProtocol::Tcp,
                }]
            } else {
                Vec::new()
            },
        },
        resources: resources(None),
        isolation: IsolationLevel::Container,
        health: health.then_some(RuntimeHealthCheck {
            probe: HealthProbe::Http {
                port: "http".into(),
                path: "/".into(),
                expected_statuses: vec![200],
            },
            interval_ms: 100,
            timeout_ms: 100,
            start_period_ms: 100,
            success_threshold: 2,
            failure_threshold: 20,
        }),
        restart: RestartPolicy::Always,
        outputs: Vec::new(),
        semantics_profile_digest: None,
    }
}

fn artifact() -> ArtifactRef {
    ArtifactRef {
        uri: format!("oci://docker.io/library/busybox@{BUSYBOX_DIGEST}"),
        digest: BUSYBOX_DIGEST.into(),
        media_type: "application/vnd.oci.image.manifest.v1+json".into(),
    }
}

fn resources(execution_timeout_ms: Option<u64>) -> ResourceLimits {
    ResourceLimits {
        cpu_millis: 250,
        memory_bytes: 64 * 1024 * 1024,
        pids: 64,
        ephemeral_storage_bytes: None,
        execution_timeout_ms,
    }
}

async fn managed_container_count(namespace: &str, unit_id: &str) -> usize {
    let docker = Docker::connect_with_unix_defaults().expect("Docker client");
    let mut filters = HashMap::new();
    filters.insert(
        "label".to_owned(),
        vec![
            "a3s.cloud.managed=true".to_owned(),
            format!("a3s.cloud.namespace={namespace}"),
            format!("a3s.runtime.unit-id={unit_id}"),
        ],
    );
    docker
        .list_containers(Some(ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        }))
        .await
        .expect("list managed containers")
        .len()
}

fn test_namespace(label: &str) -> String {
    format!(
        "test-{label}-{}",
        &Uuid::now_v7().simple().to_string()[..12]
    )
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_millis() as u64
}

fn docker_tests_enabled() -> bool {
    std::env::var("A3S_CLOUD_TEST_DOCKER").as_deref() == Ok("1")
}
