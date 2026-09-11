use super::{
    ConfirmPluginPlanProjection, ConfirmPluginPlanProjectionHandler, EnrollPluginRegistry,
    EnrollPluginRegistryHandler, GetPluginAssignment, GetPluginAssignmentHandler,
    GetPluginPlanProjection, GetPluginPlanProjectionHandler, GetPluginRegistry,
    GetPluginRegistryHandler, InspectCachedPluginCatalog, InspectCachedPluginCatalogHandler,
    InspectPluginCatalog, InspectPluginCatalogHandler, ListPluginAssignments,
    ListPluginAssignmentsHandler, ListPluginRegistries, ListPluginRegistriesHandler,
    PLUGIN_ASSIGNMENT_WORKFLOW_NAME, PLUGIN_ASSIGNMENT_WORKFLOW_VERSION, PluginAccess,
    PluginAssignmentReconciler, RecordPluginPlanProjection, RecordPluginPlanProjectionHandler,
    SearchCachedPluginCatalog, SearchCachedPluginCatalogHandler, SearchPluginCatalog,
    SearchPluginCatalogHandler, SetPluginAssignment, SetPluginAssignmentHandler,
};
use crate::modules::plugins::domain::entities::PluginRegistry;
use crate::modules::plugins::domain::repositories::IPluginAssignmentRepository;
use crate::modules::plugins::domain::repositories::IPluginRegistryRepository;
use crate::modules::plugins::domain::services::{
    IPluginRegistryCatalog, IPluginRegistryEnrollmentAuthorizer, IPluginTrustRootStore,
    PluginRegistryCatalogError, PluginRegistryEnrollmentAuthorization,
    PluginRegistryEnrollmentAuthorizationError, PluginTrustRootStoreError, PluginTrustRootWrite,
};
use crate::modules::plugins::domain::value_objects::PluginCatalogSelection;
use crate::modules::plugins::domain::value_objects::PluginTrustRoot;
use crate::modules::plugins::test_support::VALID_BOOTSTRAP_ROOT;
use crate::modules::plugins::{
    InMemoryPluginAssignmentRepository, InMemoryPluginPlanProjectionRepository,
    InMemoryPluginRegistryRepository, PluginTrustRootObjectStore,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeId, OperationId, OrganizationId, PluginPlanProjectionId, PluginRegistryId,
    PrincipalId, ProjectId, Sha256Digest,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use a3s_use_core::{
    PLUGIN_MANAGED_SCOPE_SCHEMA_V2, PlanScopeKind, PluginDesiredState, PluginManagedScope,
    PluginPackageId, PluginReleaseChannel, PluginSurfaceKind, PluginSurfaceRef,
};
use a3s_use_extension::{
    MAX_BOOTSTRAP_ROOT_BYTES, PluginCatalogHost, PluginCatalogInspection, PluginCatalogPage,
    PluginCatalogSearch, PluginCatalogSnapshot, PluginCatalogSnapshotSource,
    VerifiedRegistryMetadata, inspect_bootstrap_root,
};
use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
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
    assert!(
        registries
            .list(organization_id)
            .await
            .expect("registry list")
            .is_empty()
    );
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
    assert!(
        registries
            .list(organization_id)
            .await
            .expect("registry list")
            .is_empty()
    );

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
    assert!(
        registries
            .list(organization_id)
            .await
            .expect("registry list")
            .is_empty()
    );
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
    assert!(
        registries
            .list(organization_id)
            .await
            .expect("registry list")
            .is_empty()
    );
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

fn digest(byte: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
}

fn assignment_scope() -> PluginManagedScope {
    PluginManagedScope {
        schema: PLUGIN_MANAGED_SCOPE_SCHEMA_V2.into(),
        host_id: "host:node-01".into(),
        scope_kind: PlanScopeKind::Workspace,
        scope_id: "workspace:research".into(),
        authority_id: "cloud:organization-01".into(),
        fence_generation: 7,
        fence_digest: digest('d').as_str().into(),
    }
}

fn assignment_selection(version: &str) -> PluginCatalogSelection {
    PluginCatalogSelection {
        package_id: PluginPackageId::parse("a3s/registry-selftest").expect("package"),
        catalog_record_digest: digest('a'),
        version: version.into(),
        package_digest: digest('b'),
        manifest_digest: digest('c'),
        selected_surfaces: vec![PluginSurfaceRef {
            kind: PluginSurfaceKind::Skill,
            id: "selftest".into(),
        }],
    }
}

fn set_assignment_command(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    registry_id: PluginRegistryId,
    host_id: NodeId,
    selection: PluginCatalogSelection,
    desired_state: PluginDesiredState,
    actor_id: PrincipalId,
    idempotency_key: &str,
) -> SetPluginAssignment {
    SetPluginAssignment {
        organization_id,
        project_id,
        environment_id,
        registry_id,
        target_host_id: host_id,
        workspace_scope: assignment_scope(),
        selection,
        policy_digest: digest('e'),
        desired_state,
        actor_id,
        idempotency_key: idempotency_key.into(),
        request_id: Uuid::now_v7(),
        requested_at: Utc::now(),
    }
}

#[tokio::test]
async fn set_plugin_assignment_creates_replays_and_revises_desired_state() {
    let assignments = Arc::new(InMemoryPluginAssignmentRepository::new());
    let handler = SetPluginAssignmentHandler::new(assignments.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let registry_id = PluginRegistryId::new();
    let host_id = NodeId::new();
    let actor_id = PrincipalId::new();

    let created = handler
        .execute(
            set_assignment_command(
                organization_id,
                project_id,
                environment_id,
                registry_id,
                host_id,
                assignment_selection("0.1.0"),
                PluginDesiredState::Enabled,
                actor_id,
                "assign-1",
            ),
            context(),
        )
        .await
        .expect("create dispatch")
        .expect("create");
    assert!(!created.replayed);
    assert_eq!(created.assignment.assignment_generation, 1);
    assert_eq!(
        created.assignment.desired_state,
        PluginDesiredState::Enabled
    );

    let replayed = handler
        .execute(
            set_assignment_command(
                organization_id,
                project_id,
                environment_id,
                registry_id,
                host_id,
                assignment_selection("0.1.0"),
                PluginDesiredState::Enabled,
                actor_id,
                "assign-1",
            ),
            context(),
        )
        .await
        .expect("replay dispatch")
        .expect("replay");
    assert!(replayed.replayed);
    assert_eq!(replayed.assignment.id, created.assignment.id);

    let revised = handler
        .execute(
            set_assignment_command(
                organization_id,
                project_id,
                environment_id,
                registry_id,
                host_id,
                assignment_selection("0.2.0"),
                PluginDesiredState::InstalledDisabled,
                actor_id,
                "assign-2",
            ),
            context(),
        )
        .await
        .expect("revise dispatch")
        .expect("revise");
    assert!(!revised.replayed);
    assert_eq!(revised.assignment.id, created.assignment.id);
    assert_eq!(revised.assignment.assignment_generation, 2);
    assert_eq!(
        revised.assignment.desired_state,
        PluginDesiredState::InstalledDisabled
    );
    assert_eq!(revised.assignment.selection.version, "0.2.0");
    assert_eq!(
        assignments
            .list_for_environment(organization_id, environment_id)
            .await
            .expect("list")
            .len(),
        1
    );
}

#[tokio::test]
async fn list_and_get_plugin_assignment_queries_return_environment_scoped_rows() {
    let assignments = Arc::new(InMemoryPluginAssignmentRepository::new());
    let set_handler = SetPluginAssignmentHandler::new(assignments.clone());
    let list_handler = ListPluginAssignmentsHandler::new(assignments.clone());
    let get_handler = GetPluginAssignmentHandler::new(assignments.clone());
    let organization_id = OrganizationId::new();
    let other_environment_id = EnvironmentId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let registry_id = PluginRegistryId::new();
    let host_id = NodeId::new();
    let other_host_id = NodeId::new();
    let actor_id = PrincipalId::new();

    let created = set_handler
        .execute(
            set_assignment_command(
                organization_id,
                project_id,
                environment_id,
                registry_id,
                host_id,
                assignment_selection("0.1.0"),
                PluginDesiredState::Enabled,
                actor_id,
                "assign-query-1",
            ),
            context(),
        )
        .await
        .expect("create dispatch")
        .expect("create");

    set_handler
        .execute(
            set_assignment_command(
                organization_id,
                project_id,
                other_environment_id,
                registry_id,
                other_host_id,
                assignment_selection("0.1.0"),
                PluginDesiredState::Absent,
                actor_id,
                "assign-query-other",
            ),
            context(),
        )
        .await
        .expect("other create dispatch")
        .expect("other create");

    let listed = list_handler
        .execute(
            ListPluginAssignments {
                organization_id,
                project_id,
                environment_id,
                access: PluginAccess::organization_wide(),
            },
            context(),
        )
        .await
        .expect("list dispatch")
        .expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.assignment.id);
    assert_eq!(listed[0].environment_id, environment_id);

    let fetched = get_handler
        .execute(
            GetPluginAssignment {
                organization_id,
                assignment_id: created.assignment.id,
            },
            context(),
        )
        .await
        .expect("get dispatch")
        .expect("get");
    assert_eq!(fetched.id, created.assignment.id);
    assert_eq!(fetched.desired_state, PluginDesiredState::Enabled);

    let missing = get_handler
        .execute(
            GetPluginAssignment {
                organization_id,
                assignment_id: crate::modules::shared_kernel::domain::PluginAssignmentId::new(),
            },
            context(),
        )
        .await
        .expect("missing dispatch")
        .expect_err("missing");
    assert!(matches!(missing, ApplicationError::NotFound(_)));
}

#[tokio::test]
async fn record_get_and_confirm_plugin_plan_projection() {
    use a3s_use_core::{
        PluginOperationConfirmation, PluginOperationPlan, PluginOperationPlanEnvelope,
    };
    use chrono::TimeZone;

    const INSTALL_PLAN: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/plugins/operation-plan-install-v4.json"
    ));
    const CONFIRMATION: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/plugins/operation-confirmation-v1.json"
    ));

    let assignments = Arc::new(InMemoryPluginAssignmentRepository::new());
    let projections = Arc::new(InMemoryPluginPlanProjectionRepository::new());
    let set_handler = SetPluginAssignmentHandler::new(assignments.clone());
    let record_handler =
        RecordPluginPlanProjectionHandler::new(assignments.clone(), projections.clone());
    let get_handler = GetPluginPlanProjectionHandler::new(projections.clone());
    let confirm_handler = ConfirmPluginPlanProjectionHandler::new(projections.clone());

    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let registry_id = PluginRegistryId::new();
    let host_id = NodeId::new();
    let actor_id = PrincipalId::new();
    let created = set_handler
        .execute(
            set_assignment_command(
                organization_id,
                project_id,
                environment_id,
                registry_id,
                host_id,
                assignment_selection("2.0.0"),
                PluginDesiredState::Enabled,
                actor_id,
                "assign-plan-1",
            ),
            context(),
        )
        .await
        .expect("assign dispatch")
        .expect("assign");

    let plan = PluginOperationPlan::from_json(INSTALL_PLAN).expect("plan");
    let envelope = PluginOperationPlanEnvelope::new(plan).expect("envelope");
    let projection_id = PluginPlanProjectionId::new();
    let operation_id = OperationId::from_uuid(Uuid::now_v7());
    let recorded_at = Utc.timestamp_millis_opt(1_785_360_000_000).unwrap();
    let recorded = record_handler
        .execute(
            RecordPluginPlanProjection {
                organization_id,
                projection_id,
                assignment_id: created.assignment.id,
                operation_id,
                envelope: envelope.clone(),
                recorded_at,
            },
            context(),
        )
        .await
        .expect("record dispatch")
        .expect("record");
    assert_eq!(recorded.id, projection_id);
    assert_eq!(recorded.plan_digest.as_str(), envelope.plan_digest);
    assert!(recorded.awaits_confirmation());

    let fetched = get_handler
        .execute(
            GetPluginPlanProjection {
                organization_id,
                projection_id,
            },
            context(),
        )
        .await
        .expect("get dispatch")
        .expect("get");
    assert_eq!(fetched.id, recorded.id);

    let confirmation = PluginOperationConfirmation::from_json(CONFIRMATION).expect("confirmation");
    let confirmed_at = Utc.timestamp_millis_opt(1_785_360_200_000).unwrap();
    let confirmed = confirm_handler
        .execute(
            ConfirmPluginPlanProjection {
                organization_id,
                projection_id,
                confirmation,
                confirmed_at,
            },
            context(),
        )
        .await
        .expect("confirm dispatch")
        .expect("confirm");
    assert!(confirmed.confirmation_digest.is_some());
    assert!(!confirmed.awaits_confirmation());
}

#[tokio::test]
async fn reconciler_enqueues_the_versioned_plugin_assignment_workflow_once() {
    use crate::modules::operations::InMemoryOperationRepository;
    use crate::modules::operations::domain::repositories::IOperationRepository;

    let assignments = Arc::new(InMemoryPluginAssignmentRepository::new());
    let operations = Arc::new(InMemoryOperationRepository::new());
    let handler = SetPluginAssignmentHandler::new(assignments.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let registry_id = PluginRegistryId::new();
    let host_id = NodeId::new();
    let actor_id = PrincipalId::new();

    let created = handler
        .execute(
            set_assignment_command(
                organization_id,
                project_id,
                environment_id,
                registry_id,
                host_id,
                assignment_selection("0.1.0"),
                PluginDesiredState::Enabled,
                actor_id,
                "assign-flow-1",
            ),
            context(),
        )
        .await
        .expect("create dispatch")
        .expect("create");
    let operation_id = created
        .assignment
        .current_operation_id
        .expect("operation id");

    let reconciler = PluginAssignmentReconciler::new(assignments.clone(), operations.clone());
    let first = reconciler.run_once(100).await.expect("reconcile");
    assert_eq!(first.started, 1);
    assert_eq!(first.replayed, 0);
    assert!(first.failures.is_empty());

    let operation = operations
        .find_request(operation_id)
        .await
        .expect("find operation")
        .expect("operation");
    assert_eq!(operation.workflow.name(), PLUGIN_ASSIGNMENT_WORKFLOW_NAME);
    assert_eq!(
        operation.workflow.version(),
        PLUGIN_ASSIGNMENT_WORKFLOW_VERSION
    );
    assert_eq!(operation.subject.kind(), "plugin_assignment");
    assert_eq!(operation.subject.id(), created.assignment.id.as_uuid());

    let second = reconciler.run_once(100).await.expect("reconcile replay");
    assert_eq!(second.started, 0);
    assert_eq!(second.replayed, 1);
    assignments.mark_operation_started(operation_id).await;
    assert_eq!(
        reconciler.run_once(100).await.expect("reconciled").started,
        0
    );
}

#[tokio::test]
async fn crash_point_1_assignment_survives_before_flow_operation_enqueue() {
    use crate::modules::operations::InMemoryOperationRepository;
    use crate::modules::operations::domain::repositories::IOperationRepository;

    // Crash point 1: assignment/Operation commit before Flow creation.
    // Prove the assignment is durable with a reserved operation id while the
    // Operations rail has not yet accepted the workflow request; reconciler
    // then starts exactly once and replays after a simulated restart.
    let assignments = Arc::new(InMemoryPluginAssignmentRepository::new());
    let operations = Arc::new(InMemoryOperationRepository::new());
    let handler = SetPluginAssignmentHandler::new(assignments.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let registry_id = PluginRegistryId::new();
    let host_id = NodeId::new();
    let actor_id = PrincipalId::new();

    let created = handler
        .execute(
            set_assignment_command(
                organization_id,
                project_id,
                environment_id,
                registry_id,
                host_id,
                assignment_selection("0.1.0"),
                PluginDesiredState::Enabled,
                actor_id,
                "assign-crash-1",
            ),
            context(),
        )
        .await
        .expect("create dispatch")
        .expect("create");
    let operation_id = created
        .assignment
        .current_operation_id
        .expect("reserved operation id");

    let pending = assignments
        .pending_operation_starts(100)
        .await
        .expect("pending starts");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, created.assignment.id);
    assert!(
        operations
            .find_request(operation_id)
            .await
            .expect("find before enqueue")
            .is_none(),
        "Flow/Operation must not exist before reconciler enqueue"
    );

    let reconciler = PluginAssignmentReconciler::new(assignments.clone(), operations.clone());
    let first = reconciler
        .run_once(100)
        .await
        .expect("reconcile after crash gap");
    assert_eq!(first.started, 1);
    assert_eq!(first.replayed, 0);
    assert!(first.failures.is_empty());
    assert!(
        operations
            .find_request(operation_id)
            .await
            .expect("find after enqueue")
            .is_some(),
        "reconciler must create the Flow/Operation request"
    );

    // Simulated control-plane restart: same durable assignment, enqueue again.
    let restarted = PluginAssignmentReconciler::new(assignments.clone(), operations.clone());
    let second = restarted
        .run_once(100)
        .await
        .expect("reconcile after restart");
    assert_eq!(second.started, 0);
    assert_eq!(second.replayed, 1);
    assert!(second.failures.is_empty());
}
