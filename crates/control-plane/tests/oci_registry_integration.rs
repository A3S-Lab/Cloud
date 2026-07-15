use a3s_cloud_control_plane::modules::workloads::{
    IOciArtifactResolver, OciArtifactReference, OciRegistryArtifactResolver,
};
use reqwest::header::{CONTENT_TYPE, LOCATION};
use reqwest::{Client, StatusCode, Url};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::Duration;
use uuid::Uuid;

const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";

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

    let first = resolver.resolve(&tagged).await?;
    assert_eq!(first.digest, first_digest);
    let second_digest = push_fixture(&client, &base, &repository, "stable", b"second").await?;
    assert_ne!(first_digest, second_digest);
    let second = resolver.resolve(&tagged).await?;
    assert_eq!(second.digest, second_digest);

    let immutable = OciArtifactReference {
        uri: format!("oci://{authority}/{repository}@{first_digest}"),
        expected_digest: Some(first_digest.clone()),
    };
    assert_eq!(resolver.resolve(&immutable).await?.digest, first_digest);
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
