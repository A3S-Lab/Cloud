use super::*;
use crate::modules::artifacts::domain::{INodeArtifactStore, NodeArtifactDescriptor};
use crate::modules::artifacts::infrastructure::LocalNodeArtifactStore;
use a3s_cloud_contracts::artifact_uri;
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
