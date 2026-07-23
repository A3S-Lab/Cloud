use super::*;
use crate::modules::artifacts::domain::{
    canonical_json, dsse_pae, sha256_digest, BuildRun, IBuildEvidenceGenerator, INodeArtifactStore,
    NodeArtifactDescriptor, OciPublicationTarget, PublishedOciArtifact, DSSE_PAYLOAD_TYPE,
};
use crate::modules::artifacts::infrastructure::{
    LocalBuildEvidenceSigner, LocalNodeArtifactStore, RuntimeBuildEvidenceGenerator,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeCommandId, NodeId, OrganizationId, ProjectId, SourceRevisionId,
};
use crate::modules::sources::domain::{
    ExternalSourceRevision, GitCommitSha, GitProvider, GitRepository, NewExternalSourceRevision,
};
use a3s_cloud_contracts::artifact_uri;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{Duration, Utc};
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use serde_json::{json, Value};
use std::fs::File;
use tar::{Builder, EntryType, Header};

const OCI_INDEX: &str = "application/vnd.oci.image.index.v1+json";
const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
const OCI_CONFIG: &str = "application/vnd.oci.image.config.v1+json";
const OCI_LAYER: &str = "application/vnd.oci.image.layer.v1.tar";

#[tokio::test]
async fn runtime_output_revalidates_the_complete_oci_graph_and_stored_bytes(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let export = root.path().join("export");
    let descriptor = create_export(&export)?;
    let archive = root.path().join("output.tar");
    archive_directory(&export, &archive)?;
    let store_root = root.path().join("store");
    let store = Arc::new(LocalNodeArtifactStore::new(&store_root, 64 * 1024 * 1024)?);
    let artifact = admit(&store, &archive).await?;
    let validator = RuntimeBuildOutputValidator::new(
        store,
        root.path().join("validation"),
        64 * 1024 * 1024,
        1_024,
        64 * 1024 * 1024,
        64,
        64 * 1024 * 1024,
    )?;
    let recipe = recipe()?;

    let validated = validator.validate(&artifact, &recipe).await?;
    assert_eq!(validated.descriptor, descriptor);
    assert_eq!(validated.platforms, recipe.platforms());
    assert_eq!(validated.blob_count, 3);

    let blob = store_root
        .join("blobs/sha256")
        .join(artifact.digest.strip_prefix("sha256:").ok_or("digest")?);
    let mut bytes = std::fs::read(&blob)?;
    bytes[0] ^= 0xff;
    std::fs::write(blob, bytes)?;
    assert!(matches!(
        validator.validate(&artifact, &recipe).await,
        Err(BuildOutputValidationError::Integrity(_))
    ));
    Ok(())
}

#[tokio::test]
async fn runtime_output_rejects_non_file_archive_entries() -> Result<(), Box<dyn std::error::Error>>
{
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
    let validator = RuntimeBuildOutputValidator::new(
        store,
        root.path().join("validation"),
        1024 * 1024,
        32,
        1024 * 1024,
        16,
        1024 * 1024,
    )?;
    assert!(matches!(
        validator.validate(&artifact, &recipe()?).await,
        Err(BuildOutputValidationError::Integrity(_))
    ));
    Ok(())
}

#[tokio::test]
async fn runtime_build_evidence_revalidates_oci_output_and_signs_bound_spdx_and_slsa(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let export = root.path().join("export");
    let descriptor = create_export(&export)?;
    let archive = root.path().join("output.tar");
    archive_directory(&export, &archive)?;
    let store = Arc::new(LocalNodeArtifactStore::new(
        root.path().join("store"),
        64 * 1024 * 1024,
    )?);
    let runtime_artifact = admit(&store, &archive).await?;
    let validation_root = root.path().join("validation");
    let validator = Arc::new(RuntimeBuildOutputValidator::new(
        store,
        &validation_root,
        64 * 1024 * 1024,
        1_024,
        64 * 1024 * 1024,
        64,
        64 * 1024 * 1024,
    )?);
    let recipe = recipe()?;
    let output = validator.validate(&runtime_artifact, &recipe).await?;
    assert_eq!(output.descriptor, descriptor);

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
        format!("sha256:{}", "1".repeat(64)),
        runtime_artifact.clone(),
        requested_at + Duration::milliseconds(2),
    )?;
    build.schedule(
        NodeId::new(),
        format!("sha256:{}", "2".repeat(64)),
        requested_at + Duration::milliseconds(3),
    )?;
    build.dispatch(
        NodeCommandId::new(),
        requested_at + Duration::milliseconds(4),
    )?;
    build.begin_validation(runtime_artifact, requested_at + Duration::milliseconds(5))?;
    build.record_validated_output(output, requested_at + Duration::milliseconds(6))?;
    let target = OciPublicationTarget::new(
        "registry.example.test",
        format!("a3s-cloud/builds/{}", build.id),
        descriptor,
    )?;
    build.begin_publication(target.clone(), requested_at + Duration::milliseconds(7))?;
    build.record_published_artifact(
        PublishedOciArtifact::from_target(&target),
        requested_at + Duration::milliseconds(8),
    )?;
    build.begin_attestation(requested_at + Duration::milliseconds(9))?;

    let key_path = root.path().join("signing/build-evidence-ed25519.pk8");
    let signer = Arc::new(LocalBuildEvidenceSigner::load_or_create(&key_path).await?);
    let builder_digest = format!("sha256:{}", "b".repeat(64));
    let generator = RuntimeBuildEvidenceGenerator::new(
        validator,
        signer,
        ArtifactRef {
            uri: format!("oci://docker.io/moby/buildkit@{builder_digest}"),
            digest: builder_digest,
            media_type: OCI_INDEX.into(),
        },
    )?;
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

fn create_export(
    export: &Path,
) -> Result<crate::modules::artifacts::domain::OciDescriptor, Box<dyn std::error::Error>> {
    let layout = export.join("oci");
    let blobs = layout.join("blobs/sha256");
    std::fs::create_dir_all(&blobs)?;
    std::fs::write(
        layout.join("oci-layout"),
        br#"{"imageLayoutVersion":"1.0.0"}"#,
    )?;
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
    std::fs::write(
        layout.join("index.json"),
        serde_json::to_vec(&json!({
            "schemaVersion": 2,
            "mediaType": OCI_INDEX,
            "manifests": [root_descriptor],
        }))?,
    )?;
    let descriptor = crate::modules::artifacts::domain::OciDescriptor::new(
        manifest["mediaType"]
            .as_str()
            .ok_or("manifest media type")?,
        manifest["digest"].as_str().ok_or("manifest digest")?,
        manifest["size"].as_u64().ok_or("manifest size")?,
    )?;
    std::fs::write(
        export.join("buildkit-metadata.json"),
        serde_json::to_vec(&json!({
            "containerimage.digest": descriptor.digest(),
            "containerimage.descriptor": manifest,
        }))?,
    )?;
    Ok(descriptor)
}

fn archive_directory(export: &Path, destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = Builder::new(File::create(destination)?);
    for directory in ["oci", "oci/blobs", "oci/blobs/sha256"] {
        builder.append_dir(directory, export.join(directory))?;
    }
    builder.append_path_with_name(
        export.join("buildkit-metadata.json"),
        "buildkit-metadata.json",
    )?;
    builder.append_path_with_name(export.join("oci/oci-layout"), "oci/oci-layout")?;
    builder.append_path_with_name(export.join("oci/index.json"), "oci/index.json")?;
    let mut blobs =
        std::fs::read_dir(export.join("oci/blobs/sha256"))?.collect::<Result<Vec<_>, _>>()?;
    blobs.sort_by_key(|entry| entry.file_name());
    for blob in blobs {
        builder.append_path_with_name(
            blob.path(),
            Path::new("oci/blobs/sha256").join(blob.file_name()),
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
