use super::*;
use crate::modules::artifacts::domain::{
    canonical_json, dsse_pae, sha256_digest, BuildRun, IBuildEvidenceGenerator,
    IBuildOutputValidator, INodeArtifactStore, NodeArtifactDescriptor, OciDescriptor,
    OciPublicationTarget, PublishedOciArtifact, DSSE_PAYLOAD_TYPE,
};
use crate::modules::artifacts::infrastructure::{
    BoxBuildEvidenceGenerator, LocalBuildEvidenceSigner, LocalNodeArtifactStore,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeCommandId, NodeId, OrganizationId, ProjectId, SourceRevisionId,
};
use crate::modules::sources::domain::{
    BuildRecipe, ExternalSourceRevision, GitCommitSha, GitProvider, GitRepository,
    NewExternalSourceRevision,
};
use a3s_cloud_contracts::{
    artifact_uri, NodeBoxBuildCacheOutput, NodeBoxBuildCacheReceipt, NodeBoxBuildDescriptor,
    NodeBoxBuildOutput, NodeBoxBuildPlatform, BOX_BUILD_OUTPUT_NAME,
    NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
use a3s_runtime::contract::{ArtifactRef, RuntimeOutputArtifact};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{Duration, Utc};
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use tar::{Builder, EntryType, Header};

const OCI_INDEX: &str = "application/vnd.oci.image.index.v1+json";
const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
const OCI_CONFIG: &str = "application/vnd.oci.image.config.v1+json";
const OCI_LAYER: &str = "application/vnd.oci.image.layer.v1.tar";

struct OciFixture {
    descriptor: OciDescriptor,
    content_bytes: u64,
    blob_count: u64,
    blob_inventory_digest: String,
}

#[tokio::test]
async fn box_output_revalidates_the_complete_oci_graph_and_stored_bytes(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let layout = root.path().join("layout");
    let fixture = create_layout(&layout)?;
    let archive = root.path().join("output.tar");
    archive_layout(&layout, &archive)?;
    let store_root = root.path().join("store");
    let store = Arc::new(LocalNodeArtifactStore::new(&store_root, 64 * 1024 * 1024)?);
    let artifact = admit(&store, &archive).await?;
    let receipt = receipt(&artifact, &fixture);
    let validator = validator(store, root.path().join("validation"))?;
    let recipe = recipe()?;

    let validated = validator.validate(&receipt, &recipe).await?;
    assert_eq!(validated.descriptor, fixture.descriptor);
    assert_eq!(validated.platforms, recipe.platforms());
    assert_eq!(validated.content_bytes, fixture.content_bytes);
    assert_eq!(validated.blob_count, fixture.blob_count as usize);

    let blob = store_root
        .join("blobs/sha256")
        .join(artifact.digest.strip_prefix("sha256:").ok_or("digest")?);
    let mut bytes = std::fs::read(&blob)?;
    bytes[0] ^= 0xff;
    std::fs::write(blob, bytes)?;
    assert!(matches!(
        validator.validate(&receipt, &recipe).await,
        Err(BuildOutputValidationError::Integrity(_))
    ));
    Ok(())
}

#[tokio::test]
async fn box_output_rejects_non_file_archive_entries() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let archive = root.path().join("output.tar");
    let mut builder = Builder::new(File::create(&archive)?);
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Symlink);
    header.set_mode(0o777);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_size(0);
    header.set_link_name("target")?;
    header.set_cksum();
    builder.append_data(&mut header, "link", std::io::empty())?;
    builder.finish()?;
    drop(builder);

    let store = Arc::new(LocalNodeArtifactStore::new(
        root.path().join("store"),
        1024 * 1024,
    )?);
    let artifact = admit(&store, &archive).await?;
    let fixture = synthetic_fixture();
    let receipt = receipt(&artifact, &fixture);
    let validator = OciBuildOutputValidator::new(
        store,
        root.path().join("validation"),
        1024 * 1024,
        32,
        1024 * 1024,
        16,
        1024 * 1024,
    )?;
    assert!(matches!(
        validator.validate(&receipt, &recipe()?).await,
        Err(BuildOutputValidationError::Integrity(_))
    ));
    Ok(())
}

#[tokio::test]
async fn box_receipt_must_match_every_validated_oci_measurement(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let layout = root.path().join("layout");
    let fixture = create_layout(&layout)?;
    let archive = root.path().join("output.tar");
    archive_layout(&layout, &archive)?;
    let store = Arc::new(LocalNodeArtifactStore::new(
        root.path().join("store"),
        64 * 1024 * 1024,
    )?);
    let artifact = admit(&store, &archive).await?;
    let validator = validator(store, root.path().join("validation"))?;
    let accepted = receipt(&artifact, &fixture);
    validator.validate(&accepted, &recipe()?).await?;

    let mut mismatches = Vec::new();

    let mut descriptor = accepted.clone();
    descriptor.descriptor.digest = digest('a');
    mismatches.push(descriptor);

    let mut platforms = accepted.clone();
    platforms.platforms[0].architecture = "arm64".into();
    platforms.caches[0].receipt.platform.architecture = "arm64".into();
    mismatches.push(platforms);

    let mut bytes = accepted.clone();
    bytes.content_bytes += 1;
    mismatches.push(bytes);

    let mut blobs = accepted.clone();
    blobs.blob_count += 1;
    mismatches.push(blobs);

    let mut inventory = accepted;
    inventory.blob_inventory_digest = digest('b');
    mismatches.push(inventory);

    for mismatch in mismatches {
        assert!(matches!(
            validator.validate(&mismatch, &recipe()?).await,
            Err(BuildOutputValidationError::Integrity(_))
        ));
    }
    Ok(())
}

#[tokio::test]
async fn box_build_evidence_revalidates_oci_output_and_signs_bound_spdx_and_slsa(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let layout = root.path().join("layout");
    let fixture = create_layout(&layout)?;
    let archive = root.path().join("output.tar");
    archive_layout(&layout, &archive)?;
    let store = Arc::new(LocalNodeArtifactStore::new(
        root.path().join("store"),
        64 * 1024 * 1024,
    )?);
    let artifact = admit(&store, &archive).await?;
    let box_output = receipt(&artifact, &fixture);
    let validation_root = root.path().join("validation");
    let validator = Arc::new(validator(store, &validation_root)?);
    let recipe = recipe()?;
    let output = validator.validate(&box_output, &recipe).await?;
    assert_eq!(output.descriptor, fixture.descriptor);

    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let source_revision_id = SourceRevisionId::new();
    let requested_at = Utc::now() - Duration::seconds(1);
    let revision = ExternalSourceRevision::accept(NewExternalSourceRevision {
        organization_id,
        project_id,
        environment_id,
        id: source_revision_id,
        repository: GitRepository::parse(GitProvider::Github, "https://github.com/A3S-Lab/Cloud")?,
        commit_sha: GitCommitSha::parse("a".repeat(40))?,
        recipe,
        accepted_at: requested_at,
    })?;
    let mut build = BuildRun::reserve(
        organization_id,
        project_id,
        environment_id,
        source_revision_id,
        requested_at,
    );
    build.begin_preparation(requested_at + Duration::milliseconds(1))?;
    build.record_input(
        digest('1'),
        artifact.clone(),
        requested_at + Duration::milliseconds(2),
    )?;
    build.schedule(
        NodeId::new(),
        digest('2'),
        requested_at + Duration::milliseconds(3),
    )?;
    build.dispatch(
        NodeCommandId::new(),
        requested_at + Duration::milliseconds(4),
    )?;
    build.begin_validation(box_output, requested_at + Duration::milliseconds(5))?;
    build.record_validated_output(output, requested_at + Duration::milliseconds(6))?;
    let target = OciPublicationTarget::new(
        "registry.example.test",
        format!("a3s-cloud/builds/{}", build.id),
        fixture.descriptor,
    )?;
    build.begin_publication(target.clone(), requested_at + Duration::milliseconds(7))?;
    build.record_published_artifact(
        PublishedOciArtifact::from_target(&target),
        requested_at + Duration::milliseconds(8),
    )?;
    build.begin_attestation(requested_at + Duration::milliseconds(9))?;

    let key_path = root.path().join("signing/build-evidence-ed25519.pk8");
    let signer = Arc::new(LocalBuildEvidenceSigner::load_or_create(&key_path).await?);
    let generator = BoxBuildEvidenceGenerator::new(validator, signer)?;
    let attested_at = requested_at + Duration::milliseconds(10);
    let evidence = generator.generate(&build, &revision, attested_at).await?;

    evidence.validate()?;
    assert_eq!(
        evidence.artifact,
        PublishedOciArtifact::from_target(&target)
    );
    assert_eq!(evidence.sbom.files.len(), 3);
    assert_eq!(evidence.sbom.relationships.len(), 4);
    assert_eq!(evidence.provenance.subject.len(), 2);
    assert_eq!(
        evidence
            .provenance
            .predicate
            .build_definition
            .external_parameters
            .recipe,
        revision.recipe
    );
    assert_eq!(
        evidence.verification_state,
        crate::modules::artifacts::domain::BuildEvidenceVerificationState::Verified
    );

    let provenance = canonical_json(&evidence.provenance)?;
    let pae = dsse_pae(DSSE_PAYLOAD_TYPE, &provenance)?;
    let signature = STANDARD.decode(&evidence.envelope.signatures[0].signature)?;
    let key = Ed25519KeyPair::from_pkcs8(&std::fs::read(&key_path)?)
        .map_err(|_| "persisted evidence key is not valid Ed25519 PKCS#8")?;
    assert_eq!(
        evidence.signing_key.key_id,
        sha256_digest(key.public_key().as_ref())
    );
    assert_eq!(
        evidence.signing_key.public_key,
        STANDARD.encode(key.public_key().as_ref())
    );
    UnparsedPublicKey::new(&ED25519, key.public_key().as_ref())
        .verify(&pae, &signature)
        .map_err(|_| "generated DSSE signature did not verify")?;

    build.record_evidence(evidence, attested_at)?;
    build.begin_cleanup(
        NodeCommandId::new(),
        requested_at + Duration::milliseconds(11),
    )?;
    assert!(build.evidence.is_some());
    assert!(
        std::fs::read_dir(&validation_root)?.next().is_none(),
        "evidence generation retained materialized OCI output"
    );
    Ok(())
}

fn validator(
    store: Arc<LocalNodeArtifactStore>,
    staging_root: impl Into<std::path::PathBuf>,
) -> Result<OciBuildOutputValidator, String> {
    OciBuildOutputValidator::new(
        store,
        staging_root,
        64 * 1024 * 1024,
        1_024,
        64 * 1024 * 1024,
        64,
        64 * 1024 * 1024,
    )
}

async fn admit(
    store: &Arc<LocalNodeArtifactStore>,
    archive: &Path,
) -> Result<BuildArtifact, Box<dyn std::error::Error>> {
    let bytes = tokio::fs::read(archive).await?;
    let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
    let reference = ArtifactRef {
        uri: artifact_uri(&digest)?,
        digest: digest.clone(),
        media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
    };
    let descriptor = NodeArtifactDescriptor::new(reference.clone(), bytes.len() as u64)?;
    let file = tokio::fs::File::open(archive).await?;
    store.put(&descriptor, Box::pin(file)).await?;
    Ok(BuildArtifact::new(
        reference.uri,
        digest,
        reference.media_type,
        bytes.len() as u64,
    )?)
}

fn recipe() -> Result<BuildRecipe, String> {
    BuildRecipe::dockerfile(
        BuildRecipe::SCHEMA,
        BuildRecipe::DOCKERFILE_KIND,
        ".",
        "Dockerfile",
        None,
        vec!["linux/amd64".into()],
    )
}

fn receipt(artifact: &BuildArtifact, fixture: &OciFixture) -> NodeBoxBuildOutput {
    let artifact_ref = ArtifactRef {
        uri: artifact.uri.clone(),
        digest: artifact.digest.clone(),
        media_type: artifact.media_type.clone(),
    };
    let platform = NodeBoxBuildPlatform {
        os: "linux".into(),
        architecture: "amd64".into(),
        variant: None,
    };
    NodeBoxBuildOutput {
        artifact: RuntimeOutputArtifact {
            name: BOX_BUILD_OUTPUT_NAME.into(),
            artifact: artifact_ref.clone(),
            size_bytes: artifact.size_bytes,
        },
        descriptor: NodeBoxBuildDescriptor {
            media_type: fixture.descriptor.media_type().into(),
            digest: fixture.descriptor.digest().into(),
            size: fixture.descriptor.size(),
        },
        platforms: vec![platform.clone()],
        manifest_count: 1,
        content_bytes: fixture.content_bytes,
        blob_count: fixture.blob_count,
        blob_inventory_digest: fixture.blob_inventory_digest.clone(),
        caches: vec![NodeBoxBuildCacheOutput {
            operation_id: "fixture-linux-amd64".into(),
            artifact: RuntimeOutputArtifact {
                name: "build-cache-fixture".into(),
                artifact: artifact_ref,
                size_bytes: artifact.size_bytes,
            },
            receipt: NodeBoxBuildCacheReceipt {
                schema: NodeBoxBuildCacheReceipt::SCHEMA.into(),
                key: digest('3'),
                source_digest: artifact.digest.clone(),
                plan_digest: digest('4'),
                descriptor: NodeBoxBuildDescriptor {
                    media_type: fixture.descriptor.media_type().into(),
                    digest: fixture.descriptor.digest().into(),
                    size: fixture.descriptor.size(),
                },
                platform,
                content_bytes: fixture.content_bytes,
                entry_count: 1,
                blob_count: fixture.blob_count,
                blob_inventory_digest: fixture.blob_inventory_digest.clone(),
            },
        }],
    }
}

fn synthetic_fixture() -> OciFixture {
    OciFixture {
        descriptor: OciDescriptor::new(OCI_MANIFEST, digest('5'), 128)
            .expect("synthetic descriptor"),
        content_bytes: 512,
        blob_count: 3,
        blob_inventory_digest: digest('6'),
    }
}

fn create_layout(layout: &Path) -> Result<OciFixture, Box<dyn std::error::Error>> {
    let blobs = layout.join("blobs/sha256");
    std::fs::create_dir_all(&blobs)?;
    let marker = br#"{"imageLayoutVersion":"1.0.0"}"#;
    std::fs::write(layout.join("oci-layout"), marker)?;
    let layer = write_blob(&blobs, OCI_LAYER, b"fixture layer\n")?;
    let layer_digest = layer["digest"].as_str().ok_or("layer digest")?.to_owned();
    let config = write_json_blob(
        &blobs,
        OCI_CONFIG,
        &json!({
            "architecture": "amd64",
            "os": "linux",
            "config": {},
            "rootfs": {"type": "layers", "diff_ids": [layer_digest]},
        }),
    )?;
    let manifest = write_json_blob(
        &blobs,
        OCI_MANIFEST,
        &json!({
            "schemaVersion": 2,
            "mediaType": OCI_MANIFEST,
            "config": config,
            "layers": [layer],
        }),
    )?;
    let mut root_descriptor = manifest.clone();
    root_descriptor["platform"] = json!({"architecture": "amd64", "os": "linux"});
    let index = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": OCI_INDEX,
        "manifests": [root_descriptor],
    }))?;
    std::fs::write(layout.join("index.json"), &index)?;

    let descriptor = OciDescriptor::new(
        manifest["mediaType"]
            .as_str()
            .ok_or("manifest media type")?,
        manifest["digest"].as_str().ok_or("manifest digest")?,
        manifest["size"].as_u64().ok_or("manifest size")?,
    )?;
    let mut inventory = [&manifest, &config, &layer]
        .into_iter()
        .map(|value| {
            Ok::<_, Box<dyn std::error::Error>>((
                value["digest"].as_str().ok_or("blob digest")?.to_owned(),
                value["size"].as_u64().ok_or("blob size")?,
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    inventory.sort_by(|left, right| left.0.cmp(&right.0));
    let mut inventory_hasher = Sha256::new();
    for (digest, size) in &inventory {
        inventory_hasher.update(digest.as_bytes());
        inventory_hasher.update([0]);
        inventory_hasher.update(size.to_be_bytes());
    }
    let blob_bytes = inventory.iter().map(|(_, size)| size).sum::<u64>();
    Ok(OciFixture {
        descriptor,
        content_bytes: marker.len() as u64 + index.len() as u64 + blob_bytes,
        blob_count: inventory.len() as u64,
        blob_inventory_digest: format!("sha256:{:x}", inventory_hasher.finalize()),
    })
}

fn archive_layout(layout: &Path, destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = Builder::new(File::create(destination)?);
    builder.append_dir("blobs", layout.join("blobs"))?;
    builder.append_dir("blobs/sha256", layout.join("blobs/sha256"))?;
    builder.append_path_with_name(layout.join("oci-layout"), "oci-layout")?;
    builder.append_path_with_name(layout.join("index.json"), "index.json")?;
    let mut blobs =
        std::fs::read_dir(layout.join("blobs/sha256"))?.collect::<Result<Vec<_>, _>>()?;
    blobs.sort_by_key(|entry| entry.file_name());
    for blob in blobs {
        builder.append_path_with_name(
            blob.path(),
            Path::new("blobs/sha256").join(blob.file_name()),
        )?;
    }
    builder.finish()?;
    Ok(())
}

fn write_json_blob(
    blobs: &Path,
    media_type: &str,
    value: &Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    write_blob(blobs, media_type, &serde_json::to_vec(value)?)
}

fn write_blob(
    blobs: &Path,
    media_type: &str,
    content: &[u8],
) -> Result<Value, Box<dyn std::error::Error>> {
    let digest = format!("sha256:{:x}", Sha256::digest(content));
    std::fs::write(
        blobs.join(digest.strip_prefix("sha256:").ok_or("SHA-256 digest")?),
        content,
    )?;
    Ok(json!({
        "mediaType": media_type,
        "digest": digest,
        "size": content.len(),
    }))
}

fn digest(fill: char) -> String {
    format!("sha256:{}", fill.to_string().repeat(64))
}
