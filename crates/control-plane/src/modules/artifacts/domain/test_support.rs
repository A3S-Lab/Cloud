use super::{
    canonical_json, dsse_pae, sha256_digest, BuildEvidence, BuildEvidenceBuilder,
    BuildEvidenceSigningKey, BuildEvidenceVerificationState, BuildRun, DsseEnvelope, DsseSignature,
    InTotoSubject, SlsaBuildDefinition, SlsaBuilder, SlsaExternalParameters,
    SlsaInternalParameters, SlsaProvenancePredicate, SlsaProvenanceStatement,
    SlsaResourceDescriptor, SlsaRunDetails, SlsaRunMetadata, SpdxChecksum, SpdxCreationInfo,
    SpdxDocument, SpdxFile, SpdxPackage, SpdxRelationship, BUILD_EVIDENCE_SCHEMA,
    DSSE_PAYLOAD_TYPE, IN_TOTO_STATEMENT_TYPE, SLSA_BUILD_TYPE, SLSA_PROVENANCE_PREDICATE_TYPE,
    SPDX_VERSION,
};
use crate::modules::shared_kernel::domain::canonical_timestamp;
use crate::modules::sources::domain::BuildRecipe;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{DateTime, Utc};
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::collections::BTreeMap;

pub(crate) fn evidence_for(build: &BuildRun, attested_at: DateTime<Utc>) -> BuildEvidence {
    let attested_at = canonical_timestamp(attested_at);
    let artifact = build
        .published_artifact
        .clone()
        .expect("published artifact");
    let output = build.output.as_ref().expect("validated output");
    let source_content_digest = build
        .source_content_digest
        .clone()
        .expect("source content digest");
    let runtime_spec_digest = build
        .runtime_spec_digest
        .clone()
        .expect("Runtime specification digest");
    let artifact_digest = digest_hex(&artifact.digest);
    let recipe = BuildRecipe::dockerfile(
        BuildRecipe::SCHEMA,
        BuildRecipe::DOCKERFILE_KIND,
        ".",
        "Dockerfile",
        None,
        output
            .platforms
            .iter()
            .map(|platform| platform.as_str().to_owned())
            .collect(),
    )
    .expect("build recipe");
    let recipe_digest = recipe.digest().expect("build recipe digest");
    let builder_digest = format!("sha256:{}", "f".repeat(64));
    let builder = BuildEvidenceBuilder {
        uri: format!("oci://docker.io/moby/buildkit@{builder_digest}"),
        digest: builder_digest.clone(),
    };
    let file_digest = "9".repeat(64);
    let sbom = SpdxDocument {
        spdx_version: SPDX_VERSION.into(),
        data_license: "CC0-1.0".into(),
        spdx_id: "SPDXRef-DOCUMENT".into(),
        name: format!("A3S Cloud test build {}", build.id),
        document_namespace: format!(
            "https://a3s.dev/spdx/test-builds/{}/{}",
            build.id, build.attempt
        ),
        creation_info: SpdxCreationInfo {
            created: attested_at,
            creators: vec!["Tool: A3S Cloud Control Plane tests".into()],
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
    let sbom_digest = sha256_digest(&canonical_json(&sbom).expect("canonical SPDX"));
    let provenance = SlsaProvenanceStatement {
        statement_type: IN_TOTO_STATEMENT_TYPE.into(),
        subject: vec![
            InTotoSubject {
                name: artifact.uri.clone(),
                digest: BTreeMap::from([("sha256".into(), artifact_digest.into())]),
            },
            InTotoSubject {
                name: sbom.document_namespace.clone(),
                digest: BTreeMap::from([("sha256".into(), digest_hex(&sbom_digest).into())]),
            },
        ],
        predicate_type: SLSA_PROVENANCE_PREDICATE_TYPE.into(),
        predicate: SlsaProvenancePredicate {
            build_definition: SlsaBuildDefinition {
                build_type: SLSA_BUILD_TYPE.into(),
                external_parameters: SlsaExternalParameters {
                    repository: "https://github.com/A3S-Lab/Cloud".into(),
                    commit_sha: "a".repeat(40),
                    source_content_digest: source_content_digest.clone(),
                    recipe: recipe.clone(),
                    recipe_digest: recipe_digest.clone(),
                    platforms: output.platforms.clone(),
                },
                internal_parameters: SlsaInternalParameters {
                    build_run_id: build.id,
                    operation_id: build.operation_id,
                    source_revision_id: build.source_revision_id,
                    attempt: build.attempt,
                    runtime_spec_digest: runtime_spec_digest.clone(),
                },
                resolved_dependencies: vec![SlsaResourceDescriptor {
                    uri: "https://github.com/A3S-Lab/Cloud".into(),
                    digest: BTreeMap::from([("gitCommit".into(), "a".repeat(40))]),
                }],
            },
            run_details: SlsaRunDetails {
                builder: SlsaBuilder {
                    id: builder.uri.clone(),
                    builder_dependencies: vec![SlsaResourceDescriptor {
                        uri: builder.uri.clone(),
                        digest: BTreeMap::from([(
                            "sha256".into(),
                            digest_hex(&builder_digest).into(),
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
    let provenance_bytes = canonical_json(&provenance).expect("canonical provenance");
    let provenance_digest = sha256_digest(&provenance_bytes);
    let signing_key_pair =
        Ed25519KeyPair::from_seed_unchecked(&[7_u8; 32]).expect("test Ed25519 signing seed");
    let public_key = signing_key_pair.public_key().as_ref().to_vec();
    let signing_key = BuildEvidenceSigningKey {
        algorithm: "ed25519".into(),
        key_id: sha256_digest(&public_key),
        public_key: STANDARD.encode(&public_key),
        key_version: Some(1),
    };
    let pae = dsse_pae(DSSE_PAYLOAD_TYPE, &provenance_bytes).expect("DSSE PAE");
    let signature = signing_key_pair.sign(&pae);
    BuildEvidence::restore(BuildEvidence {
        schema: BUILD_EVIDENCE_SCHEMA.into(),
        build_run_id: build.id,
        operation_id: build.operation_id,
        source_revision_id: build.source_revision_id,
        attempt: build.attempt,
        repository: "https://github.com/A3S-Lab/Cloud".into(),
        commit_sha: "a".repeat(40),
        source_content_digest,
        recipe,
        recipe_digest,
        runtime_spec_digest,
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
                key_id: signing_key.key_id.clone(),
                signature: STANDARD.encode(signature.as_ref()),
            }],
        },
        signing_key,
        verification_state: BuildEvidenceVerificationState::Verified,
        attested_at,
    })
    .expect("valid build evidence fixture")
}

fn digest_hex(value: &str) -> &str {
    value.strip_prefix("sha256:").expect("SHA-256 digest")
}
