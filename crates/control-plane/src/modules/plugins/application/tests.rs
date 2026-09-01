use super::{
    EnrollPluginRegistry, EnrollPluginRegistryHandler, GetPluginRegistry, GetPluginRegistryHandler,
    InspectCachedPluginCatalog, InspectCachedPluginCatalogHandler, InspectPluginCatalog,
    InspectPluginCatalogHandler, ListPluginRegistries, ListPluginRegistriesHandler,
    SearchCachedPluginCatalog, SearchCachedPluginCatalogHandler, SearchPluginCatalog,
    SearchPluginCatalogHandler,
};
use crate::modules::plugins::domain::entities::PluginRegistry;
use crate::modules::plugins::domain::repositories::IPluginRegistryRepository;
use crate::modules::plugins::domain::services::{
    IPluginRegistryCatalog, IPluginRegistryEnrollmentAuthorizer, IPluginTrustRootStore,
    PluginRegistryCatalogError, PluginRegistryEnrollmentAuthorization,
    PluginRegistryEnrollmentAuthorizationError, PluginTrustRootStoreError, PluginTrustRootWrite,
};
use crate::modules::plugins::domain::value_objects::PluginTrustRoot;
use crate::modules::plugins::test_support::VALID_BOOTSTRAP_ROOT;
use crate::modules::plugins::{InMemoryPluginRegistryRepository, PluginTrustRootObjectStore};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, Sha256Digest};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use a3s_use_core::PluginReleaseChannel;
use a3s_use_extension::{
    inspect_bootstrap_root, PluginCatalogHost, PluginCatalogInspection, PluginCatalogPage,
    PluginCatalogSearch, PluginCatalogSnapshot, PluginCatalogSnapshotSource,
    VerifiedRegistryMetadata, MAX_BOOTSTRAP_ROOT_BYTES,
};
use async_trait::async_trait;
use chrono::Utc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Copy)]
enum AuthorizationOutcome {
    Allow,
    Forbid,
    Unavailable,
}

struct FixedEnrollmentAuthorizer {
    outcome: AuthorizationOutcome,
    calls: AtomicUsize,
}

impl FixedEnrollmentAuthorizer {
    fn new(outcome: AuthorizationOutcome) -> Self {
        Self {
            outcome,
            calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl IPluginRegistryEnrollmentAuthorizer for FixedEnrollmentAuthorizer {
    async fn authorize_enrollment(
        &self,
        organization_id: OrganizationId,
        actor_id: PrincipalId,
    ) -> Result<PluginRegistryEnrollmentAuthorization, PluginRegistryEnrollmentAuthorizationError>
    {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.outcome {
            AuthorizationOutcome::Allow => {
                PluginRegistryEnrollmentAuthorization::new(organization_id, actor_id)
                    .map_err(PluginRegistryEnrollmentAuthorizationError::Unavailable)
            }
            AuthorizationOutcome::Forbid => {
                Err(PluginRegistryEnrollmentAuthorizationError::Forbidden)
            }
            AuthorizationOutcome::Unavailable => Err(
                PluginRegistryEnrollmentAuthorizationError::Unavailable("fixture".into()),
            ),
        }
    }
}

struct IntegrityFailingTrustRootStore;

#[async_trait]
impl IPluginTrustRootStore for IntegrityFailingTrustRootStore {
    async fn put(
        &self,
        _root: &PluginTrustRoot,
        _bytes: Vec<u8>,
    ) -> Result<PluginTrustRootWrite, PluginTrustRootStoreError> {
        Err(PluginTrustRootStoreError::Integrity(
            "fixture corruption".into(),
        ))
    }

    async fn get(&self, _root: &PluginTrustRoot) -> Result<Vec<u8>, PluginTrustRootStoreError> {
        Err(PluginTrustRootStoreError::NotFound)
    }
}

#[derive(Default)]
struct RecordingPluginRegistryCatalog {
    search_calls: AtomicUsize,
    cached_search_calls: AtomicUsize,
    inspect_calls: AtomicUsize,
    cached_inspect_calls: AtomicUsize,
}

#[async_trait]
impl IPluginRegistryCatalog for RecordingPluginRegistryCatalog {
    async fn refresh(
        &self,
        _registry: &PluginRegistry,
    ) -> Result<VerifiedRegistryMetadata, PluginRegistryCatalogError> {
        Err(PluginRegistryCatalogError::Use {
            code: "fixture.refresh_not_expected".into(),
        })
    }

    async fn search(
        &self,
        _registry: &PluginRegistry,
        _host: &PluginCatalogHost,
        _search: &PluginCatalogSearch,
    ) -> Result<PluginCatalogPage, PluginRegistryCatalogError> {
        self.search_calls.fetch_add(1, Ordering::SeqCst);
        Ok(empty_catalog_page(PluginCatalogSnapshotSource::Refreshed))
    }

    async fn search_cached(
        &self,
        _registry: &PluginRegistry,
        _host: &PluginCatalogHost,
        _search: &PluginCatalogSearch,
    ) -> Result<PluginCatalogPage, PluginRegistryCatalogError> {
        self.cached_search_calls.fetch_add(1, Ordering::SeqCst);
        Ok(empty_catalog_page(PluginCatalogSnapshotSource::Cached))
    }

    async fn inspect(
        &self,
        _registry: &PluginRegistry,
        _host: &PluginCatalogHost,
        _package_id: &str,
        _version: Option<&str>,
        _channel: Option<PluginReleaseChannel>,
    ) -> Result<PluginCatalogInspection, PluginRegistryCatalogError> {
        self.inspect_calls.fetch_add(1, Ordering::SeqCst);
        Err(PluginRegistryCatalogError::PackageNotFound)
    }

    async fn inspect_cached(
        &self,
        _registry: &PluginRegistry,
        _host: &PluginCatalogHost,
        _package_id: &str,
        _version: Option<&str>,
        _channel: Option<PluginReleaseChannel>,
    ) -> Result<PluginCatalogInspection, PluginRegistryCatalogError> {
        self.cached_inspect_calls.fetch_add(1, Ordering::SeqCst);
        Err(PluginRegistryCatalogError::PackageIncompatible)
    }
}

fn empty_catalog_page(source: PluginCatalogSnapshotSource) -> PluginCatalogPage {
    PluginCatalogPage {
        snapshot: PluginCatalogSnapshot {
            metadata: VerifiedRegistryMetadata {
                registry_name: "fixture".into(),
                registry_url: "https://registry.example/plugins".into(),
                root_sha256: "0".repeat(64),
                root_version: 1,
                timestamp_version: 1,
                snapshot_version: 1,
                targets_version: 1,
                package_targets: 0,
            },
            source,
            host_target: "x86_64-pc-windows-gnu".into(),
            use_version: "0.3.0".into(),
            catalog_records: 0,
            verified_at_unix_seconds: 1,
            age_seconds: 0,
            snapshot_digest: format!("sha256:{}", "1".repeat(64)),
        },
        total_matches: 0,
        plugins: Vec::new(),
        next_cursor: None,
    }
}

fn catalog_host() -> PluginCatalogHost {
    PluginCatalogHost::new("x86_64-pc-windows-gnu", "0.3.0").expect("catalog host")
}

fn catalog_search() -> PluginCatalogSearch {
    PluginCatalogSearch {
        query: String::new(),
        kind: None,
        channel: None,
        publisher: None,
        category: None,
        availability: None,
        cursor: None,
        limit: 20,
    }
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

fn command(
    organization_id: OrganizationId,
    actor_id: PrincipalId,
    bootstrap_root: Vec<u8>,
    idempotency_key: &str,
) -> EnrollPluginRegistry {
    EnrollPluginRegistry {
        organization_id,
        actor_id,
        name: "Official".into(),
        endpoint: "https://registry.example/plugins".into(),
        bootstrap_root,
        idempotency_key: idempotency_key.into(),
        request_id: Uuid::now_v7(),
        requested_at: Utc::now(),
    }
}

fn trust_root(bytes: &[u8]) -> PluginTrustRoot {
    let evidence = inspect_bootstrap_root(bytes).expect("bootstrap root evidence");
    let digest = Sha256Digest::parse(format!("sha256:{}", evidence.root_sha256))
        .expect("bootstrap root digest");
    PluginTrustRoot::from_digest(digest, evidence.root_version).expect("plugin trust root")
}

fn root_store() -> Arc<PluginTrustRootObjectStore> {
    Arc::new(
        PluginTrustRootObjectStore::in_memory(MAX_BOOTSTRAP_ROOT_BYTES)
            .expect("plugin trust-root store"),
    )
}

#[tokio::test]
async fn enrollment_validates_stores_commits_and_replays_before_tenant_queries() {
    let organization_id = OrganizationId::new();
    let actor_id = PrincipalId::new();
    let authorizer = Arc::new(FixedEnrollmentAuthorizer::new(AuthorizationOutcome::Allow));
    let roots = root_store();
    let registries = Arc::new(InMemoryPluginRegistryRepository::new());
    let handler =
        EnrollPluginRegistryHandler::new(authorizer.clone(), roots.clone(), registries.clone());

    let created = handler
        .execute(
            command(
                organization_id,
                actor_id,
                VALID_BOOTSTRAP_ROOT.to_vec(),
                "enroll-official",
            ),
            context(),
        )
        .await
        .expect("enrollment dispatch")
        .expect("registry enrollment");
    let replayed = handler
        .execute(
            command(
                organization_id,
                actor_id,
                VALID_BOOTSTRAP_ROOT.to_vec(),
                "enroll-official",
            ),
            context(),
        )
        .await
        .expect("replay dispatch")
        .expect("registry replay");

    assert!(!created.replayed);
    assert!(replayed.replayed);
    assert_eq!(created.registry, replayed.registry);
    assert_eq!(created.registry.trust_root.version(), 1);
    assert_eq!(authorizer.calls(), 2);
    assert_eq!(
        roots
            .get(&created.registry.trust_root)
            .await
            .expect("stored bootstrap root"),
        VALID_BOOTSTRAP_ROOT
    );
    assert_eq!(registries.outbox_events().await.len(), 1);

    let found = GetPluginRegistryHandler::new(registries.clone())
        .execute(
            GetPluginRegistry {
                organization_id,
                registry_id: created.registry.id,
            },
            context(),
        )
        .await
        .expect("get dispatch")
        .expect("get registry");
    assert_eq!(found, created.registry);
    let listed = ListPluginRegistriesHandler::new(registries.clone())
        .execute(ListPluginRegistries { organization_id }, context())
        .await
        .expect("list dispatch")
        .expect("list registries");
    assert_eq!(listed, vec![created.registry.clone()]);
    let foreign = GetPluginRegistryHandler::new(registries.clone())
        .execute(
            GetPluginRegistry {
                organization_id: OrganizationId::new(),
                registry_id: created.registry.id,
            },
            context(),
        )
        .await
        .expect("foreign get dispatch")
        .expect_err("foreign registry lookup");
    assert!(matches!(foreign, ApplicationError::NotFound(_)));
    let foreign_list = ListPluginRegistriesHandler::new(registries)
        .execute(
            ListPluginRegistries {
                organization_id: OrganizationId::new(),
            },
            context(),
        )
        .await
        .expect("foreign list dispatch")
        .expect("foreign registry list");
    assert!(foreign_list.is_empty());
}

#[tokio::test]
async fn authorization_fails_before_root_storage_and_registry_persistence() {
    let organization_id = OrganizationId::new();
    let actor_id = PrincipalId::new();
    let authorizer = Arc::new(FixedEnrollmentAuthorizer::new(AuthorizationOutcome::Forbid));
    let roots = root_store();
    let registries = Arc::new(InMemoryPluginRegistryRepository::new());
    let handler =
        EnrollPluginRegistryHandler::new(authorizer.clone(), roots.clone(), registries.clone());

    let error = handler
        .execute(
            command(
                organization_id,
                actor_id,
                VALID_BOOTSTRAP_ROOT.to_vec(),
                "forbidden",
            ),
            context(),
        )
        .await
        .expect("enrollment dispatch")
        .expect_err("forbidden enrollment");

    assert!(matches!(error, ApplicationError::Forbidden(_)));
    assert_eq!(authorizer.calls(), 1);
    assert!(matches!(
        roots.get(&trust_root(VALID_BOOTSTRAP_ROOT)).await,
        Err(PluginTrustRootStoreError::NotFound)
    ));
    assert!(registries
        .list(organization_id)
        .await
        .expect("registry list")
        .is_empty());
}

#[tokio::test]
async fn malformed_root_and_authorization_outage_fail_without_durable_intent() {
    let organization_id = OrganizationId::new();
    let actor_id = PrincipalId::new();
    let roots = root_store();
    let registries = Arc::new(InMemoryPluginRegistryRepository::new());
    let allowed = Arc::new(FixedEnrollmentAuthorizer::new(AuthorizationOutcome::Allow));
    let handler =
        EnrollPluginRegistryHandler::new(allowed.clone(), roots.clone(), registries.clone());
    let malformed = br#"{"signed":{}}"#.to_vec();

    let invalid = handler
        .execute(
            command(organization_id, actor_id, malformed, "malformed"),
            context(),
        )
        .await
        .expect("malformed dispatch")
        .expect_err("malformed enrollment");
    assert!(matches!(invalid, ApplicationError::Invalid(_)));
    assert_eq!(allowed.calls(), 1);
    assert!(registries
        .list(organization_id)
        .await
        .expect("registry list")
        .is_empty());

    let unavailable = Arc::new(FixedEnrollmentAuthorizer::new(
        AuthorizationOutcome::Unavailable,
    ));
    let handler = EnrollPluginRegistryHandler::new(unavailable.clone(), roots, registries.clone());
    let error = handler
        .execute(
            command(
                organization_id,
                actor_id,
                VALID_BOOTSTRAP_ROOT.to_vec(),
                "unavailable",
            ),
            context(),
        )
        .await
        .expect("unavailable dispatch")
        .expect_err("authorization outage");
    assert!(matches!(error, ApplicationError::Unavailable(_)));
    assert_eq!(unavailable.calls(), 1);
    assert!(registries
        .list(organization_id)
        .await
        .expect("registry list")
        .is_empty());
}

#[tokio::test]
async fn trust_root_integrity_failure_is_unavailable_and_never_commits_registry_intent() {
    let organization_id = OrganizationId::new();
    let actor_id = PrincipalId::new();
    let authorizer = Arc::new(FixedEnrollmentAuthorizer::new(AuthorizationOutcome::Allow));
    let registries = Arc::new(InMemoryPluginRegistryRepository::new());
    let handler = EnrollPluginRegistryHandler::new(
        authorizer,
        Arc::new(IntegrityFailingTrustRootStore),
        registries.clone(),
    );

    let error = handler
        .execute(
            command(
                organization_id,
                actor_id,
                VALID_BOOTSTRAP_ROOT.to_vec(),
                "integrity-failure",
            ),
            context(),
        )
        .await
        .expect("integrity dispatch")
        .expect_err("trust-root integrity failure");

    assert!(matches!(error, ApplicationError::Unavailable(_)));
    assert!(registries
        .list(organization_id)
        .await
        .expect("registry list")
        .is_empty());
}

#[tokio::test]
async fn changed_root_under_one_idempotency_key_conflicts_after_safe_admission() {
    let organization_id = OrganizationId::new();
    let actor_id = PrincipalId::new();
    let authorizer = Arc::new(FixedEnrollmentAuthorizer::new(AuthorizationOutcome::Allow));
    let roots = root_store();
    let registries = Arc::new(InMemoryPluginRegistryRepository::new());
    let handler = EnrollPluginRegistryHandler::new(authorizer, roots, registries.clone());

    handler
        .execute(
            command(
                organization_id,
                actor_id,
                VALID_BOOTSTRAP_ROOT.to_vec(),
                "same-key",
            ),
            context(),
        )
        .await
        .expect("first dispatch")
        .expect("first enrollment");
    let mut changed_root = b"\n".to_vec();
    changed_root.extend_from_slice(VALID_BOOTSTRAP_ROOT);
    let error = handler
        .execute(
            command(organization_id, actor_id, changed_root, "same-key"),
            context(),
        )
        .await
        .expect("changed dispatch")
        .expect_err("changed idempotency input");

    assert!(matches!(error, ApplicationError::Conflict(_)));
    assert_eq!(
        registries
            .list(organization_id)
            .await
            .expect("registry list")
            .len(),
        1
    );
}

#[tokio::test]
async fn catalog_queries_preserve_use_types_online_and_cached_with_tenant_fence() {
    let organization_id = OrganizationId::new();
    let actor_id = PrincipalId::new();
    let registries = Arc::new(InMemoryPluginRegistryRepository::new());
    let enrollment = EnrollPluginRegistryHandler::new(
        Arc::new(FixedEnrollmentAuthorizer::new(AuthorizationOutcome::Allow)),
        root_store(),
        registries.clone(),
    )
    .execute(
        command(
            organization_id,
            actor_id,
            VALID_BOOTSTRAP_ROOT.to_vec(),
            "catalog-registry",
        ),
        context(),
    )
    .await
    .expect("enrollment dispatch")
    .expect("registry enrollment");
    let catalog = Arc::new(RecordingPluginRegistryCatalog::default());

    let online = SearchPluginCatalogHandler::new(registries.clone(), catalog.clone())
        .execute(
            SearchPluginCatalog {
                organization_id,
                registry_id: enrollment.registry.id,
                host: catalog_host(),
                search: catalog_search(),
            },
            context(),
        )
        .await
        .expect("online search dispatch")
        .expect("online catalog search");
    let cached = SearchCachedPluginCatalogHandler::new(registries.clone(), catalog.clone())
        .execute(
            SearchCachedPluginCatalog {
                organization_id,
                registry_id: enrollment.registry.id,
                host: catalog_host(),
                search: catalog_search(),
            },
            context(),
        )
        .await
        .expect("cached search dispatch")
        .expect("cached catalog search");

    assert_eq!(
        online.snapshot.source,
        PluginCatalogSnapshotSource::Refreshed
    );
    assert_eq!(cached.snapshot.source, PluginCatalogSnapshotSource::Cached);
    assert_eq!(catalog.search_calls.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.cached_search_calls.load(Ordering::SeqCst), 1);

    let foreign = SearchPluginCatalogHandler::new(registries.clone(), catalog.clone())
        .execute(
            SearchPluginCatalog {
                organization_id: OrganizationId::new(),
                registry_id: enrollment.registry.id,
                host: catalog_host(),
                search: catalog_search(),
            },
            context(),
        )
        .await
        .expect("foreign search dispatch")
        .expect_err("foreign registry search");
    assert!(matches!(foreign, ApplicationError::NotFound(_)));
    assert_eq!(catalog.search_calls.load(Ordering::SeqCst), 1);

    let missing = InspectPluginCatalogHandler::new(registries.clone(), catalog.clone())
        .execute(
            InspectPluginCatalog {
                organization_id,
                registry_id: enrollment.registry.id,
                host: catalog_host(),
                package_id: "a3s/example".into(),
                version: Some("1.0.0".into()),
                channel: Some(PluginReleaseChannel::Stable),
            },
            context(),
        )
        .await
        .expect("online inspection dispatch")
        .expect_err("missing package");
    assert!(matches!(missing, ApplicationError::NotFound(_)));

    let incompatible = InspectCachedPluginCatalogHandler::new(registries, catalog.clone())
        .execute(
            InspectCachedPluginCatalog {
                organization_id,
                registry_id: enrollment.registry.id,
                host: catalog_host(),
                package_id: "a3s/example".into(),
                version: None,
                channel: None,
            },
            context(),
        )
        .await
        .expect("cached inspection dispatch")
        .expect_err("incompatible package");
    assert!(matches!(incompatible, ApplicationError::Conflict(_)));
    assert_eq!(catalog.inspect_calls.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.cached_inspect_calls.load(Ordering::SeqCst), 1);
}
