use crate::modules::executions::domain::Execution;
use a3s_runtime::contract::{
    ArtifactRef, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy, RuntimeNetworkSpec,
    RuntimeProcessSpec, RuntimeUnitClass, RuntimeUnitSpec,
};
use sha2::{Digest, Sha256};

const EXECUTION_ID_ENV: &str = "A3S_EXECUTION_ID";
const EXECUTION_INPUT_ENV: &str = "A3S_EXECUTION_INPUT_JSON";
const EXECUTION_TEMPLATE_DIGEST_ENV: &str = "A3S_EXECUTION_TEMPLATE_DIGEST";
const SEMANTICS_PROFILE: &str = "a3s.cloud.execution-task.v1:network-none:json-env";

pub fn project_execution_task(execution: &Execution) -> Result<RuntimeUnitSpec, String> {
    execution.validate()?;
    let mut environment = execution.template.process.environment.clone();
    environment.insert(EXECUTION_ID_ENV.into(), execution.id.to_string());
    environment.insert(
        EXECUTION_INPUT_ENV.into(),
        serde_json::to_string(&execution.template.input)
            .map_err(|error| format!("could not encode execution input: {error}"))?,
    );
    environment.insert(
        EXECUTION_TEMPLATE_DIGEST_ENV.into(),
        execution.template_digest.clone(),
    );
    let template = &execution.template;
    let spec = RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id: execution.runtime_unit_id(),
        generation: Execution::RUNTIME_GENERATION,
        class: RuntimeUnitClass::Task,
        artifact: ArtifactRef {
            uri: template.artifact.uri.clone(),
            digest: template.artifact.digest.clone(),
            media_type: template.artifact.media_type.clone(),
        },
        process: RuntimeProcessSpec {
            command: template.process.command.clone(),
            args: template.process.args.clone(),
            working_directory: template.process.working_directory.clone(),
            environment,
        },
        mounts: Vec::new(),
        secrets: Vec::new(),
        network: RuntimeNetworkSpec {
            mode: NetworkMode::None,
            ports: Vec::new(),
        },
        resources: ResourceLimits {
            cpu_millis: template.resources.cpu_millis,
            memory_bytes: template.resources.memory_bytes,
            pids: template.resources.pids,
            ephemeral_storage_bytes: template.resources.ephemeral_storage_bytes,
            execution_timeout_ms: Some(template.resources.timeout_ms),
        },
        isolation: IsolationLevel::Sandbox,
        health: None,
        restart: RestartPolicy::Never,
        outputs: Vec::new(),
        semantics_profile_digest: Some(format!(
            "sha256:{:x}",
            Sha256::digest(SEMANTICS_PROFILE.as_bytes())
        )),
    };
    spec.validate()?;
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::executions::domain::{
        ExecutionArtifact, ExecutionProcess, ExecutionResources, ExecutionTemplate,
    };
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, ExecutionId, OrganizationId, ProjectId,
    };
    use chrono::Utc;
    use std::collections::BTreeMap;

    fn execution() -> Execution {
        let digest = format!("sha256:{}", "a".repeat(64));
        Execution::create(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            ExecutionId::new(),
            ExecutionTemplate {
                artifact: ExecutionArtifact {
                    uri: format!("oci://registry.example/tasks/echo@{digest}"),
                    digest,
                    media_type: "application/vnd.oci.image.manifest.v1+json".into(),
                },
                process: ExecutionProcess {
                    command: vec!["/bin/echo-task".into()],
                    args: vec!["--json".into()],
                    working_directory: Some("/workspace".into()),
                    environment: BTreeMap::from([("LANG".into(), "C".into())]),
                },
                input: serde_json::json!({"value": 42}),
                resources: ExecutionResources {
                    cpu_millis: 250,
                    memory_bytes: 128 * 1024 * 1024,
                    pids: 64,
                    ephemeral_storage_bytes: None,
                    timeout_ms: 5_000,
                },
            },
            Utc::now(),
        )
        .expect("execution")
    }

    #[test]
    fn projection_is_a_networkless_sandbox_task_with_bound_input() {
        let execution = execution();
        let spec = project_execution_task(&execution).expect("Runtime Task");
        assert_eq!(spec.class, RuntimeUnitClass::Task);
        assert_eq!(spec.isolation, IsolationLevel::Sandbox);
        assert_eq!(spec.network.mode, NetworkMode::None);
        assert!(spec.mounts.is_empty());
        assert!(spec.outputs.is_empty());
        assert_eq!(
            spec.process.environment.get(EXECUTION_ID_ENV),
            Some(&execution.id.to_string())
        );
        assert_eq!(
            spec.process.environment.get(EXECUTION_INPUT_ENV),
            Some(&"{\"value\":42}".to_owned())
        );
        assert_eq!(
            spec.resources.execution_timeout_ms,
            Some(execution.template.resources.timeout_ms)
        );
        spec.validate().expect("valid Task");
    }

    #[test]
    fn projection_digest_is_stable_and_bound_to_input() {
        let first = execution();
        let first_spec = project_execution_task(&first).expect("first spec");
        assert_eq!(
            first_spec.digest().expect("digest"),
            first_spec.digest().expect("stable digest")
        );

        let mut changed = first.clone();
        changed.id = ExecutionId::new();
        changed.operation_id =
            crate::modules::shared_kernel::domain::OperationId::from_uuid(changed.id.as_uuid());
        changed.template.input = serde_json::json!({"value": 43});
        changed.template_digest = changed.template.digest().expect("template digest");
        let changed_spec = project_execution_task(&changed).expect("changed spec");
        assert_ne!(
            first_spec.digest().expect("first digest"),
            changed_spec.digest().expect("changed digest")
        );
    }
}
