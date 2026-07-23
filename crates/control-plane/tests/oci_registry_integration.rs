use a3s_cloud_contracts::{
    artifact_uri, RegistryCredentialMaterial, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
use a3s_cloud_control_plane::modules::artifacts::{
    BuildArtifact, IBuildArtifactPublisher, IBuildOutputValidator, INodeArtifactStore,
    LocalNodeArtifactStore, NodeArtifactDescriptor, OciPublicationRequest, OciPublicationTarget,
    OciRegistryArtifactPublisher, OciRegistryArtifactPublisherOptions, RuntimeBuildOutputValidator,
};
use a3s_cloud_control_plane::modules::sources::domain::BuildRecipe;
use a3s_cloud_control_plane::modules::workloads::{
    IOciArtifactResolver, OciArtifactReference, OciRegistryArtifactResolver,
};
use a3s_runtime::contract::ArtifactRef;
use reqwest::header::{CONTENT_TYPE, LOCATION};
use reqwest::{Client, StatusCode, Url};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tar::Builder;
use uuid::Uuid;

const OCI_INDEX: &str = "application/vnd.oci.image.index.v1+json";
const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
const OCI_CONFIG: &str = "application/vnd.oci.image.config.v1+json";
const OCI_LAYER: &str = "application/vnd.oci.image.layer.v1.tar";

#[tokio::test]
async fn real_registry_resolves_tags_and_preserves_digest_addressability(
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(registry_url) = std::env::var("A3S_CLOUD_TEST_REGISTRY_URL").ok() else {
        return Ok(());
    };
    let base = Url::parse(&registry_url)?;
    let authority = match (base.host_str(), base.port_or_known_default()) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        _ => return Err("registry test URL must contain a host and port".into()),
    };
    let insecure_hosts = if base.scheme() == "http" {
        vec![authority.clone()]
    } else {
        Vec::new()
    };
    let repository = format!("a3s-cloud/resolution-{}", Uuid::now_v7().simple());
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let first_digest = push_fixture(&client, &base, &repository, "stable", b"first").await?;
    let resolver = OciRegistryArtifactResolver::new(Duration::from_secs(10), insecure_hosts)?;
    let tagged = OciArtifactReference {
        uri: format!("oci://{authority}/{repository}:stable"),
        expected_digest: None,
    };

    let first = resolver.resolve(&tagged, None).await?;
    assert_eq!(first.digest, first_digest);
    let second_digest = push_fixture(&client, &base, &repository, "stable", b"second").await?;
    assert_ne!(first_digest, second_digest);
    let second = resolver.resolve(&tagged, None).await?;
    assert_eq!(second.digest, second_digest);

    let immutable = OciArtifactReference {
        uri: format!("oci://{authority}/{repository}@{first_digest}"),
        expected_digest: Some(first_digest.clone()),
    };
    assert_eq!(
        resolver.resolve(&immutable, None).await?.digest,
        first_digest
    );
    Ok(())
}

#[tokio::test]
async fn real_private_registry_publishes_and_replays_a_validated_oci_graph(
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(registry_url) = std::env::var("A3S_CLOUD_TEST_REGISTRY_URL").ok() else {
        return Ok(());
    };
    let base = Url::parse(&registry_url)?;
    let authority = match (base.host_str(), base.port_or_known_default()) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        _ => return Err("registry test URL must contain a host and port".into()),
    };
    let insecure_hosts = if base.scheme() == "http" {
        vec![authority.clone()]
    } else {
        Vec::new()
    };
    let credential = PublicationCredentialEnv::from_test_environment()?;
    let root = tempfile::tempdir()?;
    let export = root.path().join("export");
    let descriptor = create_publication_export(&export)?;
    let archive = root.path().join("output.tar");
    archive_publication_export(&export, &archive)?;
    let store = Arc::new(LocalNodeArtifactStore::new(
        root.path().join("store"),
        64 * 1024 * 1024,
    )?);
    let artifact = admit_publication_artifact(&store, &archive).await?;
    let validator = Arc::new(RuntimeBuildOutputValidator::new(
        store,
        root.path().join("validation"),
        64 * 1024 * 1024,
        1_024,
        64 * 1024 * 1024,
        64,
        64 * 1024 * 1024,
    )?);
    let recipe = BuildRecipe::dockerfile(
        BuildRecipe::SCHEMA,
        BuildRecipe::DOCKERFILE_KIND,
        ".",
        "Dockerfile",
        None,
        vec!["linux/amd64".into()],
    )?;
    let output = validator.validate(&artifact, &recipe).await?;
    assert_eq!(output.descriptor, descriptor);
    let publisher = OciRegistryArtifactPublisher::new(
        validator,
        Duration::from_secs(10),
        insecure_hosts,
        OciRegistryArtifactPublisherOptions {
            registry: authority.clone(),
            repository_prefix: "a3s-cloud/publication-tests".into(),
            credential_env: credential
                .as_ref()
                .map(PublicationCredentialEnv::name)
                .unwrap_or_default()
                .into(),
            allow_anonymous: credential.is_none(),
        },
    )?;
    let target = OciPublicationTarget::new(
        authority,
        format!("a3s-cloud/publication-tests/{}", Uuid::now_v7().simple()),
        output.descriptor.clone(),
    )?;
    let request = OciPublicationRequest::new(target.clone(), output)?;

    let published = publisher.publish(&request).await?;
    assert_eq!(published.uri, target.uri());
    assert_eq!(publisher.find(&request).await?, Some(published.clone()));
    assert_eq!(publisher.publish(&request).await?, published);
    Ok(())
}

async fn push_fixture(
    client: &Client,
    base: &Url,
    repository: &str,
    tag: &str,
    marker: &[u8],
) -> Result<String, Box<dyn std::error::Error>> {
    let config = [b"{\"marker\":\"".as_slice(), marker, b"\"}".as_slice()].concat();
    let layer = [b"fixture-layer-".as_slice(), marker].concat();
    let config_digest = push_blob(client, base, repository, &config).await?;
    let layer_digest = push_blob(client, base, repository, &layer).await?;
    let manifest = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": OCI_MANIFEST,
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": config_digest,
            "size": config.len()
        },
        "layers": [{
            "mediaType": "application/vnd.oci.image.layer.v1.tar",
            "digest": layer_digest,
            "size": layer.len()
        }]
    }))?;
    let expected_digest = sha256(&manifest);
    let url = base.join(&format!("v2/{repository}/manifests/{tag}"))?;
    let response = client
        .put(url)
        .header(CONTENT_TYPE, OCI_MANIFEST)
        .body(manifest)
        .send()
        .await?;
    if response.status() != StatusCode::CREATED {
        return Err(format!("registry manifest push returned HTTP {}", response.status()).into());
    }
    let stored_digest = response
        .headers()
        .get("docker-content-digest")
        .and_then(|value| value.to_str().ok())
        .ok_or("registry manifest push omitted its digest")?;
    if stored_digest != expected_digest {
        return Err("registry changed the pushed manifest digest".into());
    }
    Ok(expected_digest)
}

async fn push_blob(
    client: &Client,
    base: &Url,
    repository: &str,
    body: &[u8],
) -> Result<String, Box<dyn std::error::Error>> {
    let start = client
        .post(base.join(&format!("v2/{repository}/blobs/uploads/"))?)
        .send()
        .await?;
    if start.status() != StatusCode::ACCEPTED {
        return Err(format!("registry blob upload returned HTTP {}", start.status()).into());
    }
    let location = start
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or("registry blob upload omitted its location")?;
    let mut upload_url = base.join(location)?;
    let digest = sha256(body);
    upload_url.query_pairs_mut().append_pair("digest", &digest);
    let completed = client.put(upload_url).body(body.to_vec()).send().await?;
    if completed.status() != StatusCode::CREATED {
        return Err(format!(
            "registry blob completion returned HTTP {}",
            completed.status()
        )
        .into());
    }
    Ok(digest)
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

struct PublicationCredentialEnv {
    name: String,
}

impl PublicationCredentialEnv {
    fn from_test_environment() -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let Some(username) = std::env::var("A3S_CLOUD_TEST_REGISTRY_USERNAME").ok() else {
            return Ok(None);
        };
        let password = std::env::var("A3S_CLOUD_TEST_REGISTRY_PASSWORD")?;
        let name = "A3S_CLOUD_TEST_PUBLICATION_CREDENTIAL".to_owned();
        std::env::set_var(
            &name,
            serde_json::to_string(&json!({
                "schema": RegistryCredentialMaterial::SCHEMA,
                "username": username,
                "password": password,
            }))?,
        );
        Ok(Some(Self { name }))
    }

    fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for PublicationCredentialEnv {
    fn drop(&mut self) {
        std::env::remove_var(&self.name);
    }
}

async fn admit_publication_artifact(
    store: &Arc<LocalNodeArtifactStore>,
    archive: &Path,
) -> Result<BuildArtifact, Box<dyn std::error::Error>> {
    let bytes = tokio::fs::read(archive).await?;
    let digest = sha256(&bytes);
    let reference = ArtifactRef {
        uri: artifact_uri(&digest)?,
        digest: digest.clone(),
        media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
    };
    let descriptor = NodeArtifactDescriptor::new(reference.clone(), bytes.len() as u64)?;
    store
        .put(&descriptor, Box::pin(tokio::fs::File::open(archive).await?))
        .await?;
    Ok(BuildArtifact::new(
        reference.uri,
        digest,
        reference.media_type,
        bytes.len() as u64,
    )?)
}

fn create_publication_export(
    export: &Path,
) -> Result<a3s_cloud_control_plane::modules::artifacts::OciDescriptor, Box<dyn std::error::Error>>
{
    let layout = export.join("oci");
    let blobs = layout.join("blobs/sha256");
    std::fs::create_dir_all(&blobs)?;
    std::fs::write(
        layout.join("oci-layout"),
        br#"{"imageLayoutVersion":"1.0.0"}"#,
    )?;
    let layer = write_publication_blob(&blobs, OCI_LAYER, b"registry publication fixture\n")?;
    let layer_digest = layer["digest"].as_str().ok_or("layer digest")?;
    let config = write_publication_json_blob(
        &blobs,
        OCI_CONFIG,
        &json!({
            "architecture": "amd64",
            "os": "linux",
            "config": {},
            "rootfs": {"type": "layers", "diff_ids": [layer_digest]},
        }),
    )?;
    let manifest = write_publication_json_blob(
        &blobs,
        OCI_MANIFEST,
        &json!({
            "schemaVersion": 2,
            "mediaType": OCI_MANIFEST,
            "config": config,
            "layers": [layer],
        }),
    )?;
    let mut root = manifest.clone();
    root["platform"] = json!({"architecture": "amd64", "os": "linux"});
    std::fs::write(
        layout.join("index.json"),
        serde_json::to_vec(&json!({
            "schemaVersion": 2,
            "mediaType": OCI_INDEX,
            "manifests": [root],
        }))?,
    )?;
    std::fs::write(
        export.join("buildkit-metadata.json"),
        serde_json::to_vec(&json!({
            "containerimage.digest": manifest["digest"],
            "containerimage.descriptor": manifest,
        }))?,
    )?;
    Ok(
        a3s_cloud_control_plane::modules::artifacts::OciDescriptor::new(
            manifest["mediaType"]
                .as_str()
                .ok_or("manifest media type")?,
            manifest["digest"].as_str().ok_or("manifest digest")?,
            manifest["size"].as_u64().ok_or("manifest size")?,
        )?,
    )
}

fn archive_publication_export(
    export: &Path,
    destination: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = Builder::new(File::create(destination)?);
    for directory in ["oci", "oci/blobs", "oci/blobs/sha256"] {
        builder.append_dir(directory, export.join(directory))?;
    }
    for path in ["buildkit-metadata.json", "oci/oci-layout", "oci/index.json"] {
        builder.append_path_with_name(export.join(path), path)?;
    }
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

fn write_publication_json_blob(
    blobs: &Path,
    media_type: &str,
    value: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    write_publication_blob(blobs, media_type, &serde_json::to_vec(value)?)
}

fn write_publication_blob(
    blobs: &Path,
    media_type: &str,
    bytes: &[u8],
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let digest = sha256(bytes);
    std::fs::write(
        blobs.join(digest.strip_prefix("sha256:").ok_or("digest")?),
        bytes,
    )?;
    Ok(json!({
        "mediaType": media_type,
        "digest": digest,
        "size": bytes.len(),
    }))
}
