use a3s_cloud_control_plane::modules::plugins::domain::entities::PluginRegistry;
use a3s_cloud_control_plane::modules::plugins::domain::services::{
    IPluginRegistryCatalog, IPluginTrustRootStore, PluginRegistryCatalogError,
    PluginTrustRootStoreError, PluginTrustRootWrite,
};
use a3s_cloud_control_plane::modules::plugins::domain::value_objects::{
    PluginRegistryEndpoint, PluginTrustRoot,
};
use a3s_cloud_control_plane::modules::plugins::A3sUsePluginRegistryCatalog;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    OrganizationId, PluginRegistryId, PrincipalId, ResourceName, Sha256Digest,
};
use a3s_use_core::PluginReleaseChannel;
use a3s_use_extension::{
    inspect_bootstrap_root, PluginCatalogHost, PluginCatalogSearch, PluginCatalogSnapshotSource,
    MAX_BOOTSTRAP_ROOT_BYTES, MAX_PLUGIN_CATALOG_PAGE_BYTES,
};
use async_trait::async_trait;
use chrono::Utc;
use reqwest::{Client, StatusCode, Url};
use std::error::Error;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const USE_REVISION: &str = include_str!("../../../tools/use-conformance/use-revision");
const EXPECTED_ROOT: &str = include_str!("../../../tools/use-conformance/plugin-v3-root.sha256");

#[tokio::test]
#[ignore = "requires public HTTPS access to the Registry fixture at the pinned A3S Use revision"]
async fn real_public_https_use_registry_refreshes_and_replays_bounded_catalog_reads(
) -> Result<(), Box<dyn Error>> {
    let revision = USE_REVISION.trim();
    if revision.len() != 40
        || !revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(test_error("the pinned A3S Use revision is invalid").into());
    }
    let expected_root = EXPECTED_ROOT.trim();
    let expected_root_hex = expected_root
        .strip_prefix("sha256:")
        .ok_or_else(|| test_error("the pinned Registry root digest is invalid"))?;
    let endpoint = Url::parse(&format!(
        "https://raw.githubusercontent.com/A3S-Lab/Use/{revision}/crates/extension/fixtures/registry/plugin-v3/"
    ))?;
    let client = Client::builder()
        .use_rustls_tls()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()?;
    let root_bytes = fetch_bounded(
        &client,
        endpoint.join("metadata/root.json")?,
        MAX_BOOTSTRAP_ROOT_BYTES,
    )
    .await?;
    let root_evidence = inspect_bootstrap_root(&root_bytes)?;
    if root_evidence.root_sha256 != expected_root_hex || root_evidence.root_version != 1 {
        return Err(test_error("the remote bootstrap root drifted from pinned evidence").into());
    }

    let temporary = tempfile::tempdir()?;
    let trust_root = PluginTrustRoot::from_digest(
        Sha256Digest::parse(expected_root.to_owned()).map_err(test_error)?,
        root_evidence.root_version,
    )
    .map_err(test_error)?;
    let root_store = Arc::new(PinnedRootStore {
        root_sha256: expected_root.to_owned(),
        bytes: Arc::new(root_bytes),
    });
    let organization_id = OrganizationId::new();
    let registry_id = PluginRegistryId::new();
    let registry = enrolled_registry(
        organization_id,
        registry_id,
        "Pinned Use fixture",
        endpoint.as_str(),
        trust_root.clone(),
    )?;
    let metadata_root = temporary.path().join("metadata");
    let catalog = A3sUsePluginRegistryCatalog::new(root_store, metadata_root.clone())?;

    let metadata = catalog.refresh(&registry).await?;
    assert_eq!(metadata.root_sha256, expected_root_hex);
    assert_eq!(metadata.root_version, 1);
    assert_eq!(metadata.timestamp_version, 7);
    assert_eq!(metadata.snapshot_version, 7);
    assert_eq!(metadata.targets_version, 7);
    assert_eq!(metadata.package_targets, 1);

    let host = PluginCatalogHost::new("linux-x86_64", "0.3.0")?;
    let search = PluginCatalogSearch {
        query: "literature".into(),
        kind: None,
        channel: Some(PluginReleaseChannel::Stable),
        publisher: Some("acme".into()),
        category: None,
        availability: None,
        cursor: None,
        limit: 1,
    };
    let online = catalog.search(&registry, &host, &search).await?;
    assert_eq!(
        online.snapshot.source,
        PluginCatalogSnapshotSource::Refreshed
    );
    assert_eq!(online.total_matches, 1);
    assert_eq!(online.plugins.len(), 1);
    assert_eq!(online.plugins[0].record.package_id, "acme/research");
    assert!(online.next_cursor.is_none());
    assert!(serde_json::to_vec(&online)?.len() <= MAX_PLUGIN_CATALOG_PAGE_BYTES);

    let cached = catalog.search_cached(&registry, &host, &search).await?;
    assert_eq!(cached.snapshot.source, PluginCatalogSnapshotSource::Cached);
    assert_eq!(
        cached.snapshot.snapshot_digest,
        online.snapshot.snapshot_digest
    );
    assert_eq!(cached.plugins, online.plugins);
    assert!(serde_json::to_vec(&cached)?.len() <= MAX_PLUGIN_CATALOG_PAGE_BYTES);

    let online_inspection = catalog
        .inspect(
            &registry,
            &host,
            "acme/research",
            Some("2.0.0"),
            Some(PluginReleaseChannel::Stable),
        )
        .await?;
    let cached_inspection = catalog
        .inspect_cached(
            &registry,
            &host,
            "acme/research",
            Some("2.0.0"),
            Some(PluginReleaseChannel::Stable),
        )
        .await?;
    assert_eq!(
        cached_inspection.snapshot.source,
        PluginCatalogSnapshotSource::Cached
    );
    assert_eq!(cached_inspection.plugin, online_inspection.plugin);
    assert_eq!(
        cached_inspection.snapshot.snapshot_digest,
        online_inspection.snapshot.snapshot_digest
    );

    let package_target = endpoint.join(&format!(
        "targets/{}",
        online.plugins[0].record.archive.target_name
    ))?;
    assert_eq!(
        client.head(package_target).send().await?.status(),
        StatusCode::NOT_FOUND,
        "the metadata-only fixture must not provide a package body"
    );

    let mut invalid_cursor = search.clone();
    invalid_cursor.cursor = Some("x".repeat(513));
    assert!(matches!(
        catalog
            .search_cached(&registry, &host, &invalid_cursor)
            .await,
        Err(PluginRegistryCatalogError::QueryInvalid)
    ));

    let wrong_version = enrolled_registry(
        organization_id,
        PluginRegistryId::new(),
        "Root drift",
        endpoint.as_str(),
        PluginTrustRoot::from_digest(
            Sha256Digest::parse(expected_root.to_owned()).map_err(test_error)?,
            root_evidence.root_version + 1,
        )
        .map_err(test_error)?,
    )?;
    assert!(matches!(
        catalog.refresh(&wrong_version).await,
        Err(PluginRegistryCatalogError::TrustRootEvidenceMismatch)
    ));

    let loopback = enrolled_registry(
        organization_id,
        PluginRegistryId::new(),
        "Loopback denied",
        "https://127.0.0.1:9/registry/",
        trust_root,
    )?;
    assert!(matches!(
        catalog.refresh(&loopback).await,
        Err(PluginRegistryCatalogError::Use { code })
            if code == "use.extension.registry_untrusted"
    ));

    tamper_cached_targets(
        &metadata_root,
        organization_id,
        registry_id,
        expected_root_hex,
    )
    .await?;
    assert!(matches!(
        catalog.search_cached(&registry, &host, &search).await,
        Err(PluginRegistryCatalogError::Use { code })
            if code == "use.extension.catalog_cache_changed"
    ));

    println!(
        "\nA3S_CLOUD_U0_PROVIDER_CERTIFIED revision={revision} root={expected_root} timestamp_version={} snapshot_version={} targets_version={} package=acme/research checks=9/9",
        metadata.timestamp_version, metadata.snapshot_version, metadata.targets_version
    );
    Ok(())
}

async fn fetch_bounded(
    client: &Client,
    url: Url,
    maximum_bytes: u64,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut response = client.get(url).send().await?;
    if response.status() != StatusCode::OK {
        return Err(test_error(format!(
            "bootstrap root request returned HTTP {}",
            response.status()
        ))
        .into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        let next_size = bytes
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| test_error("bootstrap root byte length overflowed"))?;
        if next_size as u64 > maximum_bytes {
            return Err(test_error("bootstrap root exceeded the A3S Use bound").into());
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return Err(test_error("bootstrap root response was empty").into());
    }
    Ok(bytes)
}

fn enrolled_registry(
    organization_id: OrganizationId,
    registry_id: PluginRegistryId,
    name: &str,
    endpoint: &str,
    trust_root: PluginTrustRoot,
) -> Result<PluginRegistry, Box<dyn Error>> {
    PluginRegistry::enroll(
        organization_id,
        registry_id,
        ResourceName::parse(name).map_err(test_error)?,
        PluginRegistryEndpoint::parse(endpoint).map_err(test_error)?,
        trust_root,
        PrincipalId::new(),
        Uuid::now_v7(),
        Utc::now(),
    )
    .map_err(|error| test_error(error).into())
}

async fn tamper_cached_targets(
    metadata_root: &Path,
    organization_id: OrganizationId,
    registry_id: PluginRegistryId,
    root_sha256: &str,
) -> Result<(), Box<dyn Error>> {
    let path = metadata_root
        .join("organizations")
        .join(organization_id.to_string())
        .join("registries")
        .join(registry_id.to_string())
        .join(root_sha256)
        .join("catalog-metadata")
        .join("targets.json");
    let mut bytes = tokio::fs::read(&path).await?;
    let position = bytes
        .windows(b"acme/research".len())
        .position(|window| window == b"acme/research")
        .ok_or_else(|| test_error("cached targets metadata did not contain the fixture package"))?;
    bytes[position] = b'X';
    tokio::fs::write(path, bytes).await?;
    Ok(())
}

fn test_error(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Read-only test port for one already-admitted root. It adds no storage,
/// parsing, or trust behavior; the production adapter still delegates all TUF
/// verification and metadata caching to A3S Use.
#[derive(Clone)]
struct PinnedRootStore {
    root_sha256: String,
    bytes: Arc<Vec<u8>>,
}

#[async_trait]
impl IPluginTrustRootStore for PinnedRootStore {
    async fn put(
        &self,
        _root: &PluginTrustRoot,
        _bytes: Vec<u8>,
    ) -> Result<PluginTrustRootWrite, PluginTrustRootStoreError> {
        Err(PluginTrustRootStoreError::Invalid(
            "the provider fixture trust-root port is read-only".into(),
        ))
    }

    async fn get(&self, root: &PluginTrustRoot) -> Result<Vec<u8>, PluginTrustRootStoreError> {
        if root.digest().as_str() != self.root_sha256 {
            return Err(PluginTrustRootStoreError::NotFound);
        }
        Ok(self.bytes.as_ref().clone())
    }
}
