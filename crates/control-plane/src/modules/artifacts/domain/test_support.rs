use super::{
    canonical_json, dsse_pae, sha256_digest, BuildArtifact, BuildEvidence,
    BuildEvidenceAgentReleaseManifest, BuildEvidenceBuilder, BuildEvidenceSigningKey,
    BuildEvidenceSubject, BuildEvidenceVerificationState, BuildRun, BuildSubject, DsseEnvelope,
    DsseSignature, InTotoSubject, OciDescriptor, OciPublicationTarget, PublishedOciArtifact,
    SlsaBuildDefinition, SlsaBuilder, SlsaExternalParameters, SlsaInternalParameters,
    SlsaProvenancePredicate, SlsaProvenanceStatement, SlsaResourceDescriptor, SlsaRunDetails,
    SlsaRunMetadata, SpdxChecksum, SpdxCreationInfo, SpdxDocument, SpdxFile, SpdxPackage,
    SpdxRelationship, ValidatedOciBuildOutput, BUILD_EVIDENCE_SCHEMA, DSSE_PAYLOAD_TYPE,
    IN_TOTO_STATEMENT_TYPE, SLSA_BUILD_TYPE, SLSA_PROVENANCE_PREDICATE_TYPE, SPDX_VERSION,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AssetId, AssetReleaseId, EnvironmentId, NodeCommandId, NodeId,
    OrganizationId, ProjectId, SourceRevisionId,
};
use crate::modules::sources::published::{BuildPlatform, BuildRecipe};
use a3s_cloud_contracts::{
    agent_release_builder_uri, agent_release_manifest_archive, agent_release_source_uri,
    artifact_uri, AgentReleaseManifest, AgentReleaseProvenance, NodeBoxBuildCacheOutput,
    NodeBoxBuildCacheReceipt, NodeBoxBuildDescriptor, NodeBoxBuildOutput, NodeBoxBuildPlatform,
    NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
use a3s_runtime::contract::{ArtifactRef, RuntimeOutputArtifact};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
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
    let build_request_digest = build
        .build_request_digest
        .clone()
        .expect("Box build request digest");
    let artifact_digest = digest_hex(&artifact.digest);
    let (repository, commit_sha, manifest_digest) = match build.subject {
        BuildSubject::ExternalSourceRevision { .. } => (
            "https://github.com/A3S-Lab/Cloud".to_owned(),
            "a".repeat(40),
            None,
        ),
        BuildSubject::AssetRelease {
            asset_id,
            asset_release_id,
        } => (
            format!("https://a3s.dev/cloud/assets/{asset_id}/releases/{asset_release_id}"),
            "a".repeat(40),
            Some(format!("sha256:{}", "b".repeat(64))),
        ),
    };
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
        uri: "https://a3s.dev/cloud/build/box-native/v1".into(),
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
    let mut provenance_subjects = vec![
        InTotoSubject {
            name: artifact.uri.clone(),
            digest: BTreeMap::from([("sha256".into(), artifact_digest.into())]),
        },
        InTotoSubject {
            name: sbom.document_namespace.clone(),
            digest: BTreeMap::from([("sha256".into(), digest_hex(&sbom_digest).into())]),
        },
    ];
    if let Some(output) = &build.published_output {
        provenance_subjects.push(InTotoSubject {
            name: output.uri.clone(),
            digest: BTreeMap::from([("sha256".into(), digest_hex(&output.digest).into())]),
        });
    }
    let provenance = SlsaProvenanceStatement {
        statement_type: IN_TOTO_STATEMENT_TYPE.into(),
        subject: provenance_subjects,
        predicate_type: SLSA_PROVENANCE_PREDICATE_TYPE.into(),
        predicate: SlsaProvenancePredicate {
            build_definition: SlsaBuildDefinition {
                build_type: SLSA_BUILD_TYPE.into(),
                external_parameters: SlsaExternalParameters {
                    repository: repository.clone(),
                    commit_sha: commit_sha.clone(),
                    manifest_digest: manifest_digest.clone(),
                    source_content_digest: source_content_digest.clone(),
                    recipe: recipe.clone(),
                    recipe_digest: recipe_digest.clone(),
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
                    uri: repository.clone(),
                    digest: BTreeMap::from([("gitCommit".into(), commit_sha.clone())]),
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
        subject: BuildEvidenceSubject::from_build_subject(build.subject),
        attempt: build.attempt,
        repository,
        commit_sha,
        manifest_digest,
        source_content_digest,
        recipe,
        recipe_digest,
        build_request_digest,
        builder,
        platforms: output.platforms.clone(),
        artifact,
        sbom,
        sbom_digest,
        provenance,
        provenance_digest,
        agent_release_manifest: None,
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

pub(crate) fn agent_evidence_for(build: &BuildRun, attested_at: DateTime<Utc>) -> BuildEvidence {
    let mut evidence = evidence_for(build, attested_at);
    let template = AgentReleaseManifest::parse(agent_release_template_acl())
        .expect("Agent release manifest template");
    let provenance = [
        AgentReleaseProvenance::new(
            "source",
            agent_release_source_uri(&evidence.source_content_digest).expect("source URI"),
            evidence.source_content_digest.clone(),
        )
        .expect("source provenance"),
        AgentReleaseProvenance::new(
            "builder",
            agent_release_builder_uri(build.id.as_uuid()).expect("builder URI"),
            evidence.provenance_digest.clone(),
        )
        .expect("builder provenance"),
    ];
    let manifest = template
        .bind_publication(evidence.artifact.digest.clone(), provenance)
        .expect("bound Agent release manifest");
    let archive = agent_release_manifest_archive(manifest.canonical_acl().as_bytes())
        .expect("Agent release manifest archive");
    let archive_digest = sha256_digest(&archive);
    evidence.agent_release_manifest = Some(BuildEvidenceAgentReleaseManifest {
        identity: manifest.identity().into(),
        canonical_acl: manifest.canonical_acl().into(),
        archive: BuildArtifact::new(
            artifact_uri(&archive_digest).expect("Agent manifest Artifact URI"),
            archive_digest,
            NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
            archive.len() as u64,
        )
        .expect("Agent manifest Artifact"),
    });
    BuildEvidence::restore(evidence).expect("valid Agent build evidence fixture")
}

fn agent_release_template_acl() -> &'static str {
    concat!(
        "agent_release {\n",
        "  schema = \"a3s.code.agent-release.v1\"\n",
        "  protocol = \"a3s.code.agent.v1\"\n",
        "  artifact { digest = \"sha256:1111111111111111111111111111111111111111111111111111111111111111\" media_type = \"application/vnd.oci.image.manifest.v1+json\" }\n",
        "  entrypoint { command = \"/usr/bin/a3s\" args = [\"code\", \"harness\", \"--manifest\", \"/app/.a3s/asset.acl\"] }\n",
        "  health { transport = \"http\" port = 8080 readiness_path = \"/health/ready\" liveness_path = \"/health/live\" shutdown_grace_seconds = 30 }\n",
        "  storage { workspace = \"ephemeral\" cache = \"ephemeral\" persistent_data = \"none\" }\n",
        "  capability \"runtime.service\" { level = 1 }\n",
        "  capability \"secrets.external\" { level = 1 }\n",
        "  capability \"workspace.local\" { level = 1 }\n",
        "  provenance \"source\" { uri = \"urn:a3s:source:template\" digest = \"sha256:2222222222222222222222222222222222222222222222222222222222222222\" }\n",
        "  provenance \"builder\" { uri = \"urn:a3s:builder:template\" digest = \"sha256:4444444444444444444444444444444444444444444444444444444444444444\" }\n",
        "}\n",
    )
}

fn digest_hex(value: &str) -> &str {
    value.strip_prefix("sha256:").expect("SHA-256 digest")
}

fn hosted_build_ready_for_completion_with_manifest(
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    requested_at: DateTime<Utc>,
    include_agent_manifest: bool,
) -> BuildRun {
    let mut build =
        BuildRun::reserve_asset_release(organization_id, asset_id, asset_release_id, requested_at);
    build
        .begin_preparation(requested_at + Duration::milliseconds(1))
        .expect("begin hosted preparation");
    let input = test_artifact('a', 1_024);
    build
        .record_input(
            format!("sha256:{}", "b".repeat(64)),
            input.clone(),
            requested_at + Duration::milliseconds(2),
        )
        .expect("record hosted input");
    build
        .schedule(
            NodeId::new(),
            format!("sha256:{}", "c".repeat(64)),
            requested_at + Duration::milliseconds(3),
        )
        .expect("schedule hosted build");
    build
        .dispatch(
            NodeCommandId::new(),
            requested_at + Duration::milliseconds(4),
        )
        .expect("dispatch hosted build");
    let box_output = test_box_output(&input);
    let output_artifact = BuildArtifact::new(
        box_output.artifact.artifact.uri.clone(),
        box_output.artifact.artifact.digest.clone(),
        box_output.artifact.artifact.media_type.clone(),
        box_output.artifact.size_bytes,
    )
    .expect("hosted output Artifact");
    build
        .begin_validation(box_output, requested_at + Duration::milliseconds(5))
        .expect("begin hosted validation");
    let output = ValidatedOciBuildOutput {
        artifact: output_artifact,
        descriptor: OciDescriptor::new(
            "application/vnd.oci.image.manifest.v1+json",
            format!("sha256:{}", "e".repeat(64)),
            123,
        )
        .expect("hosted OCI descriptor"),
        platforms: vec![BuildPlatform::parse("linux/amd64").expect("hosted platform")],
        content_bytes: 456,
        blob_count: 3,
    };
    build
        .record_validated_output(output.clone(), requested_at + Duration::milliseconds(6))
        .expect("record hosted output");
    let target = OciPublicationTarget::new(
        "registry.example",
        format!("a3s-cloud/assets/{asset_id}/releases/{asset_release_id}"),
        output.descriptor,
    )
    .expect("hosted publication target");
    build
        .begin_publication(target.clone(), requested_at + Duration::milliseconds(7))
        .expect("begin hosted publication");
    build
        .record_published_artifact(
            PublishedOciArtifact::from_target(&target),
            requested_at + Duration::milliseconds(8),
        )
        .expect("record hosted publication");
    build
        .begin_attestation(requested_at + Duration::milliseconds(9))
        .expect("begin hosted attestation");
    let evidence = if include_agent_manifest {
        agent_evidence_for(&build, requested_at + Duration::milliseconds(10))
    } else {
        evidence_for(&build, requested_at + Duration::milliseconds(10))
    };
    build
        .record_evidence(evidence, requested_at + Duration::milliseconds(10))
        .expect("record hosted evidence");
    build
        .begin_cleanup(
            NodeCommandId::new(),
            requested_at + Duration::milliseconds(11),
        )
        .expect("begin hosted cleanup");
    build
}

pub(crate) fn hosted_build_ready_for_completion(
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    requested_at: DateTime<Utc>,
) -> BuildRun {
    hosted_build_ready_for_completion_with_manifest(
        organization_id,
        asset_id,
        asset_release_id,
        requested_at,
        false,
    )
}

pub(crate) fn succeeded_hosted_build(
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    requested_at: DateTime<Utc>,
) -> BuildRun {
    let mut build = hosted_build_ready_for_completion(
        organization_id,
        asset_id,
        asset_release_id,
        requested_at,
    );
    build
        .complete(requested_at + Duration::milliseconds(12))
        .expect("complete hosted build");
    build
}

pub(crate) fn succeeded_hosted_agent_build(
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    requested_at: DateTime<Utc>,
) -> BuildRun {
    let mut build = hosted_build_ready_for_completion_with_manifest(
        organization_id,
        asset_id,
        asset_release_id,
        requested_at,
        true,
    );
    build
        .complete(requested_at + Duration::milliseconds(12))
        .expect("complete hosted Agent build");
    build
}

pub(crate) fn succeeded_external_build_with_output(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    source_revision_id: SourceRevisionId,
    published_output: BuildArtifact,
    requested_at: DateTime<Utc>,
) -> BuildRun {
    let mut build = BuildRun::reserve(
        organization_id,
        project_id,
        environment_id,
        source_revision_id,
        requested_at,
    );
    build
        .begin_preparation(requested_at + Duration::milliseconds(1))
        .expect("begin external preparation");
    let input = test_artifact('1', 1_024);
    build
        .record_input(
            format!("sha256:{}", "2".repeat(64)),
            input.clone(),
            requested_at + Duration::milliseconds(2),
        )
        .expect("record external input");
    build
        .schedule(
            NodeId::new(),
            format!("sha256:{}", "3".repeat(64)),
            requested_at + Duration::milliseconds(3),
        )
        .expect("schedule external build");
    build
        .dispatch(
            NodeCommandId::new(),
            requested_at + Duration::milliseconds(4),
        )
        .expect("dispatch external build");
    let box_output = test_box_output(&input);
    let output_artifact = BuildArtifact::new(
        box_output.artifact.artifact.uri.clone(),
        box_output.artifact.artifact.digest.clone(),
        box_output.artifact.artifact.media_type.clone(),
        box_output.artifact.size_bytes,
    )
    .expect("external output Artifact");
    build
        .begin_validation(box_output, requested_at + Duration::milliseconds(5))
        .expect("begin external validation");
    let output = ValidatedOciBuildOutput {
        artifact: output_artifact,
        descriptor: OciDescriptor::new(
            "application/vnd.oci.image.manifest.v1+json",
            format!("sha256:{}", "e".repeat(64)),
            123,
        )
        .expect("external OCI descriptor"),
        platforms: vec![BuildPlatform::parse("linux/amd64").expect("external platform")],
        content_bytes: 456,
        blob_count: 3,
    };
    build
        .record_validated_output(output.clone(), requested_at + Duration::milliseconds(6))
        .expect("record external output");
    let target = OciPublicationTarget::new(
        "registry.example",
        format!("a3s-cloud/builds/{}", build.id),
        output.descriptor,
    )
    .expect("external publication target");
    build
        .begin_publication(target.clone(), requested_at + Duration::milliseconds(7))
        .expect("begin external publication");
    build
        .record_published_artifact(
            PublishedOciArtifact::from_target(&target),
            requested_at + Duration::milliseconds(8),
        )
        .expect("record external OCI publication");
    build
        .record_published_output(published_output, requested_at + Duration::milliseconds(9))
        .expect("record typed build output");
    build
        .begin_attestation(requested_at + Duration::milliseconds(10))
        .expect("begin external attestation");
    let evidence = evidence_for(&build, requested_at + Duration::milliseconds(11));
    build
        .record_evidence(evidence, requested_at + Duration::milliseconds(11))
        .expect("record external evidence");
    build
        .begin_cleanup(
            NodeCommandId::new(),
            requested_at + Duration::milliseconds(12),
        )
        .expect("begin external cleanup");
    build
        .complete(requested_at + Duration::milliseconds(13))
        .expect("complete external build");
    build
}

pub(crate) fn typed_build_output(digest: &str, media_type: &str, size_bytes: u64) -> BuildArtifact {
    BuildArtifact::new(
        artifact_uri(digest).expect("typed output Artifact URI"),
        digest,
        media_type,
        size_bytes,
    )
    .expect("typed build output")
}

fn test_artifact(fill: char, size_bytes: u64) -> BuildArtifact {
    let digest = format!("sha256:{}", fill.to_string().repeat(64));
    BuildArtifact::new(
        artifact_uri(&digest).expect("test Artifact URI"),
        digest,
        NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
        size_bytes,
    )
    .expect("test Artifact")
}

fn test_box_output(source: &BuildArtifact) -> NodeBoxBuildOutput {
    let output = test_artifact('d', 1_024);
    let cache = test_artifact('8', 512);
    let platform = NodeBoxBuildPlatform {
        os: "linux".into(),
        architecture: "amd64".into(),
        variant: None,
    };
    let output = NodeBoxBuildOutput {
        artifact: RuntimeOutputArtifact {
            name: "oci-layout".into(),
            artifact: ArtifactRef {
                uri: output.uri,
                digest: output.digest,
                media_type: output.media_type,
            },
            size_bytes: output.size_bytes,
        },
        descriptor: NodeBoxBuildDescriptor {
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            digest: format!("sha256:{}", "e".repeat(64)),
            size: 123,
        },
        platforms: vec![platform.clone()],
        manifest_count: 1,
        content_bytes: 456,
        blob_count: 3,
        blob_inventory_digest: format!("sha256:{}", "7".repeat(64)),
        caches: vec![NodeBoxBuildCacheOutput {
            operation_id: "cloud-build-hosted-linux-amd64".into(),
            artifact: RuntimeOutputArtifact {
                name: "build-cache-hosted".into(),
                artifact: ArtifactRef {
                    uri: cache.uri,
                    digest: cache.digest,
                    media_type: cache.media_type,
                },
                size_bytes: cache.size_bytes,
            },
            receipt: NodeBoxBuildCacheReceipt {
                schema: NodeBoxBuildCacheReceipt::SCHEMA.into(),
                key: format!("sha256:{}", "9".repeat(64)),
                source_digest: source.digest.clone(),
                plan_digest: format!("sha256:{}", "6".repeat(64)),
                descriptor: NodeBoxBuildDescriptor {
                    media_type: "application/vnd.oci.image.manifest.v1+json".into(),
                    digest: format!("sha256:{}", "5".repeat(64)),
                    size: 64,
                },
                platform,
                content_bytes: 128,
                entry_count: 1,
                blob_count: 2,
                blob_inventory_digest: format!("sha256:{}", "4".repeat(64)),
            },
        }],
    };
    output.validate().expect("test Box output");
    output
}
