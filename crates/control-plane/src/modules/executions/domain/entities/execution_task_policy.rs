use super::{ExecutionArtifact, ExecutionProcess, ExecutionResources, ExecutionTemplate};
use crate::modules::shared_kernel::domain::{NodeId, Sha256Digest};
use a3s_cloud_contracts::{validate_cloud_artifact, CloudSecretReference};
use a3s_runtime::contract::{
    ArtifactRef, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy, RuntimeMount,
    RuntimeMountSource, RuntimeNetworkSpec, RuntimeProcessSpec, RuntimeUnitClass, RuntimeUnitSpec,
    SecretReference,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

const MAX_AUTHORITY_KIND_BYTES: usize = 96;

/// Immutable authority for a privileged finite Task projected by an existing
/// Cloud owner. It is internal execution metadata, not product configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionTaskAuthority {
    pub kind: String,
    pub subject_id: Uuid,
    pub digest: Sha256Digest,
}

impl ExecutionTaskAuthority {
    pub fn validate(&self) -> Result<(), String> {
        let mut bytes = self.kind.bytes();
        if self.kind.len() > MAX_AUTHORITY_KIND_BYTES
            || !bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
            || !bytes.all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'-' | b'_')
            })
            || self.subject_id.is_nil()
            || Sha256Digest::parse(self.digest.as_str())? != self.digest
        {
            return Err("bound execution Task authority is invalid".into());
        }
        Ok(())
    }
}

/// Extra inputs allowed only for an internally-created, node-bound Execution.
/// Public ExecutionTemplate admission never accepts this value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionTaskPolicy {
    pub authority: ExecutionTaskAuthority,
    pub mounts: Vec<RuntimeMount>,
    pub secrets: Vec<SecretReference>,
    pub semantics_profile_digest: Sha256Digest,
}

impl ExecutionTaskPolicy {
    pub fn validate(
        &self,
        target_node_id: NodeId,
        template: &ExecutionTemplate,
    ) -> Result<(), String> {
        self.authority.validate()?;
        template.validate()?;
        if target_node_id.as_uuid().is_nil()
            || self.mounts.is_empty()
            || self.mounts.len() > 128
            || self.secrets.is_empty()
            || self.secrets.len() > 128
            || Sha256Digest::parse(self.semantics_profile_digest.as_str())?
                != self.semantics_profile_digest
        {
            return Err("bound execution Task policy is invalid".into());
        }

        let mut mount_names = BTreeSet::new();
        let mut mount_targets = BTreeSet::new();
        for mount in &self.mounts {
            let RuntimeMountSource::Artifact { artifact } = &mount.source else {
                return Err("bound execution Task mounts must use shared artifacts".into());
            };
            if !mount.read_only
                || !mount_names.insert(mount.name.as_str())
                || !mount_targets.insert(mount.target.as_str())
            {
                return Err(
                    "bound execution Task artifact mounts must be read-only and unique".into(),
                );
            }
            validate_cloud_artifact(artifact)?;
        }

        let mut secret_names = BTreeSet::new();
        let mut secret_targets = BTreeSet::new();
        for secret in &self.secrets {
            let reference = CloudSecretReference::parse(&secret.reference)?;
            let target = serde_json::to_string(&secret.target)
                .map_err(|error| format!("could not encode Runtime Secret target: {error}"))?;
            if reference.workload_revision_id != self.authority.subject_id
                || !secret_names.insert(secret.name.as_str())
                || !secret_targets.insert(target)
            {
                return Err(
                    "bound execution Task Secrets must be unique and belong to its authority subject"
                        .into(),
                );
            }
        }

        // Runtime owns the final protocol validation. Building the exact
        // privileged shape here prevents a stored policy from becoming valid
        // only when it is eventually dispatched.
        runtime_spec_for_validation(template, self).validate()
    }
}

fn runtime_spec_for_validation(
    template: &ExecutionTemplate,
    policy: &ExecutionTaskPolicy,
) -> RuntimeUnitSpec {
    RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id: "cloud-bound-execution-policy-validation".into(),
        generation: 1,
        class: RuntimeUnitClass::Task,
        artifact: artifact(&template.artifact),
        process: process(&template.process),
        mounts: policy.mounts.clone(),
        secrets: policy.secrets.clone(),
        network: RuntimeNetworkSpec {
            mode: NetworkMode::Outbound,
            ports: Vec::new(),
        },
        resources: resources(&template.resources),
        isolation: IsolationLevel::Sandbox,
        health: None,
        restart: RestartPolicy::Never,
        outputs: Vec::new(),
        semantics_profile_digest: Some(policy.semantics_profile_digest.to_string()),
    }
}

fn artifact(artifact: &ExecutionArtifact) -> ArtifactRef {
    ArtifactRef {
        uri: artifact.uri.clone(),
        digest: artifact.digest.clone(),
        media_type: artifact.media_type.clone(),
    }
}

fn process(process: &ExecutionProcess) -> RuntimeProcessSpec {
    RuntimeProcessSpec {
        command: process.command.clone(),
        args: process.args.clone(),
        working_directory: process.working_directory.clone(),
        environment: process.environment.clone(),
    }
}

fn resources(resources: &ExecutionResources) -> ResourceLimits {
    ResourceLimits {
        cpu_millis: resources.cpu_millis,
        memory_bytes: resources.memory_bytes,
        pids: resources.pids,
        ephemeral_storage_bytes: resources.ephemeral_storage_bytes,
        execution_timeout_ms: Some(resources.timeout_ms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{artifact_uri, DURABLE_CELL_BUNDLE_MEDIA_TYPE};
    use a3s_runtime::contract::{RuntimeMount, SecretTarget};
    use std::collections::BTreeMap;

    fn digest(fill: char) -> String {
        format!("sha256:{}", fill.to_string().repeat(64))
    }

    fn template() -> ExecutionTemplate {
        let image_digest = digest('a');
        ExecutionTemplate {
            artifact: ExecutionArtifact {
                uri: format!("oci://registry.example/a3s/publisher@{image_digest}"),
                digest: image_digest,
                media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            },
            process: ExecutionProcess {
                command: vec!["/usr/local/bin/a3s-cell-publisher".into()],
                args: Vec::new(),
                working_directory: Some("/workspace".into()),
                environment: BTreeMap::new(),
            },
            input: serde_json::json!({"applicationAcl": "durable_cell_application {}"}),
            resources: ExecutionResources {
                cpu_millis: 250,
                memory_bytes: 128 * 1024 * 1024,
                pids: 64,
                ephemeral_storage_bytes: Some(512 * 1024 * 1024),
                timeout_ms: 30_000,
            },
        }
    }

    fn policy(subject_id: Uuid) -> ExecutionTaskPolicy {
        let bundle_digest = digest('b');
        ExecutionTaskPolicy {
            authority: ExecutionTaskAuthority {
                kind: "workload.prestart".into(),
                subject_id,
                digest: Sha256Digest::parse(digest('c')).expect("authority digest"),
            },
            mounts: vec![RuntimeMount {
                name: "application-bundle".into(),
                source: RuntimeMountSource::Artifact {
                    artifact: ArtifactRef {
                        uri: artifact_uri(&bundle_digest).expect("artifact URI"),
                        digest: bundle_digest,
                        media_type: DURABLE_CELL_BUNDLE_MEDIA_TYPE.into(),
                    },
                },
                target: "/workspace/bundle".into(),
                read_only: true,
            }],
            secrets: vec![SecretReference {
                name: "s0-access-key-id".into(),
                reference: CloudSecretReference::new(subject_id, Uuid::now_v7(), 3)
                    .expect("Secret reference")
                    .to_string(),
                target: SecretTarget::Environment {
                    variable: "AWS_ACCESS_KEY_ID".into(),
                },
            }],
            semantics_profile_digest: Sha256Digest::parse(digest('d')).expect("semantics digest"),
        }
    }

    #[test]
    fn accepts_only_read_only_shared_artifacts_and_exact_workload_secret_references() {
        let subject_id = Uuid::now_v7();
        let policy = policy(subject_id);
        policy
            .validate(NodeId::new(), &template())
            .expect("valid bound policy");

        let mut writable = policy.clone();
        writable.mounts[0].read_only = false;
        assert!(writable.validate(NodeId::new(), &template()).is_err());

        let mut foreign = policy;
        foreign.secrets[0].reference = CloudSecretReference::new(Uuid::now_v7(), Uuid::now_v7(), 3)
            .expect("foreign reference")
            .to_string();
        assert!(foreign.validate(NodeId::new(), &template()).is_err());
    }
}
