use crate::modules::artifacts::domain::{
    BuildEvidence, BuildEvidenceVerificationState, BuildRun, BuildRunStatus, OciPublicationTarget,
    PublishedOciArtifact, ValidatedOciBuildOutput,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildRunResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub id: Uuid,
    pub source_revision_id: Uuid,
    pub attempt: u32,
    pub retry_of_build_run_id: Option<Uuid>,
    pub operation_id: Uuid,
    pub status: BuildRunStatus,
    pub source_content_digest: Option<String>,
    pub output: Option<ValidatedOciBuildOutputResponse>,
    pub publication_target: Option<OciPublicationTarget>,
    pub published_artifact: Option<PublishedOciArtifact>,
    pub evidence_summary: Option<BuildEvidenceSummaryResponse>,
    pub failure: Option<String>,
    pub aggregate_version: u64,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub cancellation_requested_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl From<BuildRun> for BuildRunResponse {
    fn from(build_run: BuildRun) -> Self {
        let evidence_summary = build_run
            .evidence
            .as_deref()
            .map(BuildEvidenceSummaryResponse::from);
        Self {
            organization_id: build_run.organization_id.as_uuid(),
            project_id: build_run.project_id.as_uuid(),
            environment_id: build_run.environment_id.as_uuid(),
            id: build_run.id.as_uuid(),
            source_revision_id: build_run.source_revision_id.as_uuid(),
            attempt: build_run.attempt,
            retry_of_build_run_id: build_run.retry_of_build_run_id.map(|id| id.as_uuid()),
            operation_id: build_run.operation_id.as_uuid(),
            status: build_run.status,
            source_content_digest: build_run.source_content_digest,
            output: build_run.output.map(ValidatedOciBuildOutputResponse::from),
            publication_target: build_run.publication_target,
            published_artifact: build_run.published_artifact,
            evidence_summary,
            failure: build_run.failure,
            aggregate_version: build_run.aggregate_version,
            requested_at: build_run.requested_at,
            updated_at: build_run.updated_at,
            started_at: build_run.started_at,
            cancellation_requested_at: build_run.cancellation_requested_at,
            finished_at: build_run.finished_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildEvidenceSummaryResponse {
    pub schema: String,
    pub verification_state: BuildEvidenceVerificationState,
    pub sbom_digest: String,
    pub provenance_digest: String,
    pub signing_key_algorithm: String,
    pub signing_key_id: String,
    pub signing_key_version: Option<u32>,
    pub attested_at: DateTime<Utc>,
}

impl From<&BuildEvidence> for BuildEvidenceSummaryResponse {
    fn from(evidence: &BuildEvidence) -> Self {
        Self {
            schema: evidence.schema.clone(),
            verification_state: evidence.verification_state,
            sbom_digest: evidence.sbom_digest.clone(),
            provenance_digest: evidence.provenance_digest.clone(),
            signing_key_algorithm: evidence.signing_key.algorithm.clone(),
            signing_key_id: evidence.signing_key.key_id.clone(),
            signing_key_version: evidence.signing_key.key_version,
            attested_at: evidence.attested_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedOciBuildOutputResponse {
    pub descriptor: crate::modules::artifacts::domain::OciDescriptor,
    pub platforms: Vec<String>,
    pub content_bytes: u64,
    pub blob_count: usize,
}

impl From<ValidatedOciBuildOutput> for ValidatedOciBuildOutputResponse {
    fn from(output: ValidatedOciBuildOutput) -> Self {
        Self {
            descriptor: output.descriptor,
            platforms: output
                .platforms
                .into_iter()
                .map(|platform| platform.as_str().to_owned())
                .collect(),
            content_bytes: output.content_bytes,
            blob_count: output.blob_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::artifacts::domain::{
        BuildArtifact, BuildRun, OciDescriptor, ValidatedOciBuildOutput,
        OCI_IMAGE_MANIFEST_MEDIA_TYPE,
    };
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, NodeCommandId, NodeId, OrganizationId, ProjectId, SourceRevisionId,
    };
    use crate::modules::sources::domain::BuildPlatform;

    #[test]
    fn build_run_response_uses_the_public_camel_case_contract() {
        let mut build = BuildRun::reserve(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            SourceRevisionId::new(),
            Utc::now(),
        );
        let digest = format!("sha256:{}", "a".repeat(64));
        let input_artifact = BuildArtifact::new(
            "artifact://internal/build-input",
            &digest,
            "application/vnd.a3s.build-input.v1.tar+gzip",
            64,
        )
        .expect("internal input artifact");
        let runtime_output_artifact = BuildArtifact::new(
            "artifact://internal/runtime-output",
            &digest,
            "application/vnd.oci.image.layout.v1.tar+gzip",
            128,
        )
        .expect("internal runtime output artifact");
        build.input_artifact = Some(input_artifact);
        build.node_id = Some(NodeId::new());
        build.command_id = Some(NodeCommandId::new());
        build.cleanup_command_id = Some(NodeCommandId::new());
        build.runtime_spec_digest = Some(format!("sha256:{}", "b".repeat(64)));
        build.runtime_output_artifact = Some(runtime_output_artifact.clone());
        build.output = Some(ValidatedOciBuildOutput {
            artifact: runtime_output_artifact,
            descriptor: OciDescriptor::new(OCI_IMAGE_MANIFEST_MEDIA_TYPE, digest, 64)
                .expect("OCI descriptor"),
            platforms: vec![BuildPlatform::parse("linux/amd64").expect("build platform")],
            content_bytes: 128,
            blob_count: 2,
        });
        let encoded = serde_json::to_value(BuildRunResponse::from(build)).expect("response");
        assert_eq!(encoded["status"], "queued");
        assert!(encoded.get("sourceRevisionId").is_some());
        assert_eq!(encoded["attempt"], 1);
        assert!(encoded["retryOfBuildRunId"].is_null());
        assert!(encoded.get("operationId").is_some());
        assert!(encoded.get("cancellationRequestedAt").is_some());
        assert!(encoded.get("source_revision_id").is_none());
        for private_field in [
            "inputArtifact",
            "nodeId",
            "commandId",
            "cleanupCommandId",
            "runtimeSpecDigest",
            "runtimeOutputArtifact",
        ] {
            assert!(encoded.get(private_field).is_none());
        }
        assert!(encoded["output"].get("artifact").is_none());
        let encoded = encoded.to_string();
        assert!(!encoded.contains("artifact://internal/build-input"));
        assert!(!encoded.contains("artifact://internal/runtime-output"));
    }
}
