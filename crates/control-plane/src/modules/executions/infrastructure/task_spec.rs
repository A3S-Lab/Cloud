use crate::modules::executions::domain::{
    Execution, ExecutionTaskArtifactMount, ExecutionTaskSecret, ExecutionTaskSecretTarget,
};
use a3s_runtime::contract::{
    ArtifactRef, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy, RuntimeMount,
    RuntimeMountSource, RuntimeNetworkSpec, RuntimeProcessSpec, RuntimeUnitClass, RuntimeUnitSpec,
    SecretReference, SecretTarget,
};
use sha2::{Digest, Sha256};

const EXECUTION_ID_ENV: &str = "A3S_EXECUTION_ID";
const EXECUTION_INPUT_ENV: &str = "A3S_EXECUTION_INPUT_JSON";
const EXECUTION_TEMPLATE_DIGEST_ENV: &str = "A3S_EXECUTION_TEMPLATE_DIGEST";
const EXECUTION_AUTHORITY_KIND_ENV: &str = "A3S_EXECUTION_AUTHORITY_KIND";
const EXECUTION_AUTHORITY_SUBJECT_ENV: &str = "A3S_EXECUTION_AUTHORITY_SUBJECT_ID";
const EXECUTION_AUTHORITY_DIGEST_ENV: &str = "A3S_EXECUTION_AUTHORITY_DIGEST";
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
    if let Some(policy) = &execution.task_policy {
        let authority = policy.authority();
        environment.insert(EXECUTION_AUTHORITY_KIND_ENV.into(), authority.kind().into());
        environment.insert(
            EXECUTION_AUTHORITY_SUBJECT_ENV.into(),
            authority.subject_id().to_string(),
        );
        environment.insert(
            EXECUTION_AUTHORITY_DIGEST_ENV.into(),
            authority.digest().to_string(),
        );
    }
    let template = &execution.template;
    let mounts = if let Some(policy) = &execution.task_policy {
        policy
            .mounts()
            .iter()
            .map(runtime_mount)
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };
    let secrets = execution
        .task_policy
        .as_ref()
        .map(|policy| policy.secrets().iter().map(runtime_secret).collect())
        .unwrap_or_default();
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
        mounts,
        secrets,
        network: RuntimeNetworkSpec {
            mode: if execution.task_policy.is_some() {
                NetworkMode::Outbound
            } else {
                NetworkMode::None
            },
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
        service_lifecycle: None,
        restart: RestartPolicy::Never,
        outputs: Vec::new(),
        semantics_profile_digest: Some(execution.task_policy.as_ref().map_or_else(
            || format!("sha256:{:x}", Sha256::digest(SEMANTICS_PROFILE.as_bytes())),
            |policy| policy.semantics_profile_digest().to_string(),
        )),
        identity_attachment_digest: None,
    };
    spec.validate()?;
    Ok(spec)
}

fn runtime_mount(mount: &ExecutionTaskArtifactMount) -> Result<RuntimeMount, String> {
    Ok(RuntimeMount {
        name: mount.name().into(),
        source: RuntimeMountSource::Artifact {
            artifact: ArtifactRef {
                uri: mount.artifact_uri()?,
                digest: mount.artifact_digest().to_string(),
                media_type: mount.artifact_media_type().into(),
            },
        },
        target: mount.target().into(),
        read_only: true,
    })
}

fn runtime_secret(secret: &ExecutionTaskSecret) -> SecretReference {
    SecretReference {
        name: secret.name().into(),
        reference: secret.reference().to_string(),
        target: match secret.target() {
            ExecutionTaskSecretTarget::Environment { variable } => SecretTarget::Environment {
                variable: variable.clone(),
            },
            ExecutionTaskSecretTarget::File { path, mode } => SecretTarget::File {
                path: path.clone(),
                mode: *mode,
            },
            ExecutionTaskSecretTarget::RegistryCredential => SecretTarget::RegistryCredential,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::executions::domain::{
        ExecutionArtifact, ExecutionProcess, ExecutionResources, ExecutionTaskArtifactMount,
        ExecutionTaskAuthority, ExecutionTaskPolicy, ExecutionTaskSecret,
        ExecutionTaskSecretTarget, ExecutionTemplate,
    };
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, ExecutionId, NodeId, OrganizationId, ProjectId, Sha256Digest,
    };
    use a3s_cloud_contracts::{artifact_uri, CloudSecretReference, DURABLE_CELL_BUNDLE_MEDIA_TYPE};
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

    #[test]
    fn bound_projection_reuses_the_same_task_with_exact_node_inputs_and_outbound_network() {
        let standard = execution();
        let subject_id = uuid::Uuid::now_v7();
        let target_node_id = NodeId::new();
        let bundle_digest = format!("sha256:{}", "b".repeat(64));
        let secret_id = uuid::Uuid::now_v7();
        let policy = ExecutionTaskPolicy::new(
            ExecutionTaskAuthority::new(
                "workload.prestart",
                subject_id,
                Sha256Digest::parse(format!("sha256:{}", "c".repeat(64)))
                    .expect("authority digest"),
            )
            .expect("authority"),
            vec![ExecutionTaskArtifactMount::new(
                "application-bundle",
                artifact_uri(&bundle_digest).expect("artifact URI"),
                Sha256Digest::parse(&bundle_digest).expect("bundle digest"),
                DURABLE_CELL_BUNDLE_MEDIA_TYPE,
                "/workspace/bundle",
            )
            .expect("artifact mount")],
            vec![ExecutionTaskSecret::new(
                "s0-access-key-id",
                CloudSecretReference::new(subject_id, secret_id, 4).expect("Secret reference"),
                ExecutionTaskSecretTarget::Environment {
                    variable: "AWS_ACCESS_KEY_ID".into(),
                },
            )
            .expect("Secret")],
            Sha256Digest::parse(format!("sha256:{}", "d".repeat(64))).expect("semantics digest"),
        )
        .expect("Task policy");
        let bound = Execution::create_bound_task(
            standard.organization_id,
            standard.project_id,
            standard.environment_id,
            ExecutionId::new(),
            standard.template,
            target_node_id,
            policy.clone(),
            Utc::now(),
        )
        .expect("bound execution");

        let spec = project_execution_task(&bound).expect("bound Runtime Task");
        assert_eq!(bound.target_node_id, Some(target_node_id));
        assert_eq!(spec.class, RuntimeUnitClass::Task);
        assert_eq!(spec.network.mode, NetworkMode::Outbound);
        assert_eq!(spec.mounts.len(), 1);
        assert_eq!(spec.mounts[0].name, policy.mounts()[0].name());
        assert_eq!(spec.mounts[0].target, policy.mounts()[0].target());
        assert!(spec.mounts[0].read_only);
        let RuntimeMountSource::Artifact { artifact } = &spec.mounts[0].source else {
            panic!("bound execution mount must project one Cloud artifact");
        };
        assert_eq!(
            artifact.uri,
            artifact_uri(&bundle_digest).expect("artifact URI")
        );
        assert_eq!(artifact.digest, bundle_digest);
        assert_eq!(artifact.media_type, DURABLE_CELL_BUNDLE_MEDIA_TYPE);
        assert_eq!(spec.secrets.len(), 1);
        assert_eq!(spec.secrets[0].name, policy.secrets()[0].name());
        assert_eq!(
            spec.secrets[0].reference,
            policy.secrets()[0].reference().to_string()
        );
        assert_eq!(
            spec.secrets[0].target,
            SecretTarget::Environment {
                variable: "AWS_ACCESS_KEY_ID".into(),
            }
        );
        assert_eq!(
            spec.semantics_profile_digest.as_deref(),
            Some(policy.semantics_profile_digest().as_str())
        );
        assert_eq!(
            spec.process
                .environment
                .get(EXECUTION_AUTHORITY_SUBJECT_ENV)
                .map(String::as_str),
            Some(subject_id.to_string().as_str())
        );
        assert!(spec.outputs.is_empty());
        spec.validate().expect("valid bound Runtime Task");
    }
}
