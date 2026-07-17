use a3s_runtime::contract::{
    ArtifactRef, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy, RuntimeActionRequest,
    RuntimeApplyRequest, RuntimeNetworkSpec, RuntimeProcessSpec, RuntimeUnitClass, RuntimeUnitSpec,
};
use a3s_runtime::{RuntimeBaseConformanceCase, RuntimeConformanceCase};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const BUSYBOX_DIGEST: &str =
    "sha256:73aaf090f3d85aa34ee199857f03fa3a95c8ede2ffd4cc2cdb5b94e566b11662";

pub fn base_case(run_id: &str) -> RuntimeBaseConformanceCase {
    let task = task_spec(unit_id(run_id, "base-task"), "printf 'task-complete\\n'");
    let service = service_spec(unit_id(run_id, "base-service"), "exec sleep 300");
    let failure = task_spec(unit_id(run_id, "base-failure"), "exit 23");
    let mut timeout = task_spec(unit_id(run_id, "base-timeout"), "exec sleep 5");
    timeout.resources.execution_timeout_ms = Some(150);
    let generation = service_spec(unit_id(run_id, "base-generation"), "exec sleep 300");
    let mut generation_conflict = generation.clone();
    generation_conflict.process.args = vec!["-c".into(), "exec sleep 301".into()];

    RuntimeBaseConformanceCase {
        lifecycle: RuntimeConformanceCase {
            task_apply: apply("base-task-apply", task.clone()),
            task_remove: action("base-task-remove", &task),
            service_apply: apply("base-service-apply", service.clone()),
            service_stop: action("base-service-stop", &service),
            service_remove: action("base-service-remove", &service),
        },
        task_failure_apply: apply("base-failure-apply", failure.clone()),
        task_failure_remove: action("base-failure-remove", &failure),
        task_timeout_apply: apply("base-timeout-apply", timeout.clone()),
        task_timeout_remove: action("base-timeout-remove", &timeout),
        generation_apply: apply("base-generation-apply", generation.clone()),
        generation_conflict_apply: apply("base-generation-conflict", generation_conflict),
        generation_remove: action("base-generation-remove", &generation),
    }
}

pub fn task_spec(unit_id: String, script: &str) -> RuntimeUnitSpec {
    RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id,
        generation: 1,
        class: RuntimeUnitClass::Task,
        artifact: artifact(),
        process: shell_process(script),
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

pub fn service_spec(unit_id: String, script: &str) -> RuntimeUnitSpec {
    RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id,
        generation: 1,
        class: RuntimeUnitClass::Service,
        artifact: artifact(),
        process: shell_process(script),
        mounts: Vec::new(),
        secrets: Vec::new(),
        network: RuntimeNetworkSpec {
            mode: NetworkMode::Outbound,
            ports: Vec::new(),
        },
        resources: resources(None),
        isolation: IsolationLevel::Container,
        health: None,
        restart: RestartPolicy::Always,
        outputs: Vec::new(),
        semantics_profile_digest: None,
    }
}

pub fn shell_process(script: &str) -> RuntimeProcessSpec {
    RuntimeProcessSpec {
        command: vec!["/bin/sh".into()],
        args: vec!["-c".into(), script.into()],
        working_directory: None,
        environment: BTreeMap::new(),
    }
}

pub fn artifact() -> ArtifactRef {
    ArtifactRef {
        uri: format!("oci://docker.io/library/busybox@{BUSYBOX_DIGEST}"),
        digest: BUSYBOX_DIGEST.into(),
        media_type: "application/vnd.oci.image.manifest.v1+json".into(),
    }
}

pub fn resources(execution_timeout_ms: Option<u64>) -> ResourceLimits {
    ResourceLimits {
        cpu_millis: 250,
        memory_bytes: 64 * 1024 * 1024,
        pids: 64,
        ephemeral_storage_bytes: None,
        execution_timeout_ms,
    }
}

pub fn apply(prefix: &str, spec: RuntimeUnitSpec) -> RuntimeApplyRequest {
    RuntimeApplyRequest {
        schema: RuntimeApplyRequest::SCHEMA.into(),
        request_id: request_id(prefix),
        deadline_at_ms: None,
        spec,
    }
}

pub fn action(prefix: &str, spec: &RuntimeUnitSpec) -> RuntimeActionRequest {
    RuntimeActionRequest {
        schema: RuntimeActionRequest::SCHEMA.into(),
        request_id: request_id(prefix),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        deadline_at_ms: None,
    }
}

pub fn unit_id(run_id: &str, suffix: &str) -> String {
    format!(
        "{run_id}-{suffix}-{}",
        &Uuid::now_v7().simple().to_string()[..8]
    )
}

fn request_id(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::now_v7())
}
