use super::metadata::read_buildkit_descriptor;
use super::oci_layout::{validate_oci_layout, OciLayoutLimits};
use super::{BuildkitBuildService, BuildkitConnection};
use crate::modules::artifacts::domain::{
    BuildServiceError, IBuildService, OciBuildRequest, OciDescriptor,
};
use crate::modules::sources::domain::{BuildPlatform, BuildRecipe};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::TempDir;
use uuid::Uuid;

const OCI_INDEX: &str = "application/vnd.oci.image.index.v1+json";
const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
const OCI_CONFIG: &str = "application/vnd.oci.image.config.v1+json";
const OCI_LAYER: &str = "application/vnd.oci.image.layer.v1.tar";

#[test]
fn buildkit_connection_rejects_unauthenticated_remote_tcp() {
    assert!(BuildkitConnection::insecure_loopback_for_conformance("tcp://127.0.0.1:1234").is_ok());
    assert!(BuildkitConnection::insecure_loopback_for_conformance("tcp://192.0.2.1:1234").is_err());
    assert!(BuildkitConnection::unix("unix:///run/buildkit/buildkitd.sock").is_ok());
    assert!(BuildkitConnection::unix("tcp://127.0.0.1:1234").is_err());
}

#[tokio::test]
async fn oci_validation_binds_metadata_platforms_and_every_blob() {
    let fixture = OciFixture::create().await;
    let descriptor = read_buildkit_descriptor(&fixture.metadata)
        .await
        .expect("BuildKit descriptor");
    assert_eq!(descriptor, fixture.descriptor);

    let validated = validate_oci_layout(
        &fixture.layout,
        &descriptor,
        &[BuildPlatform::parse("linux/amd64").expect("platform")],
        OciLayoutLimits::new(64, 16 * 1024 * 1024).expect("limits"),
    )
    .await
    .expect("valid OCI layout");
    assert_eq!(validated.platforms.len(), 1);
    assert_eq!(validated.blob_count, 3);
    assert!(validated.content_bytes > 0);

    tokio::fs::write(&fixture.layer, b"corrupt layer")
        .await
        .expect("corrupt layer");
    let error = validate_oci_layout(
        &fixture.layout,
        &descriptor,
        &[BuildPlatform::parse("linux/amd64").expect("platform")],
        OciLayoutLimits::new(64, 16 * 1024 * 1024).expect("limits"),
    )
    .await
    .expect_err("corrupt OCI layout");
    assert!(matches!(error, BuildServiceError::Integrity(_)));
}

#[tokio::test]
async fn buildkit_metadata_must_match_its_root_descriptor() {
    let fixture = OciFixture::create().await;
    let mut metadata: Value = serde_json::from_slice(
        &tokio::fs::read(&fixture.metadata)
            .await
            .expect("BuildKit metadata"),
    )
    .expect("metadata JSON");
    metadata["containerimage.digest"] = json!(format!("sha256:{}", "f".repeat(64)));
    tokio::fs::write(
        &fixture.metadata,
        serde_json::to_vec(&metadata).expect("metadata encoding"),
    )
    .await
    .expect("mismatched metadata");

    let error = read_buildkit_descriptor(&fixture.metadata)
        .await
        .expect_err("mismatched descriptor");
    assert!(matches!(error, BuildServiceError::Integrity(_)));
}

#[cfg(unix)]
#[tokio::test]
async fn build_output_replays_without_reexecution_and_revalidates_integrity() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = OciFixture::create().await;
    let source = fixture.root.path().join("source");
    tokio::fs::create_dir(&source)
        .await
        .expect("source directory");
    tokio::fs::write(source.join("Dockerfile"), "FROM scratch\n")
        .await
        .expect("Dockerfile");
    let executable = fixture.root.path().join("fake-buildctl");
    let script = format!(
        "#!/bin/sh\nset -eu\noutput=\nmetadata=\nprevious=\nfor argument in \"$@\"; do\n  if [ \"$previous\" = output ]; then output=$argument; previous=; continue; fi\n  if [ \"$previous\" = metadata ]; then metadata=$argument; previous=; continue; fi\n  case \"$argument\" in\n    --output) previous=output ;;\n    --metadata-file) previous=metadata ;;\n  esac\ndone\ndestination=$(printf '%s' \"$output\" | sed -n 's/.*dest=\\([^,]*\\).*/\\1/p')\ncp -R '{}' \"$destination\"\nmkdir \"$destination/ingest\"\ncp '{}' \"$metadata\"\n",
        fixture.layout.display(),
        fixture.metadata.display(),
    );
    tokio::fs::write(&executable, script)
        .await
        .expect("fake buildctl");
    let mut permissions = tokio::fs::metadata(&executable)
        .await
        .expect("fake buildctl metadata")
        .permissions();
    permissions.set_mode(0o700);
    tokio::fs::set_permissions(&executable, permissions)
        .await
        .expect("fake buildctl permissions");
    let output_root = fixture.root.path().join("outputs");
    let service = BuildkitBuildService::new(
        &executable,
        BuildkitConnection::insecure_loopback_for_conformance("tcp://127.0.0.1:1234")
            .expect("conformance connection"),
        &output_root,
        Duration::from_secs(10),
        64,
        16 * 1024 * 1024,
    )
    .expect("BuildKit service");
    let build_id = Uuid::now_v7();
    let request = request(build_id, source);

    let built = service.build(&request).await.expect("first build");
    assert_eq!(built.descriptor, fixture.descriptor);
    tokio::fs::remove_file(&executable)
        .await
        .expect("remove fake buildctl");
    assert_eq!(
        service.build(&request).await.expect("immutable replay"),
        built
    );

    let committed_layer = built
        .oci_layout_directory
        .join("blobs/sha256")
        .join(fixture.layer.file_name().expect("layer digest"));
    tokio::fs::write(committed_layer, b"tampered")
        .await
        .expect("tamper with committed output");
    let error = service.build(&request).await.expect_err("tampered output");
    assert!(matches!(error, BuildServiceError::Integrity(_)));

    let conflict = service
        .build(
            &OciBuildRequest::new(
                build_id,
                request.source_directory().to_owned(),
                format!("sha256:{}", "c".repeat(64)),
                request.recipe().clone(),
            )
            .expect("conflicting request"),
        )
        .await
        .expect_err("conflicting build");
    assert!(matches!(conflict, BuildServiceError::Conflict));
    service.remove(build_id).await.expect("remove build");
    service.remove(build_id).await.expect("idempotent remove");
}

#[tokio::test]
#[ignore = "requires a real rootless BuildKit worker"]
async fn real_rootless_buildkit_builds_and_validates_oci_layout() {
    let buildctl =
        PathBuf::from(std::env::var("A3S_CLOUD_TEST_BUILDCTL").expect("BuildKit client path"));
    let address =
        std::env::var("A3S_CLOUD_TEST_BUILDKIT_ADDRESS").expect("BuildKit endpoint address");
    let root = tempfile::tempdir().expect("BuildKit conformance directory");
    let source = root.path().join("source");
    tokio::fs::create_dir(&source)
        .await
        .expect("source directory");
    tokio::fs::write(
        source.join("Dockerfile"),
        "FROM scratch\nCOPY message.txt /message.txt\n",
    )
    .await
    .expect("Dockerfile");
    tokio::fs::write(
        source.join("message.txt"),
        "A3S Cloud rootless BuildKit conformance\n",
    )
    .await
    .expect("build input");
    let service = BuildkitBuildService::new(
        buildctl,
        BuildkitConnection::insecure_loopback_for_conformance(address)
            .expect("conformance connection"),
        root.path().join("outputs"),
        Duration::from_secs(120),
        1_024,
        1024 * 1024 * 1024,
    )
    .expect("BuildKit service");
    let request = request(Uuid::now_v7(), source);

    let built = service.build(&request).await.expect("rootless OCI build");
    assert_eq!(built.platforms.len(), 1);
    assert_eq!(built.platforms[0].as_str(), "linux/amd64");
    assert!(built.blob_count >= 3);
    assert!(built.content_bytes > built.descriptor.size());
    assert_eq!(
        service.build(&request).await.expect("validated replay"),
        built
    );
    service
        .remove(request.build_id())
        .await
        .expect("build cleanup");
}

fn request(build_id: Uuid, source: PathBuf) -> OciBuildRequest {
    OciBuildRequest::new(
        build_id,
        source,
        format!("sha256:{}", "a".repeat(64)),
        BuildRecipe::dockerfile(
            BuildRecipe::SCHEMA,
            BuildRecipe::DOCKERFILE_KIND,
            ".",
            "Dockerfile",
            None,
            vec!["linux/amd64".into()],
        )
        .expect("build recipe"),
    )
    .expect("OCI build request")
}

struct OciFixture {
    root: TempDir,
    layout: PathBuf,
    metadata: PathBuf,
    layer: PathBuf,
    descriptor: OciDescriptor,
}

impl OciFixture {
    async fn create() -> Self {
        let root = tempfile::tempdir().expect("OCI fixture");
        let layout = root.path().join("fixture-layout");
        let blobs = layout.join("blobs/sha256");
        tokio::fs::create_dir_all(&blobs)
            .await
            .expect("OCI blobs directory");
        tokio::fs::write(
            layout.join("oci-layout"),
            br#"{"imageLayoutVersion":"1.0.0"}"#,
        )
        .await
        .expect("OCI layout marker");

        let layer = write_blob(&blobs, OCI_LAYER, b"fixture layer\n").await;
        let layer_digest = layer["digest"].as_str().expect("layer digest").to_owned();
        let config = write_json_blob(
            &blobs,
            OCI_CONFIG,
            &json!({
                "architecture": "amd64",
                "os": "linux",
                "config": {},
                "rootfs": {
                    "type": "layers",
                    "diff_ids": [layer_digest],
                },
            }),
        )
        .await;
        let manifest = write_json_blob(
            &blobs,
            OCI_MANIFEST,
            &json!({
                "schemaVersion": 2,
                "mediaType": OCI_MANIFEST,
                "config": config,
                "layers": [layer.clone()],
            }),
        )
        .await;
        let mut root_descriptor = manifest.clone();
        root_descriptor["platform"] = json!({"architecture": "amd64", "os": "linux"});
        tokio::fs::write(
            layout.join("index.json"),
            serde_json::to_vec(&json!({
                "schemaVersion": 2,
                "mediaType": OCI_INDEX,
                "manifests": [root_descriptor],
            }))
            .expect("OCI index encoding"),
        )
        .await
        .expect("OCI index");
        let descriptor = OciDescriptor::new(
            manifest["mediaType"].as_str().expect("manifest media type"),
            manifest["digest"].as_str().expect("manifest digest"),
            manifest["size"].as_u64().expect("manifest size"),
        )
        .expect("root descriptor");
        let metadata = root.path().join("metadata.json");
        tokio::fs::write(
            &metadata,
            serde_json::to_vec(&json!({
                "containerimage.digest": descriptor.digest(),
                "containerimage.descriptor": manifest,
            }))
            .expect("metadata encoding"),
        )
        .await
        .expect("BuildKit metadata");
        Self {
            root,
            layout,
            metadata,
            layer: blobs.join(layer_digest.strip_prefix("sha256:").expect("SHA-256 layer")),
            descriptor,
        }
    }
}

async fn write_json_blob(blobs: &Path, media_type: &str, value: &Value) -> Value {
    write_blob(
        blobs,
        media_type,
        &serde_json::to_vec(value).expect("blob encoding"),
    )
    .await
}

async fn write_blob(blobs: &Path, media_type: &str, content: &[u8]) -> Value {
    let digest = format!("sha256:{:x}", Sha256::digest(content));
    tokio::fs::write(
        blobs.join(digest.strip_prefix("sha256:").expect("SHA-256 blob")),
        content,
    )
    .await
    .expect("OCI blob");
    json!({
        "mediaType": media_type,
        "digest": digest,
        "size": content.len(),
    })
}
