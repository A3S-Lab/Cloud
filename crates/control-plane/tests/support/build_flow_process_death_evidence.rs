use super::runtime::ActionRecorder;
use a3s_cloud_control_plane::modules::artifacts::{
    canonical_json, dsse_pae, sha256_digest, BuildEvidence, BuildEvidenceBuilder,
    BuildEvidenceGenerationError, BuildEvidenceSubject, BuildEvidenceVerificationState, BuildRun,
    BuildSource, DsseEnvelope, DsseSignature, IBuildEvidenceGenerator, IBuildEvidenceSigner,
    InTotoSubject, LocalBuildEvidenceSigner, SlsaBuildDefinition, SlsaBuilder,
    SlsaExternalParameters, SlsaInternalParameters, SlsaProvenancePredicate,
    SlsaProvenanceStatement, SlsaResourceDescriptor, SlsaRunDetails, SlsaRunMetadata, SpdxChecksum,
    SpdxCreationInfo, SpdxDocument, SpdxFile, SpdxPackage, SpdxRelationship, BUILD_EVIDENCE_SCHEMA,
    DSSE_PAYLOAD_TYPE, IN_TOTO_STATEMENT_TYPE, SLSA_BUILD_TYPE, SLSA_PROVENANCE_PREDICATE_TYPE,
    SPDX_VERSION,
};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use std::sync::Arc;

pub(super) struct PersistentEvidenceGenerator {
    actions: ActionRecorder,
    signer: Arc<LocalBuildEvidenceSigner>,
}

impl PersistentEvidenceGenerator {
    pub(super) fn new(actions: ActionRecorder, signer: Arc<LocalBuildEvidenceSigner>) -> Self {
        Self { actions, signer }
    }
}

#[async_trait]
impl IBuildEvidenceGenerator for PersistentEvidenceGenerator {
    async fn generate(
        &self,
        build: &BuildRun,
        source: &BuildSource,
        attested_at: DateTime<Utc>,
    ) -> Result<BuildEvidence, BuildEvidenceGenerationError> {
        self.actions
            .record("evidence")
            .map_err(|error| BuildEvidenceGenerationError::Storage(error.to_string()))?;
        source
            .validate()
            .map_err(BuildEvidenceGenerationError::Integrity)?;
        if build.organization_id != source.organization_id || build.subject != source.subject {
            return Err(BuildEvidenceGenerationError::Invalid(
                "persistent evidence source changed BuildRun identity".into(),
            ));
        }
        let attested_at = canonical_timestamp(attested_at);
        let artifact = build.published_artifact.clone().ok_or_else(|| {
            BuildEvidenceGenerationError::Invalid(
                "persistent evidence requires a published artifact".into(),
            )
        })?;
        let output = build.output.as_ref().ok_or_else(|| {
            BuildEvidenceGenerationError::Invalid(
                "persistent evidence requires validated output".into(),
            )
        })?;
        let source_content_digest = build.source_content_digest.clone().ok_or_else(|| {
            BuildEvidenceGenerationError::Invalid(
                "persistent evidence requires a source content digest".into(),
            )
        })?;
        let build_request_digest = build.build_request_digest.clone().ok_or_else(|| {
            BuildEvidenceGenerationError::Invalid(
                "persistent evidence requires a Box build request digest".into(),
            )
        })?;
        let artifact_digest = digest_hex(&artifact.digest)?;
        let builder_digest = format!("sha256:{}", "f".repeat(64));
        let builder = BuildEvidenceBuilder {
            uri: "https://a3s.dev/cloud/build/box-native/v1".into(),
            digest: builder_digest.clone(),
        };
        let file_digest = "9".repeat(64);
        let sbom = SpdxDocument {
            spdx_version: SPDX_VERSION.into(),
            data_license: "CC0-1.0".into(),
            spdx_id: "SPDXRef-DOCUMENT".into(),
            name: format!("A3S Cloud persistent build {}", build.id),
            document_namespace: format!(
                "https://a3s.dev/spdx/persistent-builds/{}/{}",
                build.id, build.attempt
            ),
            creation_info: SpdxCreationInfo {
                created: attested_at,
                creators: vec!["Tool: A3S Cloud PostgreSQL process-death gate".into()],
            },
            packages: vec![SpdxPackage {
                name: format!("a3s-cloud-build-{}", build.id),
                spdx_id: "SPDXRef-Package-OCI".into(),
                version_info: artifact.digest.clone(),
                supplier: "Organization: A3S Cloud".into(),
                download_location: artifact.uri.clone(),
                files_analyzed: false,
                checksums: vec![SpdxChecksum {
                    algorithm: "SHA256".into(),
                    checksum_value: artifact_digest.into(),
                }],
                primary_package_purpose: "CONTAINER".into(),
            }],
            files: vec![SpdxFile {
                file_name: format!("oci/blobs/sha256/{file_digest}"),
                spdx_id: format!("SPDXRef-OCI-{file_digest}"),
                checksums: vec![SpdxChecksum {
                    algorithm: "SHA256".into(),
                    checksum_value: file_digest.clone(),
                }],
                file_types: vec!["BINARY".into()],
                comment: "OCI descriptor mediaType=application/octet-stream, sizeBytes=1, depth=1"
                    .into(),
            }],
            relationships: vec![
                SpdxRelationship {
                    spdx_element_id: "SPDXRef-DOCUMENT".into(),
                    relationship_type: "DESCRIBES".into(),
                    related_spdx_element: "SPDXRef-Package-OCI".into(),
                },
                SpdxRelationship {
                    spdx_element_id: "SPDXRef-Package-OCI".into(),
                    relationship_type: "CONTAINS".into(),
                    related_spdx_element: format!("SPDXRef-OCI-{file_digest}"),
                },
            ],
        };
        let sbom_digest =
            sha256_digest(&canonical_json(&sbom).map_err(BuildEvidenceGenerationError::Integrity)?);
        let provenance = SlsaProvenanceStatement {
            statement_type: IN_TOTO_STATEMENT_TYPE.into(),
            subject: vec![
                InTotoSubject {
                    name: artifact.uri.clone(),
                    digest: BTreeMap::from([("sha256".into(), artifact_digest.into())]),
                },
                InTotoSubject {
                    name: sbom.document_namespace.clone(),
                    digest: BTreeMap::from([("sha256".into(), digest_hex(&sbom_digest)?.into())]),
                },
            ],
            predicate_type: SLSA_PROVENANCE_PREDICATE_TYPE.into(),
            predicate: SlsaProvenancePredicate {
                build_definition: SlsaBuildDefinition {
                    build_type: SLSA_BUILD_TYPE.into(),
                    external_parameters: SlsaExternalParameters {
                        repository: source.repository.clone(),
                        commit_sha: source.commit_sha.as_str().into(),
                        manifest_digest: source
                            .manifest_digest
                            .as_ref()
                            .map(|digest| digest.as_str().to_owned()),
                        source_content_digest: source_content_digest.clone(),
                        recipe: source.recipe.clone(),
                        recipe_digest: source.recipe_digest.clone(),
                        platforms: output.platforms.clone(),
                    },
                    internal_parameters: SlsaInternalParameters {
                        build_run_id: build.id,
                        operation_id: build.operation_id,
                        subject: BuildEvidenceSubject::from_build_subject(build.subject),
                        attempt: build.attempt,
                        build_request_digest: build_request_digest.clone(),
                        published_output: build.published_output.clone(),
                    },
                    resolved_dependencies: vec![SlsaResourceDescriptor {
                        uri: source.repository.clone(),
                        digest: BTreeMap::from([(
                            "gitCommit".into(),
                            source.commit_sha.as_str().into(),
                        )]),
                    }],
                },
                run_details: SlsaRunDetails {
                    builder: SlsaBuilder {
                        id: builder.uri.clone(),
                        builder_dependencies: vec![SlsaResourceDescriptor {
                            uri: builder.uri.clone(),
                            digest: BTreeMap::from([(
                                "sha256".into(),
                                digest_hex(&builder_digest)?.into(),
                            )]),
                        }],
                    },
                    metadata: SlsaRunMetadata {
                        invocation_id: build.operation_id.to_string(),
                        started_on: build.started_at.unwrap_or(build.requested_at),
                        finished_on: attested_at,
                    },
                },
            },
        };
        let provenance_bytes =
            canonical_json(&provenance).map_err(BuildEvidenceGenerationError::Integrity)?;
        let provenance_digest = sha256_digest(&provenance_bytes);
        let pae = dsse_pae(DSSE_PAYLOAD_TYPE, &provenance_bytes)
            .map_err(BuildEvidenceGenerationError::Invalid)?;
        let signature = self
            .signer
            .sign(&pae)
            .await
            .map_err(|error| BuildEvidenceGenerationError::Unavailable(error.to_string()))?;
        BuildEvidence::restore(BuildEvidence {
            schema: BUILD_EVIDENCE_SCHEMA.into(),
            build_run_id: build.id,
            operation_id: build.operation_id,
            subject: BuildEvidenceSubject::from_build_subject(build.subject),
            attempt: build.attempt,
            repository: source.repository.clone(),
            commit_sha: source.commit_sha.as_str().into(),
            manifest_digest: source
                .manifest_digest
                .as_ref()
                .map(|digest| digest.as_str().to_owned()),
            source_content_digest,
            recipe: source.recipe.clone(),
            recipe_digest: source.recipe_digest.clone(),
            build_request_digest,
            builder,
            platforms: output.platforms.clone(),
            artifact,
            sbom,
            sbom_digest,
            provenance,
            provenance_digest,
            envelope: DsseEnvelope {
                payload_type: DSSE_PAYLOAD_TYPE.into(),
                payload: STANDARD.encode(&provenance_bytes),
                signatures: vec![DsseSignature {
                    key_id: signature.key.key_id.clone(),
                    signature: STANDARD.encode(&signature.signature),
                }],
            },
            signing_key: signature.key,
            verification_state: BuildEvidenceVerificationState::Verified,
            attested_at,
        })
        .map_err(BuildEvidenceGenerationError::Integrity)
    }
}

fn canonical_timestamp(value: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(value.timestamp_millis()).expect("valid canonical timestamp")
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
                "persistent evidence digest is not canonical SHA-256".into(),
            )
        })
}
