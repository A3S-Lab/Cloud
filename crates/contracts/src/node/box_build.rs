use a3s_runtime::contract::{ArtifactRef, RuntimeOutputArtifact};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

use super::{
    validate_cloud_artifact, validate_lower_sha256, validate_single_line,
    NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};

pub const BOX_BUILD_OUTPUT_NAME: &str = "oci-layout";
const BOX_BUILD_CACHE_OUTPUT_PREFIX: &str = "build-cache-";
const OCI_IMAGE_INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";
const OCI_IMAGE_MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
const MAX_BUILD_PLANS: usize = 8;
const MAX_BUILD_PLAN_BYTES: usize = 16 * 1024;
const MAX_BUILD_CACHE_ENTRIES: u64 = 16 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 10 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeBoxBuildPlatform {
    pub os: String,
    pub architecture: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

impl NodeBoxBuildPlatform {
    pub fn validate(&self) -> Result<(), String> {
        if self.os != "linux" || !matches!(self.architecture.as_str(), "amd64" | "arm64") {
            return Err("Box build platform must be linux/amd64 or linux/arm64".into());
        }
        if self.variant.as_deref().is_some_and(|variant| {
            validate_single_line("Box build platform variant", variant, 64).is_err()
        }) {
            return Err("Box build platform variant is invalid".into());
        }
        Ok(())
    }

    fn identity(&self) -> String {
        format!(
            "{}/{}/{}",
            self.os,
            self.architecture,
            self.variant.as_deref().unwrap_or("")
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeBoxBuildDescriptor {
    pub media_type: String,
    pub digest: String,
    pub size: u64,
}

impl NodeBoxBuildDescriptor {
    fn validate(&self, expected_media_type: &str) -> Result<(), String> {
        if self.media_type != expected_media_type || self.size == 0 {
            return Err("Box build descriptor media type or size is invalid".into());
        }
        validate_lower_sha256("Box build descriptor digest", &self.digest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeBoxBuildCacheReceipt {
    pub schema: String,
    pub key: String,
    pub source_digest: String,
    pub plan_digest: String,
    pub descriptor: NodeBoxBuildDescriptor,
    pub platform: NodeBoxBuildPlatform,
    pub content_bytes: u64,
    pub entry_count: u64,
    pub blob_count: u64,
    pub blob_inventory_digest: String,
}

impl NodeBoxBuildCacheReceipt {
    pub const SCHEMA: &'static str = "a3s.box.build-cache-receipt.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err("Box build cache receipt schema is unsupported".into());
        }
        validate_lower_sha256("Box build cache key", &self.key)?;
        validate_lower_sha256("Box build cache source digest", &self.source_digest)?;
        validate_lower_sha256("Box build cache plan digest", &self.plan_digest)?;
        validate_lower_sha256(
            "Box build cache blob inventory digest",
            &self.blob_inventory_digest,
        )?;
        self.descriptor.validate(OCI_IMAGE_MANIFEST_MEDIA_TYPE)?;
        self.platform.validate()?;
        if self.content_bytes < self.descriptor.size
            || self.entry_count > MAX_BUILD_CACHE_ENTRIES
            || self.blob_count < 2
        {
            return Err("Box build cache receipt bounds are invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeBoxBuildCacheInput {
    pub artifact: ArtifactRef,
    pub receipt: NodeBoxBuildCacheReceipt,
}

impl NodeBoxBuildCacheInput {
    fn validate(&self, source_digest: &str, plan_digest: &str) -> Result<(), String> {
        validate_cloud_artifact(&self.artifact)?;
        self.receipt.validate()?;
        if self.receipt.source_digest != source_digest {
            return Err("Box build cache source does not match the build input Artifact".into());
        }
        if self.receipt.plan_digest != plan_digest {
            return Err("Box build cache does not match the canonical build plan".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeBoxBuildPlan {
    pub operation_id: String,
    pub plan_acl: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<NodeBoxBuildCacheInput>,
}

impl NodeBoxBuildPlan {
    fn validate(&self, source_digest: &str) -> Result<(), String> {
        validate_single_line("Box build operation ID", &self.operation_id, 255)?;
        if self.plan_acl.is_empty()
            || self.plan_acl.len() > MAX_BUILD_PLAN_BYTES
            || self.plan_acl.contains('\0')
            || !self.plan_acl.ends_with('\n')
        {
            return Err("Box build plan must be bounded canonical A3S ACL".into());
        }
        if let Some(cache) = &self.cache {
            cache.validate(source_digest, &self.plan_digest())?;
        }
        Ok(())
    }

    fn plan_digest(&self) -> String {
        format!("sha256:{:x}", Sha256::digest(self.plan_acl.as_bytes()))
    }

    pub fn cache_output_name(&self) -> String {
        format!(
            "{BOX_BUILD_CACHE_OUTPUT_PREFIX}{:x}",
            Sha256::digest(self.operation_id.as_bytes())
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeBoxBuildRequest {
    pub schema: String,
    pub generation: u64,
    pub source: ArtifactRef,
    pub plans: Vec<NodeBoxBuildPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly_reference: Option<String>,
    pub output_max_bytes: u64,
    pub cache_max_bytes: u64,
}

impl NodeBoxBuildRequest {
    pub const SCHEMA: &'static str = "a3s.cloud.box-build-request.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA || self.generation == 0 {
            return Err("Box build request schema or generation is invalid".into());
        }
        validate_cloud_artifact(&self.source)?;
        if !(1..=MAX_BUILD_PLANS).contains(&self.plans.len()) {
            return Err("Box build request must contain between one and eight plans".into());
        }
        let mut operation_ids = BTreeSet::new();
        let mut output_names = BTreeSet::new();
        for plan in &self.plans {
            plan.validate(&self.source.digest)?;
            if !operation_ids.insert(plan.operation_id.as_str())
                || !output_names.insert(plan.cache_output_name())
            {
                return Err("Box build plan identities must be unique".into());
            }
        }
        if self
            .plans
            .windows(2)
            .any(|plans| plans[0].operation_id >= plans[1].operation_id)
        {
            return Err("Box build plans must be sorted by operation ID".into());
        }
        match (&self.assembly_reference, self.plans.len()) {
            (None, 1) => {}
            (Some(reference), count) if count > 1 => {
                validate_single_line("Box build assembly reference", reference, 4096)?;
            }
            _ => {
                return Err(
                    "Box build assembly reference must exist only for multi-platform output".into(),
                )
            }
        }
        if !(1..=MAX_ARTIFACT_BYTES).contains(&self.output_max_bytes)
            || !(1..=MAX_ARTIFACT_BYTES).contains(&self.cache_max_bytes)
        {
            return Err("Box build Artifact bounds are invalid".into());
        }
        Ok(())
    }

    pub fn binding_digest(&self) -> Result<String, String> {
        self.validate()?;
        let encoded = serde_json::to_vec(self)
            .map_err(|error| format!("could not encode Box build request: {error}"))?;
        Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum NodeBoxBuildPhase {
    Running,
    Cancelling,
    Succeeded,
    Cancelled { message: String },
    Failed { message: String },
}

impl NodeBoxBuildPhase {
    pub(crate) fn validate(&self) -> Result<(), String> {
        match self {
            Self::Cancelled { message } | Self::Failed { message } => {
                validate_single_line("Box build terminal message", message, 4 * 1024)
            }
            Self::Running | Self::Cancelling | Self::Succeeded => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeBoxBuildStartResult {
    pub binding_digest: String,
    pub phase: NodeBoxBuildPhase,
}

impl NodeBoxBuildStartResult {
    pub fn validate_for(&self, request: &NodeBoxBuildRequest) -> Result<(), String> {
        request.validate()?;
        self.phase.validate()?;
        if self.binding_digest != request.binding_digest()? {
            return Err("Box build start result changed the request identity".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeBoxBuildCacheOutput {
    pub operation_id: String,
    pub artifact: RuntimeOutputArtifact,
    pub receipt: NodeBoxBuildCacheReceipt,
}

impl NodeBoxBuildCacheOutput {
    pub fn validate(&self) -> Result<(), String> {
        self.receipt.validate()?;
        validate_single_line("Box build cache operation ID", &self.operation_id, 255)?;
        validate_single_line("Box build cache output name", &self.artifact.name, 255)?;
        if self.artifact.artifact.media_type != NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE
            || self.artifact.size_bytes == 0
            || self.artifact.size_bytes > MAX_ARTIFACT_BYTES
        {
            return Err("Box build cache output identity or bounds are invalid".into());
        }
        validate_cloud_artifact(&self.artifact.artifact)
    }

    fn validate_for(
        &self,
        plan: &NodeBoxBuildPlan,
        request: &NodeBoxBuildRequest,
    ) -> Result<(), String> {
        self.validate()?;
        if self.operation_id != plan.operation_id
            || self.artifact.name != plan.cache_output_name()
            || self.artifact.size_bytes > request.cache_max_bytes
            || self.receipt.source_digest != request.source.digest
            || self.receipt.plan_digest != plan.plan_digest()
        {
            return Err("Box build cache output changed its admitted identity or bound".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeBoxBuildOutput {
    pub artifact: RuntimeOutputArtifact,
    pub descriptor: NodeBoxBuildDescriptor,
    pub platforms: Vec<NodeBoxBuildPlatform>,
    pub manifest_count: u64,
    pub content_bytes: u64,
    pub blob_count: u64,
    pub blob_inventory_digest: String,
    pub caches: Vec<NodeBoxBuildCacheOutput>,
}

impl NodeBoxBuildOutput {
    pub fn validate(&self) -> Result<(), String> {
        validate_single_line("Box build output name", &self.artifact.name, 255)?;
        validate_cloud_artifact(&self.artifact.artifact)?;
        if !matches!(
            self.descriptor.media_type.as_str(),
            OCI_IMAGE_MANIFEST_MEDIA_TYPE | OCI_IMAGE_INDEX_MEDIA_TYPE
        ) {
            return Err("Box build output descriptor media type is invalid".into());
        }
        self.descriptor.validate(&self.descriptor.media_type)?;
        validate_lower_sha256(
            "Box build output blob inventory digest",
            &self.blob_inventory_digest,
        )?;
        if self.artifact.name != BOX_BUILD_OUTPUT_NAME
            || self.artifact.artifact.media_type != NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE
            || self.artifact.size_bytes == 0
            || self.artifact.size_bytes > MAX_ARTIFACT_BYTES
            || !(1..=MAX_BUILD_PLANS).contains(&self.platforms.len())
            || self.manifest_count != self.platforms.len() as u64
            || self.content_bytes < self.descriptor.size
            || self.blob_count < 2
            || self.caches.len() != self.platforms.len()
        {
            return Err("Box build output identity or bounds are invalid".into());
        }
        let mut platform_ids = BTreeSet::new();
        for platform in &self.platforms {
            platform.validate()?;
            if !platform_ids.insert(platform.identity()) {
                return Err("Box build output platforms must be unique".into());
            }
        }
        if self
            .platforms
            .windows(2)
            .any(|platforms| platforms[0].identity() >= platforms[1].identity())
        {
            return Err("Box build output platforms must be sorted".into());
        }
        let mut operation_ids = BTreeSet::new();
        let mut cache_platforms = BTreeSet::new();
        let mut source_digest = None;
        for cache in &self.caches {
            cache.validate()?;
            if !operation_ids.insert(cache.operation_id.as_str())
                || !cache_platforms.insert(cache.receipt.platform.identity())
                || source_digest
                    .replace(cache.receipt.source_digest.as_str())
                    .is_some_and(|expected| expected != cache.receipt.source_digest.as_str())
            {
                return Err(
                    "Box build cache outputs must have unique consistent identities".into(),
                );
            }
        }
        if cache_platforms != platform_ids {
            return Err("Box build cache platforms must match the output platforms".into());
        }
        Ok(())
    }

    fn validate_for(&self, request: &NodeBoxBuildRequest) -> Result<(), String> {
        self.validate()?;
        let expected_media_type = if request.plans.len() == 1 {
            OCI_IMAGE_MANIFEST_MEDIA_TYPE
        } else {
            OCI_IMAGE_INDEX_MEDIA_TYPE
        };
        self.descriptor.validate(expected_media_type)?;
        if self.artifact.size_bytes > request.output_max_bytes
            || self.platforms.len() != request.plans.len()
        {
            return Err("Box build output changed its admitted identity or bounds".into());
        }
        for (cache, plan) in self.caches.iter().zip(&request.plans) {
            cache.validate_for(plan, request)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum NodeBoxBuildInspection {
    Running {
        binding_digest: String,
    },
    Cancelling {
        binding_digest: String,
    },
    Cancelled {
        binding_digest: String,
        message: String,
    },
    Failed {
        binding_digest: String,
        message: String,
    },
    Succeeded {
        binding_digest: String,
        output: Box<NodeBoxBuildOutput>,
    },
}

impl NodeBoxBuildInspection {
    pub fn validate_for(&self, request: &NodeBoxBuildRequest) -> Result<(), String> {
        request.validate()?;
        let binding_digest = match self {
            Self::Running { binding_digest }
            | Self::Cancelling { binding_digest }
            | Self::Cancelled { binding_digest, .. }
            | Self::Failed { binding_digest, .. }
            | Self::Succeeded { binding_digest, .. } => binding_digest,
        };
        if binding_digest != &request.binding_digest()? {
            return Err("Box build inspection changed the request identity".into());
        }
        match self {
            Self::Cancelled { message, .. } | Self::Failed { message, .. } => {
                validate_single_line("Box build terminal message", message, 4 * 1024)
            }
            Self::Succeeded { output, .. } => output.validate_for(request),
            Self::Running { .. } | Self::Cancelling { .. } => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeBoxBuildCancellation {
    NotFound,
    Requested,
    AlreadyRequested,
    AlreadyCancelled,
    AlreadyTerminal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeBoxBuildOperationCancellation {
    pub operation_id: String,
    pub outcome: NodeBoxBuildCancellation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeBoxBuildCancelResult {
    pub binding_digest: String,
    pub operations: Vec<NodeBoxBuildOperationCancellation>,
}

impl NodeBoxBuildCancelResult {
    pub fn validate_for(&self, request: &NodeBoxBuildRequest) -> Result<(), String> {
        request.validate()?;
        if self.binding_digest != request.binding_digest()?
            || self.operations.len() != request.plans.len()
            || self
                .operations
                .iter()
                .zip(&request.plans)
                .any(|(operation, plan)| operation.operation_id != plan.operation_id)
        {
            return Err("Box build cancellation changed the request operations".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeBoxBuildOperationRemoval {
    pub operation_id: String,
    pub removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeBoxBuildRemoveResult {
    pub binding_digest: String,
    pub operations: Vec<NodeBoxBuildOperationRemoval>,
    pub assembly_removed: bool,
}

impl NodeBoxBuildRemoveResult {
    pub fn validate_for(&self, request: &NodeBoxBuildRequest) -> Result<(), String> {
        request.validate()?;
        if self.binding_digest != request.binding_digest()?
            || self.operations.len() != request.plans.len()
            || self
                .operations
                .iter()
                .zip(&request.plans)
                .any(|(removed, plan)| removed.operation_id != plan.operation_id)
            || request.assembly_reference.is_none() && self.assembly_removed
        {
            return Err("Box build removal changed the request operations".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        artifact_uri, NodeCommandAck, NodeCommandEnvelope, NodeCommandMetadata, NodeCommandOutcome,
        NodeCommandPayload, NodeCommandResult,
    };
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    fn digest(fill: char) -> String {
        format!("sha256:{}", fill.to_string().repeat(64))
    }

    fn artifact(fill: char) -> ArtifactRef {
        let digest = digest(fill);
        ArtifactRef {
            uri: artifact_uri(&digest).expect("artifact URI"),
            digest,
            media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
        }
    }

    fn request() -> NodeBoxBuildRequest {
        NodeBoxBuildRequest {
            schema: NodeBoxBuildRequest::SCHEMA.into(),
            generation: 1,
            source: artifact('a'),
            plans: vec![NodeBoxBuildPlan {
                operation_id: "build-1-linux-amd64".into(),
                plan_acl: concat!(
                    "build \"oci\" {\n",
                    "  cache = \"content-addressed\"\n",
                    "  context = \".\"\n",
                    "  file = \"Containerfile\"\n",
                    "  network = \"none\"\n",
                    "  platform = \"linux/amd64\"\n",
                    "  schema = \"a3s.box.build-plan.v1\"\n",
                    "}\n",
                )
                .into(),
                cache: None,
            }],
            assembly_reference: None,
            output_max_bytes: 1024 * 1024,
            cache_max_bytes: 1024 * 1024,
        }
    }

    fn cache_receipt(source_digest: String, plan_digest: String) -> NodeBoxBuildCacheReceipt {
        NodeBoxBuildCacheReceipt {
            schema: NodeBoxBuildCacheReceipt::SCHEMA.into(),
            key: digest('c'),
            source_digest,
            plan_digest,
            descriptor: NodeBoxBuildDescriptor {
                media_type: OCI_IMAGE_MANIFEST_MEDIA_TYPE.into(),
                digest: digest('f'),
                size: 100,
            },
            platform: NodeBoxBuildPlatform {
                os: "linux".into(),
                architecture: "amd64".into(),
                variant: None,
            },
            content_bytes: 200,
            entry_count: 1,
            blob_count: 2,
            blob_inventory_digest: digest('1'),
        }
    }

    fn successful_inspection(request: &NodeBoxBuildRequest) -> NodeBoxBuildInspection {
        let plan = &request.plans[0];
        NodeBoxBuildInspection::Succeeded {
            binding_digest: request.binding_digest().expect("binding digest"),
            output: Box::new(NodeBoxBuildOutput {
                artifact: RuntimeOutputArtifact {
                    name: BOX_BUILD_OUTPUT_NAME.into(),
                    artifact: artifact('b'),
                    size_bytes: 4096,
                },
                descriptor: NodeBoxBuildDescriptor {
                    media_type: OCI_IMAGE_MANIFEST_MEDIA_TYPE.into(),
                    digest: digest('c'),
                    size: 512,
                },
                platforms: vec![NodeBoxBuildPlatform {
                    os: "linux".into(),
                    architecture: "amd64".into(),
                    variant: None,
                }],
                manifest_count: 1,
                content_bytes: 2048,
                blob_count: 3,
                blob_inventory_digest: digest('d'),
                caches: vec![NodeBoxBuildCacheOutput {
                    operation_id: plan.operation_id.clone(),
                    artifact: RuntimeOutputArtifact {
                        name: plan.cache_output_name(),
                        artifact: artifact('e'),
                        size_bytes: 1024,
                    },
                    receipt: cache_receipt(request.source.digest.clone(), plan.plan_digest()),
                }],
            }),
        }
    }

    #[test]
    fn request_identity_is_closed_and_ordered() {
        let request = request();
        request.validate().expect("valid Box build request");
        assert_eq!(request.binding_digest().expect("binding digest").len(), 71);

        let mut invalid = request.clone();
        invalid.plans.push(invalid.plans[0].clone());
        assert_eq!(
            invalid.validate().expect_err("duplicate plan"),
            "Box build plan identities must be unique"
        );
    }

    #[test]
    fn cache_input_must_bind_the_exact_source() {
        let mut request = request();
        request.plans[0].cache = Some(NodeBoxBuildCacheInput {
            artifact: artifact('b'),
            receipt: cache_receipt(digest('d'), request.plans[0].plan_digest()),
        });
        assert_eq!(
            request.validate().expect_err("source mismatch"),
            "Box build cache source does not match the build input Artifact"
        );
    }

    #[test]
    fn cache_input_must_bind_the_exact_canonical_plan() {
        let mut request = request();
        request.plans[0].cache = Some(NodeBoxBuildCacheInput {
            artifact: artifact('b'),
            receipt: cache_receipt(request.source.digest.clone(), digest('e')),
        });
        assert_eq!(
            request.validate().expect_err("plan mismatch"),
            "Box build cache does not match the canonical build plan"
        );
    }

    #[test]
    fn successful_inspection_requires_one_cache_per_plan() {
        let request = request();
        let inspection = NodeBoxBuildInspection::Succeeded {
            binding_digest: request.binding_digest().expect("binding digest"),
            output: Box::new(NodeBoxBuildOutput {
                artifact: RuntimeOutputArtifact {
                    name: BOX_BUILD_OUTPUT_NAME.into(),
                    artifact: artifact('b'),
                    size_bytes: 4096,
                },
                descriptor: NodeBoxBuildDescriptor {
                    media_type: OCI_IMAGE_MANIFEST_MEDIA_TYPE.into(),
                    digest: digest('c'),
                    size: 512,
                },
                platforms: vec![NodeBoxBuildPlatform {
                    os: "linux".into(),
                    architecture: "amd64".into(),
                    variant: None,
                }],
                manifest_count: 1,
                content_bytes: 2048,
                blob_count: 3,
                blob_inventory_digest: digest('d'),
                caches: Vec::new(),
            }),
        };
        assert_eq!(
            inspection
                .validate_for(&request)
                .expect_err("missing cache"),
            "Box build output identity or bounds are invalid"
        );
    }

    #[test]
    fn successful_inspection_binds_each_cache_to_its_canonical_plan() {
        let request = request();
        let mut inspection = successful_inspection(&request);
        inspection
            .validate_for(&request)
            .expect("matching Box build output");

        let NodeBoxBuildInspection::Succeeded { output, .. } = &mut inspection else {
            panic!("inspection must succeed");
        };
        output.caches[0].receipt.plan_digest = digest('9');
        assert_eq!(
            inspection
                .validate_for(&request)
                .expect_err("changed plan digest"),
            "Box build cache output changed its admitted identity or bound"
        );
    }

    #[test]
    fn node_command_acknowledgement_binds_the_box_build_action_and_request() {
        let request = request();
        let issued_at = Utc::now();
        let command = NodeCommandEnvelope::new(
            NodeCommandMetadata {
                command_id: Uuid::now_v7(),
                lease_id: Uuid::now_v7(),
                node_id: Uuid::now_v7(),
                sequence: 1,
                aggregate_id: Uuid::now_v7(),
                issued_at,
                not_after: issued_at + Duration::minutes(1),
                correlation_id: Uuid::now_v7(),
            },
            NodeCommandPayload::BoxBuildStart {
                request: Box::new(request.clone()),
            },
        )
        .expect("Box build start command");
        let mut acknowledgement = NodeCommandAck {
            schema: NodeCommandAck::SCHEMA.into(),
            command_id: command.command_id,
            lease_id: command.lease_id,
            node_id: command.node_id,
            sequence: command.sequence,
            payload_digest: command.payload_digest.clone(),
            completed_at: issued_at + Duration::seconds(1),
            outcome: NodeCommandOutcome::Succeeded {
                result: Box::new(NodeCommandResult::BoxBuildStarted {
                    started: NodeBoxBuildStartResult {
                        binding_digest: request.binding_digest().expect("binding digest"),
                        phase: NodeBoxBuildPhase::Running,
                    },
                }),
            },
        };
        acknowledgement
            .validate_against(&command)
            .expect("matching Box build acknowledgement");

        let NodeCommandOutcome::Succeeded { result } = &mut acknowledgement.outcome else {
            panic!("acknowledgement must succeed");
        };
        let NodeCommandResult::BoxBuildStarted { started } = result.as_mut() else {
            panic!("acknowledgement must contain Box build start evidence");
        };
        started.binding_digest = digest('f');
        assert_eq!(
            acknowledgement
                .validate_against(&command)
                .expect_err("changed binding"),
            "Box build start result changed the request identity"
        );
    }
}
