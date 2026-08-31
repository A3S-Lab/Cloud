use super::{
    validation::{valid_absolute_path, valid_environment_name, valid_name},
    ExecutionTemplate,
};
use crate::modules::shared_kernel::domain::{NodeId, Sha256Digest};
use a3s_cloud_contracts::{
    artifact_uri as cloud_artifact_uri, CloudSecretReference, DURABLE_CELL_BUNDLE_MEDIA_TYPE,
    NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE, SKILL_BUNDLE_MEDIA_TYPE,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeSet;
use uuid::Uuid;

const MAX_AUTHORITY_KIND_BYTES: usize = 96;
const MAX_TASK_INPUTS: usize = 128;

/// Immutable authority for a privileged finite Task projected by an existing
/// Cloud owner. It is internal execution metadata, not product configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionTaskAuthority {
    kind: String,
    subject_id: Uuid,
    digest: Sha256Digest,
}

impl ExecutionTaskAuthority {
    pub fn new(
        kind: impl Into<String>,
        subject_id: Uuid,
        digest: Sha256Digest,
    ) -> Result<Self, String> {
        let value = Self {
            kind: kind.into(),
            subject_id,
            digest,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), String> {
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

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub const fn subject_id(&self) -> Uuid {
        self.subject_id
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExecutionTaskAuthorityDocument {
    kind: String,
    subject_id: Uuid,
    digest: Sha256Digest,
}

impl<'de> Deserialize<'de> for ExecutionTaskAuthority {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let document = ExecutionTaskAuthorityDocument::deserialize(deserializer)?;
        Self::new(document.kind, document.subject_id, document.digest)
            .map_err(serde::de::Error::custom)
    }
}

/// One immutable Cloud artifact mounted read-only into a bound Execution Task.
///
/// The Domain deliberately has no Volume, tmpfs, or writable variant. Those
/// are Runtime transport capabilities, while a bound Execution accepts only a
/// content-addressed Cloud artifact. Custom serialization preserves the
/// migration-119 document without making its Runtime-shaped `source` and
/// `read_only` fields part of the domain model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionTaskArtifactMount {
    name: String,
    artifact_digest: Sha256Digest,
    artifact_media_type: String,
    target: String,
}

impl ExecutionTaskArtifactMount {
    pub fn new(
        name: impl Into<String>,
        artifact_uri: impl Into<String>,
        artifact_digest: Sha256Digest,
        artifact_media_type: impl Into<String>,
        target: impl Into<String>,
    ) -> Result<Self, String> {
        let artifact_uri = artifact_uri.into();
        let value = Self {
            name: name.into(),
            artifact_digest,
            artifact_media_type: artifact_media_type.into(),
            target: target.into(),
        };
        value.validate()?;
        if artifact_uri != value.artifact_uri()? {
            return Err("bound execution Task artifact URI does not match its digest".into());
        }
        Ok(value)
    }

    fn validate(&self) -> Result<(), String> {
        cloud_artifact_uri(self.artifact_digest.as_str())?;
        if !valid_name(&self.name)
            || !valid_absolute_path(&self.target)
            || !matches!(
                self.artifact_media_type.as_str(),
                NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE
                    | SKILL_BUNDLE_MEDIA_TYPE
                    | DURABLE_CELL_BUNDLE_MEDIA_TYPE
            )
        {
            return Err(
                "bound execution Task mount must be a named read-only Cloud artifact at a bounded absolute target"
                    .into(),
            );
        }
        Ok(())
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn artifact_uri(&self) -> Result<String, String> {
        cloud_artifact_uri(self.artifact_digest.as_str())
    }

    pub const fn artifact_digest(&self) -> &Sha256Digest {
        &self.artifact_digest
    }

    pub fn artifact_media_type(&self) -> &str {
        &self.artifact_media_type
    }

    pub fn target(&self) -> &str {
        &self.target
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutionTaskArtifactMountDocument {
    name: String,
    source: ExecutionTaskArtifactMountSourceDocument,
    target: String,
    read_only: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ExecutionTaskArtifactMountSourceDocument {
    Artifact {
        artifact: ExecutionTaskArtifactDocument,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutionTaskArtifactDocument {
    uri: String,
    digest: String,
    media_type: String,
}

impl Serialize for ExecutionTaskArtifactMount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ExecutionTaskArtifactMountDocument {
            name: self.name.clone(),
            source: ExecutionTaskArtifactMountSourceDocument::Artifact {
                artifact: ExecutionTaskArtifactDocument {
                    uri: self.artifact_uri().map_err(serde::ser::Error::custom)?,
                    digest: self.artifact_digest.to_string(),
                    media_type: self.artifact_media_type.clone(),
                },
            },
            target: self.target.clone(),
            read_only: true,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExecutionTaskArtifactMount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let document = ExecutionTaskArtifactMountDocument::deserialize(deserializer)?;
        if !document.read_only {
            return Err(serde::de::Error::custom(
                "bound execution Task artifact mount must be read-only",
            ));
        }
        let ExecutionTaskArtifactMountSourceDocument::Artifact { artifact } = document.source;
        Self::new(
            document.name,
            artifact.uri,
            Sha256Digest::parse(artifact.digest).map_err(serde::de::Error::custom)?,
            artifact.media_type,
            document.target,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Product-level materialization target for one opaque Secret reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionTaskSecretTarget {
    Environment { variable: String },
    File { path: String, mode: u32 },
    RegistryCredential,
}

impl ExecutionTaskSecretTarget {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Environment { variable } if valid_environment_name(variable) => Ok(()),
            Self::File { path, mode }
                if valid_absolute_path(path) && (1..=0o777).contains(mode) =>
            {
                Ok(())
            }
            Self::RegistryCredential => Ok(()),
            _ => Err("bound execution Task Secret target is invalid".into()),
        }
    }
}

/// One opaque Secret reference admitted by the owning Cloud context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionTaskSecret {
    name: String,
    reference: CloudSecretReference,
    target: ExecutionTaskSecretTarget,
}

impl ExecutionTaskSecret {
    pub fn new(
        name: impl Into<String>,
        reference: CloudSecretReference,
        target: ExecutionTaskSecretTarget,
    ) -> Result<Self, String> {
        let value = Self {
            name: name.into(),
            reference,
            target,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), String> {
        if !valid_name(&self.name) {
            return Err("bound execution Task Secret name is invalid".into());
        }
        self.reference.validate()?;
        self.target.validate()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn reference(&self) -> CloudSecretReference {
        self.reference
    }

    pub const fn target(&self) -> &ExecutionTaskSecretTarget {
        &self.target
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutionTaskSecretDocument {
    name: String,
    reference: String,
    target: ExecutionTaskSecretTarget,
}

impl Serialize for ExecutionTaskSecret {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ExecutionTaskSecretDocument {
            name: self.name.clone(),
            reference: self.reference.to_string(),
            target: self.target.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExecutionTaskSecret {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let document = ExecutionTaskSecretDocument::deserialize(deserializer)?;
        Self::new(
            document.name,
            CloudSecretReference::parse(&document.reference).map_err(serde::de::Error::custom)?,
            document.target,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Extra inputs allowed only for an internally-created, node-bound Execution.
/// Public ExecutionTemplate admission never accepts this value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionTaskPolicy {
    authority: ExecutionTaskAuthority,
    mounts: Vec<ExecutionTaskArtifactMount>,
    secrets: Vec<ExecutionTaskSecret>,
    semantics_profile_digest: Sha256Digest,
}

impl ExecutionTaskPolicy {
    pub fn new(
        authority: ExecutionTaskAuthority,
        mounts: Vec<ExecutionTaskArtifactMount>,
        secrets: Vec<ExecutionTaskSecret>,
        semantics_profile_digest: Sha256Digest,
    ) -> Result<Self, String> {
        let value = Self {
            authority,
            mounts,
            secrets,
            semantics_profile_digest,
        };
        value.validate_inputs()?;
        Ok(value)
    }

    pub fn validate(
        &self,
        target_node_id: NodeId,
        template: &ExecutionTemplate,
    ) -> Result<(), String> {
        template.validate()?;
        if target_node_id.as_uuid().is_nil() {
            return Err("bound execution Task target Node is invalid".into());
        }
        self.validate_inputs()
    }

    fn validate_inputs(&self) -> Result<(), String> {
        self.authority.validate()?;
        if self.mounts.is_empty()
            || self.mounts.len() > MAX_TASK_INPUTS
            || self.secrets.is_empty()
            || self.secrets.len() > MAX_TASK_INPUTS
            || Sha256Digest::parse(self.semantics_profile_digest.as_str())?
                != self.semantics_profile_digest
        {
            return Err("bound execution Task policy is invalid".into());
        }

        let mut mount_names = BTreeSet::new();
        let mut mount_targets = BTreeSet::new();
        for mount in &self.mounts {
            mount.validate()?;
            if !mount_names.insert(mount.name()) || !mount_targets.insert(mount.target()) {
                return Err(
                    "bound execution Task artifact mounts must have unique names and targets"
                        .into(),
                );
            }
        }

        let mut secret_names = BTreeSet::new();
        let mut secret_targets = BTreeSet::new();
        for secret in &self.secrets {
            secret.validate()?;
            if secret.reference().workload_revision_id != self.authority.subject_id
                || !secret_names.insert(secret.name())
                || !secret_targets.insert(secret.target())
            {
                return Err(
                    "bound execution Task Secrets must be unique and belong to its authority subject"
                        .into(),
                );
            }
        }
        Ok(())
    }

    pub const fn authority(&self) -> &ExecutionTaskAuthority {
        &self.authority
    }

    pub fn mounts(&self) -> &[ExecutionTaskArtifactMount] {
        &self.mounts
    }

    pub fn secrets(&self) -> &[ExecutionTaskSecret] {
        &self.secrets
    }

    pub const fn semantics_profile_digest(&self) -> &Sha256Digest {
        &self.semantics_profile_digest
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExecutionTaskPolicyDocument {
    authority: ExecutionTaskAuthority,
    mounts: Vec<ExecutionTaskArtifactMount>,
    secrets: Vec<ExecutionTaskSecret>,
    semantics_profile_digest: Sha256Digest,
}

impl<'de> Deserialize<'de> for ExecutionTaskPolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let document = ExecutionTaskPolicyDocument::deserialize(deserializer)?;
        Self::new(
            document.authority,
            document.mounts,
            document.secrets,
            document.semantics_profile_digest,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::artifact_uri;

    fn digest(fill: char) -> String {
        format!("sha256:{}", fill.to_string().repeat(64))
    }

    fn template() -> ExecutionTemplate {
        let image_digest = digest('a');
        ExecutionTemplate {
            artifact: super::super::ExecutionArtifact {
                uri: format!("oci://registry.example/a3s/publisher@{image_digest}"),
                digest: image_digest,
                media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            },
            process: super::super::ExecutionProcess {
                command: vec!["/usr/local/bin/a3s-cell-publisher".into()],
                args: Vec::new(),
                working_directory: Some("/workspace".into()),
                environment: std::collections::BTreeMap::new(),
            },
            input: serde_json::json!({"applicationAcl": "durable_cell_application {}"}),
            resources: super::super::ExecutionResources {
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
        ExecutionTaskPolicy::new(
            ExecutionTaskAuthority::new(
                "workload.prestart",
                subject_id,
                Sha256Digest::parse(digest('c')).expect("authority digest"),
            )
            .expect("authority"),
            vec![ExecutionTaskArtifactMount::new(
                "application-bundle",
                artifact_uri(&bundle_digest).expect("artifact URI"),
                Sha256Digest::parse(bundle_digest).expect("bundle digest"),
                DURABLE_CELL_BUNDLE_MEDIA_TYPE,
                "/workspace/bundle",
            )
            .expect("artifact mount")],
            vec![ExecutionTaskSecret::new(
                "s0-access-key-id",
                CloudSecretReference::new(subject_id, Uuid::now_v7(), 3).expect("Secret reference"),
                ExecutionTaskSecretTarget::Environment {
                    variable: "AWS_ACCESS_KEY_ID".into(),
                },
            )
            .expect("Secret")],
            Sha256Digest::parse(digest('d')).expect("semantics digest"),
        )
        .expect("policy")
    }

    #[test]
    fn accepts_only_read_only_shared_artifacts_and_exact_workload_secret_references() {
        let subject_id = Uuid::now_v7();
        let policy = policy(subject_id);
        policy
            .validate(NodeId::new(), &template())
            .expect("valid bound policy");

        let mut document = serde_json::to_value(&policy).expect("policy document");
        document["mounts"][0]["read_only"] = serde_json::json!(false);
        assert!(serde_json::from_value::<ExecutionTaskPolicy>(document).is_err());

        let mut mismatched_artifact =
            serde_json::to_value(&policy).expect("mismatched artifact policy document");
        mismatched_artifact["mounts"][0]["source"]["artifact"]["uri"] =
            serde_json::json!(artifact_uri(&digest('e')).expect("foreign artifact URI"));
        assert!(serde_json::from_value::<ExecutionTaskPolicy>(mismatched_artifact).is_err());

        let mut invalid_target =
            serde_json::to_value(&policy).expect("invalid target policy document");
        invalid_target["secrets"][0]["target"]["variable"] = serde_json::json!("1INVALID");
        assert!(serde_json::from_value::<ExecutionTaskPolicy>(invalid_target).is_err());

        let mut invalid_authority =
            serde_json::to_value(&policy).expect("invalid authority policy document");
        invalid_authority["authority"]["kind"] = serde_json::json!("Invalid.Authority");
        assert!(serde_json::from_value::<ExecutionTaskPolicy>(invalid_authority).is_err());

        let mut foreign = serde_json::to_value(&policy).expect("foreign policy document");
        foreign["secrets"][0]["reference"] =
            serde_json::json!(CloudSecretReference::new(Uuid::now_v7(), Uuid::now_v7(), 3)
                .expect("foreign reference")
                .to_string());
        assert!(serde_json::from_value::<ExecutionTaskPolicy>(foreign).is_err());
    }

    #[test]
    fn persisted_policy_document_remains_compatible_without_runtime_domain_types() {
        let policy = policy(Uuid::now_v7());
        let document = serde_json::to_value(&policy).expect("policy document");
        assert_eq!(document["mounts"][0]["source"]["kind"], "artifact");
        assert_eq!(document["mounts"][0]["read_only"], true);
        assert_eq!(
            document["mounts"][0]["source"]["artifact"]["uri"],
            policy.mounts()[0].artifact_uri().expect("artifact URI")
        );
        assert_eq!(
            document["mounts"][0]["source"]["artifact"]["media_type"],
            DURABLE_CELL_BUNDLE_MEDIA_TYPE
        );
        assert_eq!(document["secrets"][0]["target"]["kind"], "environment");
        assert!(document.get("semanticsProfileDigest").is_some());
        assert_eq!(
            serde_json::from_value::<ExecutionTaskPolicy>(document).expect("restore policy"),
            policy
        );
    }
}
