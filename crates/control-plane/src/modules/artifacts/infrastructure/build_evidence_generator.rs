use super::buildkit_build_service::OciLayoutBlob;
use super::RuntimeBuildOutputValidator;
use crate::modules::artifacts::domain::{
    canonical_json, dsse_pae, sha256_digest, BuildEvidence, BuildEvidenceBuilder,
    BuildEvidenceGenerationError, BuildEvidenceSigningError, BuildEvidenceVerificationState,
    BuildRun, BuildRunStatus, DsseEnvelope, DsseSignature, IBuildEvidenceGenerator,
    IBuildEvidenceSigner, InTotoSubject, SlsaBuildDefinition, SlsaBuilder, SlsaExternalParameters,
    SlsaInternalParameters, SlsaProvenancePredicate, SlsaProvenanceStatement,
    SlsaResourceDescriptor, SlsaRunDetails, SlsaRunMetadata, SpdxChecksum, SpdxCreationInfo,
    SpdxDocument, SpdxFile, SpdxPackage, SpdxRelationship, BUILD_EVIDENCE_SCHEMA,
    DSSE_PAYLOAD_TYPE, IN_TOTO_STATEMENT_TYPE, SLSA_BUILD_TYPE, SLSA_PROVENANCE_PREDICATE_TYPE,
    SPDX_VERSION,
};
use crate::modules::sources::domain::ExternalSourceRevision;
use a3s_runtime::contract::ArtifactRef;
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct RuntimeBuildEvidenceGenerator {
    outputs: Arc<RuntimeBuildOutputValidator>,
    signer: Arc<dyn IBuildEvidenceSigner>,
    builder: BuildEvidenceBuilder,
}

impl RuntimeBuildEvidenceGenerator {
    pub fn new(
        outputs: Arc<RuntimeBuildOutputValidator>,
        signer: Arc<dyn IBuildEvidenceSigner>,
        builder: ArtifactRef,
    ) -> Result<Self, String> {
        builder.validate()?;
        let builder = BuildEvidenceBuilder {
            uri: builder.uri,
            digest: builder.digest,
        };
        builder.validate()?;
        Ok(Self {
            outputs,
            signer,
            builder,
        })
    }
}

#[async_trait]
impl IBuildEvidenceGenerator for RuntimeBuildEvidenceGenerator {
    async fn generate(
        &self,
        build: &BuildRun,
        revision: &ExternalSourceRevision,
        attested_at: DateTime<Utc>,
    ) -> Result<BuildEvidence, BuildEvidenceGenerationError> {
        validate_request(build, revision)?;
        let attested_at = crate::modules::shared_kernel::domain::canonical_timestamp(attested_at);
        if attested_at < build.updated_at {
            return Err(BuildEvidenceGenerationError::Invalid(
                "build evidence attestation time regressed".into(),
            ));
        }
        let output = build.output.as_ref().ok_or_else(|| {
            BuildEvidenceGenerationError::Invalid(
                "build evidence requires validated OCI output".into(),
            )
        })?;
        let materialized = self
            .outputs
            .materialize_validated_output(output)
            .await
            .map_err(map_output_error)?;
        let blobs = materialized.validated.blobs.clone();
        materialized.cleanup().await;
        if blobs.len() != output.blob_count {
            return Err(BuildEvidenceGenerationError::Integrity(
                "validated OCI descriptor inventory changed before attestation".into(),
            ));
        }

        let artifact = build.published_artifact.clone().ok_or_else(|| {
            BuildEvidenceGenerationError::Invalid(
                "build evidence requires a published OCI artifact".into(),
            )
        })?;
        let sbom = build_spdx(build, &artifact, &blobs, attested_at)?;
        let sbom_bytes = canonical_json(&sbom).map_err(BuildEvidenceGenerationError::Integrity)?;
        let sbom_digest = sha256_digest(&sbom_bytes);
        let provenance = build_provenance(
            build,
            revision,
            &self.builder,
            &artifact,
            &sbom,
            &sbom_digest,
            attested_at,
        )?;
        let provenance_bytes =
            canonical_json(&provenance).map_err(BuildEvidenceGenerationError::Integrity)?;
        let provenance_digest = sha256_digest(&provenance_bytes);
        let pae = dsse_pae(DSSE_PAYLOAD_TYPE, &provenance_bytes)
            .map_err(BuildEvidenceGenerationError::Invalid)?;
        let signature = self.signer.sign(&pae).await.map_err(map_signing_error)?;
        let envelope = DsseEnvelope {
            payload_type: DSSE_PAYLOAD_TYPE.into(),
            payload: STANDARD.encode(&provenance_bytes),
            signatures: vec![DsseSignature {
                key_id: signature.key.key_id.clone(),
                signature: STANDARD.encode(&signature.signature),
            }],
        };
        BuildEvidence::restore(BuildEvidence {
            schema: BUILD_EVIDENCE_SCHEMA.into(),
            build_run_id: build.id,
            operation_id: build.operation_id,
            source_revision_id: build.source_revision_id,
            attempt: build.attempt,
            repository: revision.repository.canonical_url().into(),
            commit_sha: revision.commit_sha.as_str().into(),
            source_content_digest: build.source_content_digest.clone().ok_or_else(|| {
                BuildEvidenceGenerationError::Invalid(
                    "build evidence omitted its source content digest".into(),
                )
            })?,
            recipe: revision.recipe.clone(),
            recipe_digest: revision.recipe_digest.clone(),
            runtime_spec_digest: build.runtime_spec_digest.clone().ok_or_else(|| {
                BuildEvidenceGenerationError::Invalid(
                    "build evidence omitted its Runtime specification digest".into(),
                )
            })?,
            builder: self.builder.clone(),
            platforms: output.platforms.clone(),
            artifact,
            sbom,
            sbom_digest,
            provenance,
            provenance_digest,
            envelope,
            signing_key: signature.key,
            verification_state: BuildEvidenceVerificationState::Verified,
            attested_at,
        })
        .map_err(BuildEvidenceGenerationError::Integrity)
    }
}

fn validate_request(
    build: &BuildRun,
    revision: &ExternalSourceRevision,
) -> Result<(), BuildEvidenceGenerationError> {
    if !build.evidence_required
        || build.evidence.is_some()
        || !matches!(
            build.status,
            BuildRunStatus::Attesting | BuildRunStatus::Cancelling
        )
        || build.published_artifact.is_none()
        || build.output.is_none()
        || build.organization_id != revision.organization_id
        || build.project_id != revision.project_id
        || build.environment_id != revision.environment_id
        || build.source_revision_id != revision.id
    {
        return Err(BuildEvidenceGenerationError::Invalid(
            "build evidence request changed its durable build or source identity".into(),
        ));
    }
    revision
        .clone()
        .validate()
        .map_err(BuildEvidenceGenerationError::Invalid)?;
    Ok(())
}

fn build_spdx(
    build: &BuildRun,
    artifact: &crate::modules::artifacts::domain::PublishedOciArtifact,
    blobs: &[OciLayoutBlob],
    created: DateTime<Utc>,
) -> Result<SpdxDocument, BuildEvidenceGenerationError> {
    if blobs.is_empty() || blobs.len() > 100_000 {
        return Err(BuildEvidenceGenerationError::Invalid(
            "OCI descriptor inventory exceeds the SPDX evidence bound".into(),
        ));
    }
    let artifact_hex = digest_hex(&artifact.digest)?;
    let package_id = "SPDXRef-Package-OCI".to_owned();
    let mut files = Vec::with_capacity(blobs.len());
    let mut relationships = Vec::with_capacity(blobs.len() + 1);
    relationships.push(SpdxRelationship {
        spdx_element_id: "SPDXRef-DOCUMENT".into(),
        relationship_type: "DESCRIBES".into(),
        related_spdx_element: package_id.clone(),
    });
    for blob in blobs {
        let digest = digest_hex(&blob.digest)?;
        let spdx_id = format!("SPDXRef-OCI-{digest}");
        files.push(SpdxFile {
            file_name: format!("oci/blobs/sha256/{digest}"),
            spdx_id: spdx_id.clone(),
            checksums: vec![SpdxChecksum {
                algorithm: "SHA256".into(),
                checksum_value: digest.into(),
            }],
            file_types: vec!["BINARY".into()],
            comment: format!(
                "OCI descriptor mediaType={}, sizeBytes={}, depth={}",
                blob.media_type, blob.size, blob.depth
            ),
        });
        relationships.push(SpdxRelationship {
            spdx_element_id: package_id.clone(),
            relationship_type: "CONTAINS".into(),
            related_spdx_element: spdx_id,
        });
    }
    let document = SpdxDocument {
        spdx_version: SPDX_VERSION.into(),
        data_license: "CC0-1.0".into(),
        spdx_id: "SPDXRef-DOCUMENT".into(),
        name: format!("A3S Cloud OCI build {}", build.id),
        document_namespace: format!(
            "https://a3s.dev/spdx/builds/{}/{}/{}",
            build.id, build.attempt, artifact_hex
        ),
        creation_info: SpdxCreationInfo {
            created,
            creators: vec!["Tool: A3S Cloud Control Plane".into()],
        },
        packages: vec![SpdxPackage {
            name: format!("a3s-cloud-build-{}", build.id),
            spdx_id: package_id,
            version_info: artifact.digest.clone(),
            supplier: "Organization: A3S Cloud".into(),
            download_location: artifact.uri.clone(),
            files_analyzed: false,
            checksums: vec![SpdxChecksum {
                algorithm: "SHA256".into(),
                checksum_value: artifact_hex.into(),
            }],
            primary_package_purpose: "CONTAINER".into(),
        }],
        files,
        relationships,
    };
    document
        .validate()
        .map_err(BuildEvidenceGenerationError::Integrity)?;
    Ok(document)
}

fn build_provenance(
    build: &BuildRun,
    revision: &ExternalSourceRevision,
    builder: &BuildEvidenceBuilder,
    artifact: &crate::modules::artifacts::domain::PublishedOciArtifact,
    sbom: &SpdxDocument,
    sbom_digest: &str,
    finished_on: DateTime<Utc>,
) -> Result<SlsaProvenanceStatement, BuildEvidenceGenerationError> {
    let output = build.output.as_ref().ok_or_else(|| {
        BuildEvidenceGenerationError::Invalid(
            "SLSA provenance requires validated OCI output".into(),
        )
    })?;
    let source_content_digest = build.source_content_digest.clone().ok_or_else(|| {
        BuildEvidenceGenerationError::Invalid(
            "SLSA provenance requires a source content digest".into(),
        )
    })?;
    let runtime_spec_digest = build.runtime_spec_digest.clone().ok_or_else(|| {
        BuildEvidenceGenerationError::Invalid(
            "SLSA provenance requires a Runtime specification digest".into(),
        )
    })?;
    let artifact_hex = digest_hex(&artifact.digest)?;
    let sbom_hex = digest_hex(sbom_digest)?;
    let builder_hex = digest_hex(&builder.digest)?;
    let source_hex = digest_hex(&source_content_digest)?;
    let recipe_hex = digest_hex(&revision.recipe_digest)?;
    let statement = SlsaProvenanceStatement {
        statement_type: IN_TOTO_STATEMENT_TYPE.into(),
        subject: vec![
            InTotoSubject {
                name: artifact.uri.clone(),
                digest: BTreeMap::from([("sha256".into(), artifact_hex.into())]),
            },
            InTotoSubject {
                name: sbom.document_namespace.clone(),
                digest: BTreeMap::from([("sha256".into(), sbom_hex.into())]),
            },
        ],
        predicate_type: SLSA_PROVENANCE_PREDICATE_TYPE.into(),
        predicate: SlsaProvenancePredicate {
            build_definition: SlsaBuildDefinition {
                build_type: SLSA_BUILD_TYPE.into(),
                external_parameters: SlsaExternalParameters {
                    repository: revision.repository.canonical_url().into(),
                    commit_sha: revision.commit_sha.as_str().into(),
                    source_content_digest: source_content_digest.clone(),
                    recipe: revision.recipe.clone(),
                    recipe_digest: revision.recipe_digest.clone(),
                    platforms: output.platforms.clone(),
                },
                internal_parameters: SlsaInternalParameters {
                    build_run_id: build.id,
                    operation_id: build.operation_id,
                    source_revision_id: build.source_revision_id,
                    attempt: build.attempt,
                    runtime_spec_digest,
                },
                resolved_dependencies: vec![
                    SlsaResourceDescriptor {
                        uri: revision.repository.canonical_url().into(),
                        digest: BTreeMap::from([(
                            "gitCommit".into(),
                            revision.commit_sha.as_str().into(),
                        )]),
                    },
                    SlsaResourceDescriptor {
                        uri: format!("https://a3s.dev/cloud/source-content/{source_hex}"),
                        digest: BTreeMap::from([("sha256".into(), source_hex.into())]),
                    },
                    SlsaResourceDescriptor {
                        uri: format!("https://a3s.dev/cloud/build-recipes/{recipe_hex}"),
                        digest: BTreeMap::from([("sha256".into(), recipe_hex.into())]),
                    },
                ],
            },
            run_details: SlsaRunDetails {
                builder: SlsaBuilder {
                    id: builder.uri.clone(),
                    builder_dependencies: vec![SlsaResourceDescriptor {
                        uri: builder.uri.clone(),
                        digest: BTreeMap::from([("sha256".into(), builder_hex.into())]),
                    }],
                },
                metadata: SlsaRunMetadata {
                    invocation_id: build.operation_id.to_string(),
                    started_on: build.started_at.unwrap_or(build.requested_at),
                    finished_on,
                },
            },
        },
    };
    statement
        .validate()
        .map_err(BuildEvidenceGenerationError::Integrity)?;
    Ok(statement)
}

fn digest_hex(value: &str) -> Result<&str, BuildEvidenceGenerationError> {
    value
        .strip_prefix("sha256:")
        .filter(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or_else(|| {
            BuildEvidenceGenerationError::Integrity(
                "build evidence input contains a non-canonical SHA-256 digest".into(),
            )
        })
}

fn map_output_error(
    error: crate::modules::artifacts::domain::BuildOutputValidationError,
) -> BuildEvidenceGenerationError {
    match error {
        crate::modules::artifacts::domain::BuildOutputValidationError::Invalid(message) => {
            BuildEvidenceGenerationError::Invalid(message)
        }
        crate::modules::artifacts::domain::BuildOutputValidationError::Integrity(message) => {
            BuildEvidenceGenerationError::Integrity(message)
        }
        crate::modules::artifacts::domain::BuildOutputValidationError::Unavailable(message) => {
            BuildEvidenceGenerationError::Unavailable(message)
        }
        crate::modules::artifacts::domain::BuildOutputValidationError::Storage(message) => {
            BuildEvidenceGenerationError::Storage(message)
        }
    }
}

fn map_signing_error(error: BuildEvidenceSigningError) -> BuildEvidenceGenerationError {
    match error {
        BuildEvidenceSigningError::Invalid(message) => {
            BuildEvidenceGenerationError::Invalid(message)
        }
        BuildEvidenceSigningError::Rejected(message) => {
            BuildEvidenceGenerationError::Integrity(message)
        }
        BuildEvidenceSigningError::Unavailable(message) => {
            BuildEvidenceGenerationError::Unavailable(message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::artifacts::domain::{OciDescriptor, PublishedOciArtifact};
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, NodeCommandId, NodeId, OrganizationId, ProjectId, SourceRevisionId,
    };
    use crate::modules::sources::domain::BuildPlatform;
    #[test]
    fn spdx_covers_every_reachable_oci_descriptor_with_digest_and_graph_metadata() {
        let now = Utc::now();
        let mut build = BuildRun::reserve(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            SourceRevisionId::new(),
            now,
        );
        build.source_content_digest = Some(format!("sha256:{}", "1".repeat(64)));
        build.runtime_spec_digest = Some(format!("sha256:{}", "2".repeat(64)));
        build.output = Some(crate::modules::artifacts::domain::ValidatedOciBuildOutput {
            artifact: crate::modules::artifacts::domain::BuildArtifact::new(
                format!("a3s-cloud-blob://sha256/{}", "3".repeat(64)),
                format!("sha256:{}", "3".repeat(64)),
                "application/vnd.a3s.cloud.runtime-archive.v1.tar",
                123,
            )
            .expect("output artifact"),
            descriptor: OciDescriptor::new(
                "application/vnd.oci.image.manifest.v1+json",
                format!("sha256:{}", "4".repeat(64)),
                20,
            )
            .expect("descriptor"),
            platforms: vec![BuildPlatform::parse("linux/amd64").expect("platform")],
            content_bytes: 60,
            blob_count: 3,
        });
        build.node_id = Some(NodeId::new());
        build.command_id = Some(NodeCommandId::new());
        build.started_at = Some(now);
        build.status = BuildRunStatus::Attesting;
        let artifact = PublishedOciArtifact {
            uri: format!("oci://registry.example/a3s/build@sha256:{}", "4".repeat(64)),
            digest: format!("sha256:{}", "4".repeat(64)),
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            size_bytes: 20,
        };
        build.published_artifact = Some(artifact.clone());
        let blobs = vec![
            blob('4', "application/vnd.oci.image.manifest.v1+json", 20, 0),
            blob('5', "application/vnd.oci.image.config.v1+json", 10, 1),
            blob('6', "application/vnd.oci.image.layer.v1.tar+gzip", 30, 1),
        ];

        let sbom = build_spdx(
            &build,
            &artifact,
            &blobs,
            crate::modules::shared_kernel::domain::canonical_timestamp(now),
        )
        .expect("SPDX");

        assert_eq!(sbom.spdx_version, "SPDX-2.3");
        assert_eq!(sbom.files.len(), blobs.len());
        assert_eq!(sbom.relationships.len(), blobs.len() + 1);
        for blob in blobs {
            let digest = blob.digest.strip_prefix("sha256:").expect("digest");
            let file = sbom
                .files
                .iter()
                .find(|file| file.spdx_id == format!("SPDXRef-OCI-{digest}"))
                .expect("descriptor file");
            assert_eq!(file.checksums[0].checksum_value, digest);
            assert!(file.comment.contains(&format!("sizeBytes={}", blob.size)));
            assert!(file.comment.contains(&format!("depth={}", blob.depth)));
        }
    }

    fn blob(fill: char, media_type: &str, size: u64, depth: usize) -> OciLayoutBlob {
        OciLayoutBlob {
            media_type: media_type.into(),
            digest: format!("sha256:{}", fill.to_string().repeat(64)),
            size,
            depth,
        }
    }
}
