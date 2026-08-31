use super::test_support::{evidence_for, succeeded_hosted_agent_build, succeeded_hosted_build};
use super::{
    BuildArtifact, BuildEvidence, BuildEvidenceAgentReleaseManifest, BuildRun, BuildRunStatus,
    OciDescriptor, OciPublicationTarget, PublishedOciArtifact, ValidatedOciBuildOutput,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, BuildRunId, EnvironmentId, NodeCommandId, NodeId, OrganizationId,
    ProjectId, SourceRevisionId,
};
use crate::modules::sources::published::BuildPlatform;
use a3s_cloud_contracts::{
    agent_release_builder_uri, agent_release_manifest_archive, agent_release_source_uri,
    artifact_uri, AgentReleaseManifest, AgentReleaseProvenance, NodeBoxBuildCacheOutput,
    NodeBoxBuildCacheReceipt, NodeBoxBuildDescriptor, NodeBoxBuildOutput, NodeBoxBuildPlatform,
    DURABLE_CELL_BUNDLE_MEDIA_TYPE, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
use a3s_runtime::contract::{ArtifactRef, RuntimeOutputArtifact};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{Duration, Utc};
use ring::signature::{Ed25519KeyPair, KeyPair};

#[test]
fn oci_descriptor_accepts_only_content_addressed_image_roots() {
    let descriptor = OciDescriptor::new(
        "application/vnd.oci.image.manifest.v1+json",
        format!("sha256:{}", "b".repeat(64)),
        123,
    )
    .expect("OCI descriptor");
    assert_eq!(descriptor.size(), 123);

    assert!(OciDescriptor::new(
        "application/octet-stream",
        format!("sha256:{}", "b".repeat(64)),
        123,
    )
    .is_err());
    assert!(OciDescriptor::new(
        "application/vnd.oci.image.manifest.v1+json",
        format!("sha256:{}", "B".repeat(64)),
        123,
    )
    .is_err());
    assert!(OciDescriptor::new(
        "application/vnd.oci.image.manifest.v1+json",
        format!("sha256:{}", "b".repeat(64)),
        0,
    )
    .is_err());
}

#[test]
fn build_run_binds_one_source_to_one_box_operation_and_validated_output() {
    let requested_at = Utc::now();
    let source_revision_id = SourceRevisionId::new();
    let mut build = BuildRun::reserve(
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        source_revision_id,
        requested_at,
    );
    assert_eq!(build.id, BuildRun::id_for(source_revision_id));
    assert_eq!(build.id.as_uuid(), build.operation_id.as_uuid());

    build
        .begin_preparation(requested_at + Duration::milliseconds(1))
        .expect("begin preparation");
    let prepared_checkout = build.clone();
    build
        .begin_preparation(requested_at + Duration::milliseconds(2))
        .expect("replay preparation");
    assert_eq!(build, prepared_checkout);
    let input = artifact('a');
    build
        .record_input(
            format!("sha256:{}", "b".repeat(64)),
            input.clone(),
            requested_at + Duration::milliseconds(3),
        )
        .expect("record input");
    let prepared = build.clone();
    build
        .record_input(
            format!("sha256:{}", "b".repeat(64)),
            input,
            requested_at + Duration::milliseconds(4),
        )
        .expect("replay input");
    assert_eq!(build, prepared);
    let node_id = NodeId::new();
    build
        .schedule(
            node_id,
            format!("sha256:{}", "c".repeat(64)),
            requested_at + Duration::milliseconds(5),
        )
        .expect("schedule");
    let scheduled = build.clone();
    build
        .schedule(
            node_id,
            format!("sha256:{}", "c".repeat(64)),
            requested_at + Duration::milliseconds(6),
        )
        .expect("replay schedule");
    assert_eq!(build, scheduled);
    let command_id = NodeCommandId::new();
    build
        .dispatch(command_id, requested_at + Duration::milliseconds(7))
        .expect("dispatch");
    let running = build.clone();
    build
        .dispatch(command_id, requested_at + Duration::milliseconds(8))
        .expect("replay dispatch");
    assert_eq!(build, running);
    let mut impossible_running_cleanup = build.clone();
    impossible_running_cleanup.cleanup_command_id = Some(NodeCommandId::new());
    assert!(BuildRun::restore(impossible_running_cleanup).is_err());
    let box_output = box_output('d', 'e');
    let output_artifact = artifact_from_box(&box_output);
    build
        .begin_validation(box_output.clone(), requested_at + Duration::milliseconds(9))
        .expect("begin validation");
    assert!(build
        .begin_cleanup(
            NodeCommandId::new(),
            requested_at + Duration::milliseconds(10),
        )
        .is_err());
    let validating = build.clone();
    build
        .begin_validation(box_output, requested_at + Duration::milliseconds(11))
        .expect("replay validation");
    assert_eq!(build, validating);
    let output = ValidatedOciBuildOutput {
        artifact: output_artifact,
        descriptor: OciDescriptor::new(
            "application/vnd.oci.image.manifest.v1+json",
            format!("sha256:{}", "e".repeat(64)),
            123,
        )
        .expect("descriptor"),
        platforms: vec![BuildPlatform::parse("linux/amd64").expect("platform")],
        content_bytes: 456,
        blob_count: 3,
    };
    build
        .record_validated_output(output.clone(), requested_at + Duration::milliseconds(12))
        .expect("record validated output");
    let validated = build.clone();
    build
        .record_validated_output(output.clone(), requested_at + Duration::milliseconds(13))
        .expect("replay validated output");
    assert_eq!(build, validated);
    let target = OciPublicationTarget::new(
        "registry.example",
        format!("a3s-cloud/builds/{}", build.id),
        output.descriptor.clone(),
    )
    .expect("publication target");
    build
        .begin_publication(target.clone(), requested_at + Duration::milliseconds(14))
        .expect("begin publication");
    let publishing = build.clone();
    build
        .begin_publication(target.clone(), requested_at + Duration::milliseconds(15))
        .expect("replay publication target");
    assert_eq!(build, publishing);
    let published = PublishedOciArtifact::from_target(&target);
    build
        .record_published_artifact(published.clone(), requested_at + Duration::milliseconds(16))
        .expect("record publication");
    let projected = build.clone();
    build
        .record_published_artifact(published.clone(), requested_at + Duration::milliseconds(17))
        .expect("replay publication");
    assert_eq!(build, projected);
    build
        .begin_attestation(requested_at + Duration::milliseconds(18))
        .expect("begin attestation");
    let attesting = build.clone();
    build
        .begin_attestation(requested_at + Duration::milliseconds(19))
        .expect("replay attestation");
    assert_eq!(build, attesting);
    let evidence = evidence_for(&build, requested_at + Duration::milliseconds(20));
    build
        .record_evidence(evidence.clone(), requested_at + Duration::milliseconds(20))
        .expect("record evidence");
    let evidenced = build.clone();
    build
        .record_evidence(evidence, requested_at + Duration::milliseconds(21))
        .expect("replay evidence");
    assert_eq!(build, evidenced);
    let cleanup_command_id = NodeCommandId::new();
    build
        .begin_cleanup(
            cleanup_command_id,
            requested_at + Duration::milliseconds(22),
        )
        .expect("begin cleanup");
    let cleanup = build.clone();
    build
        .begin_cleanup(
            cleanup_command_id,
            requested_at + Duration::milliseconds(23),
        )
        .expect("replay cleanup");
    assert_eq!(build, cleanup);
    build
        .complete(requested_at + Duration::milliseconds(24))
        .expect("complete");
    let completed = build.clone();
    build
        .complete(requested_at + Duration::milliseconds(25))
        .expect("replay completion");
    assert_eq!(build, completed);

    assert_eq!(build.status, BuildRunStatus::Succeeded);
    assert_eq!(build.node_id, Some(node_id));
    assert_eq!(build.command_id, Some(command_id));
    assert_eq!(build.output, Some(output));
    assert_eq!(build.publication_target, Some(target));
    assert_eq!(build.published_artifact, Some(published));
    BuildRun::restore(build).expect("restore valid build run");
}

#[test]
fn build_run_records_a_completed_publication_across_cancellation() {
    let now = Utc::now();
    let mut build = publishing_build(now);
    let target = build
        .publication_target
        .clone()
        .expect("publication target");
    build
        .request_cancellation(now + Duration::milliseconds(8))
        .expect("request cancellation");
    let published = PublishedOciArtifact::from_target(&target);
    build
        .record_published_artifact(published.clone(), now + Duration::milliseconds(9))
        .expect("adopt publication after cancellation");
    assert_eq!(build.status, BuildRunStatus::Cancelling);
    assert_eq!(build.published_artifact, Some(published));
    assert!(build
        .begin_cleanup(NodeCommandId::new(), now + Duration::milliseconds(10))
        .is_err());
    BuildRun::restore(build).expect("restore cancelling published build");
}

#[test]
fn build_run_publishes_one_distinct_typed_output_bound_to_signed_provenance() {
    let now = Utc::now();
    let mut build = publishing_build(now);
    let target = build
        .publication_target
        .clone()
        .expect("publication target");
    let bundle_digest = format!("sha256:{}", "f".repeat(64));
    let bundle = BuildArtifact::new(
        artifact_uri(&bundle_digest).expect("bundle Artifact URI"),
        bundle_digest,
        DURABLE_CELL_BUNDLE_MEDIA_TYPE,
        4_096,
    )
    .expect("typed bundle");

    assert!(build
        .record_published_output(bundle.clone(), now + Duration::milliseconds(8))
        .is_err());
    build
        .record_published_artifact(
            PublishedOciArtifact::from_target(&target),
            now + Duration::milliseconds(8),
        )
        .expect("published OCI artifact");
    build
        .record_published_output(bundle.clone(), now + Duration::milliseconds(9))
        .expect("published typed output");
    let published = build.clone();
    build
        .record_published_output(bundle.clone(), now + Duration::milliseconds(10))
        .expect("published output replay");
    assert_eq!(build, published);

    let mut replacement = bundle.clone();
    replacement.size_bytes += 1;
    assert!(build
        .record_published_output(replacement, now + Duration::milliseconds(10))
        .is_err());
    build
        .begin_attestation(now + Duration::milliseconds(10))
        .expect("begin attestation");
    let evidence = evidence_for(&build, now + Duration::milliseconds(11));
    assert_eq!(evidence.provenance.subject.len(), 3);
    assert!(evidence.provenance.subject.iter().any(|subject| {
        subject.name == bundle.uri
            && subject.digest.get("sha256")
                == Some(&bundle.digest.trim_start_matches("sha256:").to_owned())
    }));
    assert_eq!(
        evidence
            .provenance
            .predicate
            .build_definition
            .internal_parameters
            .published_output
            .as_ref(),
        Some(&bundle)
    );
    let mut changed_descriptor = build.clone();
    changed_descriptor
        .published_output
        .as_mut()
        .expect("published output")
        .size_bytes += 1;
    assert!(changed_descriptor
        .record_evidence(evidence.clone(), now + Duration::milliseconds(11))
        .is_err());
    build
        .record_evidence(evidence, now + Duration::milliseconds(11))
        .expect("record bundle-bound evidence");
    build
        .begin_cleanup(NodeCommandId::new(), now + Duration::milliseconds(12))
        .expect("begin cleanup");
    build
        .complete(now + Duration::milliseconds(13))
        .expect("complete build");
    assert_eq!(build.status, BuildRunStatus::Succeeded);
    assert_eq!(build.published_output, Some(bundle));
    BuildRun::restore(build).expect("restore bundle-producing build");
}

#[test]
fn build_run_rejects_an_oci_manifest_alias_as_a_typed_output() {
    let now = Utc::now();
    let mut build = publishing_build(now);
    let target = build
        .publication_target
        .clone()
        .expect("publication target");
    let published = PublishedOciArtifact::from_target(&target);
    build
        .record_published_artifact(published.clone(), now + Duration::milliseconds(8))
        .expect("published OCI artifact");
    let aliased = BuildArtifact::new(
        artifact_uri(&published.digest).expect("alias Artifact URI"),
        published.digest,
        DURABLE_CELL_BUNDLE_MEDIA_TYPE,
        4_096,
    )
    .expect("syntactically valid alias");
    assert_eq!(
        build
            .record_published_output(aliased, now + Duration::milliseconds(9))
            .expect_err("OCI alias must fail closed"),
        "published build output cannot reuse the OCI manifest digest"
    );
}

#[test]
fn build_run_freezes_verified_evidence_before_cleanup() {
    let now = Utc::now();
    let mut build = attesting_build(now);
    assert!(build
        .begin_cleanup(NodeCommandId::new(), now + Duration::milliseconds(10))
        .is_err());
    assert!(build.complete(now + Duration::milliseconds(10)).is_err());

    let first = evidence_for(&build, now + Duration::milliseconds(10));
    build
        .record_evidence(first.clone(), now + Duration::milliseconds(10))
        .expect("record evidence");
    let recorded = build.clone();
    build
        .record_evidence(first, now + Duration::milliseconds(11))
        .expect("replay evidence");
    assert_eq!(build, recorded);

    let replacement = evidence_for(&build, now + Duration::milliseconds(12));
    assert!(build
        .record_evidence(replacement, now + Duration::milliseconds(12))
        .is_err());
    build
        .begin_cleanup(NodeCommandId::new(), now + Duration::milliseconds(12))
        .expect("begin cleanup after evidence");
}

#[test]
fn build_evidence_restore_rejects_a_tampered_ed25519_signature() {
    let now = Utc::now();
    let build = attesting_build(now);
    let mut evidence = evidence_for(&build, now + Duration::milliseconds(10));

    evidence.envelope.signatures[0].signature = STANDARD.encode([0_u8; 64]);

    assert!(evidence.validate().is_err());
}

#[test]
fn build_evidence_restore_rejects_an_internally_consistent_public_key_substitution() {
    let now = Utc::now();
    let build = attesting_build(now);
    let mut evidence = evidence_for(&build, now + Duration::milliseconds(10));
    let replacement =
        Ed25519KeyPair::from_seed_unchecked(&[8_u8; 32]).expect("replacement Ed25519 test key");
    let public_key = replacement.public_key().as_ref().to_vec();
    let key_id = super::sha256_digest(&public_key);

    evidence.signing_key.public_key = STANDARD.encode(&public_key);
    evidence.signing_key.key_id.clone_from(&key_id);
    evidence.envelope.signatures[0].key_id = key_id;

    assert!(evidence.validate().is_err());
}

#[test]
fn cancelled_published_build_can_fail_closed_when_attestation_is_terminal() {
    let now = Utc::now();
    let mut build = publishing_build(now);
    let target = build
        .publication_target
        .clone()
        .expect("publication target");
    build
        .request_cancellation(now + Duration::milliseconds(8))
        .expect("request cancellation");
    build
        .record_published_artifact(
            PublishedOciArtifact::from_target(&target),
            now + Duration::milliseconds(9),
        )
        .expect("adopt publication");
    build
        .begin_attestation(now + Duration::milliseconds(10))
        .expect("begin attestation");
    build
        .record_failure(
            "build evidence signature failed integrity validation".into(),
            now + Duration::milliseconds(11),
        )
        .expect("fail closed");
    build
        .begin_cleanup(NodeCommandId::new(), now + Duration::milliseconds(12))
        .expect("cleanup failed build");
    build
        .complete(now + Duration::milliseconds(13))
        .expect("complete cancellation");

    assert_eq!(build.status, BuildRunStatus::Cancelled);
    assert!(build.evidence.is_none());
    assert!(build.failure.is_some());
    BuildRun::restore(build).expect("restore failed-closed cancellation");
}

#[test]
fn build_run_terminal_outcomes_are_truthful_and_idempotent() {
    let now = Utc::now();
    let mut cancelled = BuildRun::reserve(
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        SourceRevisionId::new(),
        now,
    );
    cancelled
        .request_cancellation(now + Duration::milliseconds(1))
        .expect("request cancellation");
    cancelled
        .complete(now + Duration::milliseconds(2))
        .expect("complete cancellation");
    let cancelled_snapshot = cancelled.clone();
    cancelled
        .complete(now + Duration::milliseconds(3))
        .expect("replay completion");
    assert_eq!(cancelled, cancelled_snapshot);
    assert_eq!(cancelled.status, BuildRunStatus::Cancelled);

    let mut failed = BuildRun::reserve(
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        SourceRevisionId::new(),
        now,
    );
    failed
        .record_failure(
            "checkout failed integrity validation".into(),
            now + Duration::milliseconds(1),
        )
        .expect("record failure");
    let failed_pending_cleanup = failed.clone();
    failed
        .record_failure(
            "checkout failed integrity validation".into(),
            now + Duration::milliseconds(2),
        )
        .expect("replay failure");
    assert_eq!(failed, failed_pending_cleanup);
    failed
        .complete(now + Duration::milliseconds(3))
        .expect("complete failure");
    assert_eq!(failed.status, BuildRunStatus::Failed);
    assert!(BuildRun::restore(failed).is_ok());
}

#[test]
fn build_run_retry_creates_a_fresh_attempt_and_preserves_lineage() {
    let now = Utc::now();
    let mut failed = BuildRun::reserve(
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        SourceRevisionId::new(),
        now,
    );
    failed
        .record_failure("builder timed out".into(), now + Duration::milliseconds(1))
        .expect("record failure");
    failed
        .complete(now + Duration::milliseconds(2))
        .expect("complete failure");

    let retry = BuildRun::retry(&failed, now + Duration::milliseconds(3)).expect("retry build");

    assert_eq!(failed.attempt, 1);
    assert_eq!(failed.retry_of_build_run_id, None);
    assert_eq!(retry.attempt, 2);
    assert_eq!(retry.retry_of_build_run_id, Some(failed.id));
    assert_eq!(retry.source_revision_id(), failed.source_revision_id());
    assert_eq!(retry.status, BuildRunStatus::Queued);
    assert!(retry.evidence_required);
    assert!(retry.evidence.is_none());
    assert_ne!(retry.id, failed.id);
    assert_eq!(retry.id.as_uuid(), retry.operation_id.as_uuid());
    assert_eq!(
        retry.id,
        BuildRun::id_for_attempt(
            failed
                .source_revision_id()
                .expect("external source revision"),
            2,
        )
        .expect("attempt identity")
    );
    assert!(BuildRun::restore(retry).is_ok());

    assert!(BuildRun::retry(&failed, now + Duration::milliseconds(1)).is_err());
    let queued = BuildRun::reserve(
        failed.organization_id,
        failed.project_id().expect("external project"),
        failed.environment_id().expect("external environment"),
        SourceRevisionId::new(),
        now,
    );
    assert!(BuildRun::retry(&queued, now + Duration::milliseconds(3)).is_err());
}

#[test]
fn hosted_release_build_retry_preserves_the_exact_subject_lineage() {
    let now = Utc::now();
    let organization_id = OrganizationId::new();
    let asset_id = AssetId::new();
    let asset_release_id = AssetReleaseId::new();
    let mut failed =
        BuildRun::reserve_asset_release(organization_id, asset_id, asset_release_id, now);
    failed
        .record_failure(
            "hosted checkout failed".into(),
            now + Duration::milliseconds(1),
        )
        .expect("record hosted failure");
    failed
        .complete(now + Duration::milliseconds(2))
        .expect("complete hosted failure");

    let retry = BuildRun::retry(&failed, now + Duration::milliseconds(3))
        .expect("retry hosted release build");
    assert_eq!(retry.organization_id, organization_id);
    assert_eq!(retry.asset_id(), Some(asset_id));
    assert_eq!(retry.asset_release_id(), Some(asset_release_id));
    assert_eq!(retry.project_id(), None);
    assert_eq!(retry.environment_id(), None);
    assert_eq!(retry.source_revision_id(), None);
    assert_eq!(retry.retry_of_build_run_id, Some(failed.id));
    assert_eq!(
        retry.id,
        BuildRun::id_for_subject_attempt(retry.subject, 2).expect("hosted attempt identity")
    );
    assert!(BuildRun::restore(retry).is_ok());
}

#[test]
fn hosted_build_evidence_round_trips_its_closed_flattened_subject() {
    let build = succeeded_hosted_build(
        OrganizationId::new(),
        AssetId::new(),
        AssetReleaseId::new(),
        Utc::now(),
    );
    let evidence = build.evidence.as_deref().expect("hosted build evidence");
    let mut encoded = serde_json::to_value(evidence).expect("serialize hosted build evidence");
    assert_eq!(
        encoded["assetId"],
        build.asset_id().expect("hosted Asset").to_string()
    );
    assert_eq!(
        serde_json::from_value::<BuildEvidence>(encoded.clone())
            .expect("deserialize hosted build evidence"),
        *evidence
    );

    encoded["unexpectedSubjectIdentity"] = serde_json::json!("rejected");
    assert!(serde_json::from_value::<BuildEvidence>(encoded).is_err());
}

#[test]
fn final_agent_release_manifest_rejects_archive_and_provenance_tampering() {
    let now = Utc::now();
    let build = succeeded_hosted_agent_build(
        OrganizationId::new(),
        AssetId::new(),
        AssetReleaseId::new(),
        now,
    );
    let evidence = build
        .evidence
        .as_deref()
        .expect("Agent build evidence")
        .clone();

    let mut changed_archive = evidence.clone();
    changed_archive
        .agent_release_manifest
        .as_mut()
        .expect("Agent release manifest")
        .archive
        .size_bytes += 1;
    assert!(BuildEvidence::restore(changed_archive)
        .expect_err("changed archive must fail closed")
        .contains("archive changed its exact bytes"));

    let manifest = AgentReleaseManifest::parse(
        &evidence
            .agent_release_manifest
            .as_ref()
            .expect("Agent release manifest")
            .canonical_acl,
    )
    .expect("final Agent release manifest");
    let replacement_builder = BuildRunId::new();
    assert_ne!(replacement_builder, build.id);
    let changed_manifest = manifest
        .bind_publication(
            evidence.artifact.digest.clone(),
            [
                AgentReleaseProvenance::new(
                    "source",
                    agent_release_source_uri(&evidence.source_content_digest).expect("source URI"),
                    evidence.source_content_digest.clone(),
                )
                .expect("source provenance"),
                AgentReleaseProvenance::new(
                    "builder",
                    agent_release_builder_uri(replacement_builder.as_uuid())
                        .expect("replacement builder URI"),
                    evidence.provenance_digest.clone(),
                )
                .expect("replacement builder provenance"),
            ],
        )
        .expect("internally valid changed manifest");
    let archive = agent_release_manifest_archive(changed_manifest.canonical_acl().as_bytes())
        .expect("changed manifest archive");
    let archive_digest = super::sha256_digest(&archive);
    let mut changed_provenance = evidence;
    changed_provenance.agent_release_manifest = Some(BuildEvidenceAgentReleaseManifest {
        identity: changed_manifest.identity().into(),
        canonical_acl: changed_manifest.canonical_acl().into(),
        archive: BuildArtifact::new(
            artifact_uri(&archive_digest).expect("changed archive URI"),
            archive_digest,
            NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
            archive.len() as u64,
        )
        .expect("changed manifest Artifact"),
    });
    assert!(BuildEvidence::restore(changed_provenance)
        .expect_err("changed provenance must fail closed")
        .contains("changed its provenance binding"));
}

fn artifact(fill: char) -> BuildArtifact {
    let digest = format!("sha256:{}", fill.to_string().repeat(64));
    BuildArtifact::new(
        artifact_uri(&digest).expect("artifact URI"),
        digest,
        NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
        1024,
    )
    .expect("artifact")
}

fn artifact_from_box(output: &NodeBoxBuildOutput) -> BuildArtifact {
    BuildArtifact::new(
        output.artifact.artifact.uri.clone(),
        output.artifact.artifact.digest.clone(),
        output.artifact.artifact.media_type.clone(),
        output.artifact.size_bytes,
    )
    .expect("Box output artifact")
}

fn box_output(artifact_fill: char, descriptor_fill: char) -> NodeBoxBuildOutput {
    let output_digest = format!("sha256:{}", artifact_fill.to_string().repeat(64));
    let cache_digest = format!("sha256:{}", "8".repeat(64));
    let platform = NodeBoxBuildPlatform {
        os: "linux".into(),
        architecture: "amd64".into(),
        variant: None,
    };
    let output = NodeBoxBuildOutput {
        artifact: RuntimeOutputArtifact {
            name: "oci-layout".into(),
            artifact: ArtifactRef {
                uri: artifact_uri(&output_digest).expect("output artifact URI"),
                digest: output_digest,
                media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
            },
            size_bytes: 1024,
        },
        descriptor: NodeBoxBuildDescriptor {
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            digest: format!("sha256:{}", descriptor_fill.to_string().repeat(64)),
            size: 123,
        },
        platforms: vec![platform.clone()],
        manifest_count: 1,
        content_bytes: 456,
        blob_count: 3,
        blob_inventory_digest: format!("sha256:{}", "7".repeat(64)),
        caches: vec![NodeBoxBuildCacheOutput {
            operation_id: "cloud-build-test-linux-amd64".into(),
            artifact: RuntimeOutputArtifact {
                name: "build-cache-test".into(),
                artifact: ArtifactRef {
                    uri: artifact_uri(&cache_digest).expect("cache artifact URI"),
                    digest: cache_digest,
                    media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
                },
                size_bytes: 512,
            },
            receipt: NodeBoxBuildCacheReceipt {
                schema: NodeBoxBuildCacheReceipt::SCHEMA.into(),
                key: format!("sha256:{}", "9".repeat(64)),
                source_digest: format!("sha256:{}", "b".repeat(64)),
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
    output.validate().expect("Box build output");
    output
}

fn publishing_build(now: chrono::DateTime<Utc>) -> BuildRun {
    let mut build = BuildRun::reserve(
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        SourceRevisionId::new(),
        now,
    );
    build
        .begin_preparation(now + Duration::milliseconds(1))
        .expect("preparing build");
    build
        .record_input(
            format!("sha256:{}", "b".repeat(64)),
            artifact('a'),
            now + Duration::milliseconds(2),
        )
        .expect("prepared input");
    build
        .schedule(
            NodeId::new(),
            format!("sha256:{}", "c".repeat(64)),
            now + Duration::milliseconds(3),
        )
        .expect("scheduled build");
    build
        .dispatch(NodeCommandId::new(), now + Duration::milliseconds(4))
        .expect("dispatched build");
    let box_output = box_output('d', 'e');
    let runtime_output = artifact_from_box(&box_output);
    build
        .begin_validation(box_output, now + Duration::milliseconds(5))
        .expect("validating build");
    let output = ValidatedOciBuildOutput {
        artifact: runtime_output,
        descriptor: OciDescriptor::new(
            "application/vnd.oci.image.manifest.v1+json",
            format!("sha256:{}", "e".repeat(64)),
            123,
        )
        .expect("descriptor"),
        platforms: vec![BuildPlatform::parse("linux/amd64").expect("platform")],
        content_bytes: 456,
        blob_count: 3,
    };
    build
        .record_validated_output(output.clone(), now + Duration::milliseconds(6))
        .expect("validated output");
    build
        .begin_publication(
            OciPublicationTarget::new(
                "registry.example",
                format!("a3s-cloud/builds/{}", build.id),
                output.descriptor,
            )
            .expect("publication target"),
            now + Duration::milliseconds(7),
        )
        .expect("publishing build");
    build
}

fn attesting_build(now: chrono::DateTime<Utc>) -> BuildRun {
    let mut build = publishing_build(now);
    let target = build
        .publication_target
        .clone()
        .expect("publication target");
    build
        .record_published_artifact(
            PublishedOciArtifact::from_target(&target),
            now + Duration::milliseconds(8),
        )
        .expect("published artifact");
    build
        .begin_attestation(now + Duration::milliseconds(9))
        .expect("attesting build");
    build
}
