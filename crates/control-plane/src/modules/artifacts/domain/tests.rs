use super::{
    BuildArtifact, BuildRun, BuildRunStatus, OciBuildRequest, OciDescriptor, OciPublicationTarget,
    PublishedOciArtifact, ValidatedOciBuildOutput,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeCommandId, NodeId, OrganizationId, ProjectId, SourceRevisionId,
};
use crate::modules::sources::domain::{BuildPlatform, BuildRecipe};
use chrono::{Duration, Utc};
use std::path::PathBuf;
use uuid::Uuid;

#[test]
fn oci_build_request_requires_an_immutable_identity_and_canonical_recipe() {
    let recipe = BuildRecipe::dockerfile(
        BuildRecipe::SCHEMA,
        BuildRecipe::DOCKERFILE_KIND,
        ".",
        "Dockerfile",
        None,
        vec!["linux/amd64".into()],
    )
    .expect("build recipe");

    let request = OciBuildRequest::new(
        Uuid::now_v7(),
        PathBuf::from("/tmp/a3s-cloud-source"),
        format!("sha256:{}", "a".repeat(64)),
        recipe,
    )
    .expect("OCI build request");
    assert_eq!(
        request.source_content_digest(),
        format!("sha256:{}", "a".repeat(64))
    );

    assert!(OciBuildRequest::new(
        Uuid::nil(),
        PathBuf::from("/tmp/a3s-cloud-source"),
        format!("sha256:{}", "a".repeat(64)),
        request.recipe().clone(),
    )
    .is_err());
    assert!(OciBuildRequest::new(
        Uuid::now_v7(),
        PathBuf::from("relative/source"),
        format!("sha256:{}", "a".repeat(64)),
        request.recipe().clone(),
    )
    .is_err());
    assert!(OciBuildRequest::new(
        Uuid::now_v7(),
        PathBuf::from("/tmp/a3s-cloud-source"),
        format!("sha256:{}", "A".repeat(64)),
        request.recipe().clone(),
    )
    .is_err());
}

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
fn build_run_binds_one_source_to_one_runtime_task_and_validated_output() {
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
    let output_artifact = artifact('d');
    build
        .begin_validation(
            output_artifact.clone(),
            requested_at + Duration::milliseconds(9),
        )
        .expect("begin validation");
    assert!(build
        .begin_cleanup(
            NodeCommandId::new(),
            requested_at + Duration::milliseconds(10),
        )
        .is_err());
    let validating = build.clone();
    build
        .begin_validation(
            output_artifact.clone(),
            requested_at + Duration::milliseconds(11),
        )
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
    let cleanup_command_id = NodeCommandId::new();
    build
        .begin_cleanup(
            cleanup_command_id,
            requested_at + Duration::milliseconds(18),
        )
        .expect("begin cleanup");
    let cleanup = build.clone();
    build
        .begin_cleanup(
            cleanup_command_id,
            requested_at + Duration::milliseconds(19),
        )
        .expect("replay cleanup");
    assert_eq!(build, cleanup);
    build
        .complete(requested_at + Duration::milliseconds(20))
        .expect("complete");
    let completed = build.clone();
    build
        .complete(requested_at + Duration::milliseconds(21))
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
    BuildRun::restore(build).expect("restore cancelling published build");
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

fn artifact(fill: char) -> BuildArtifact {
    BuildArtifact::new(
        format!("a3s-cloud-blob://sha256/{}", fill.to_string().repeat(64)),
        format!("sha256:{}", fill.to_string().repeat(64)),
        "application/vnd.a3s.cloud.runtime-archive.v1.tar",
        1024,
    )
    .expect("artifact")
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
    let runtime_output = artifact('d');
    build
        .begin_validation(runtime_output.clone(), now + Duration::milliseconds(5))
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
