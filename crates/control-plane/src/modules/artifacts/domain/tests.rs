use super::{OciBuildRequest, OciDescriptor};
use crate::modules::sources::domain::BuildRecipe;
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
