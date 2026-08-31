use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// These tests are executable architecture decisions, not a description of the
/// desired end state. Existing violations are recorded as exact removable
/// debt: new sites fail, and resolved sites must leave the allowlist.

#[test]
fn cross_context_outer_layer_debt_can_only_shrink() {
    let allowed = lines(
        r#"
agents/presentation/controllers/agent_commands_controller.rs -> identity/presentation
agents/presentation/controllers/agent_queries_controller.rs -> identity/presentation
applications/presentation/controller.rs -> identity/presentation
applications/presentation/delivery_controller.rs -> identity/presentation
artifacts/presentation/controllers/build_run_commands_controller.rs -> identity/presentation
artifacts/presentation/controllers/build_run_queries_controller.rs -> identity/presentation
assets/presentation/controllers/asset_commands_controller.rs -> identity/presentation
assets/presentation/controllers/asset_queries_controller.rs -> identity/presentation
assets/presentation/controllers/mcp_service_profile_commands_controller.rs -> identity/presentation
assets/presentation/controllers/mcp_service_profile_queries_controller.rs -> identity/presentation
assets/presentation/controllers/smart_http_controller.rs -> identity/presentation
audit/presentation/controller.rs -> identity/presentation
connectors/presentation/controller.rs -> identity/presentation
durable_cells/presentation/controller.rs -> identity/presentation
durable_cells/presentation/deployment_admission.rs -> workloads/presentation
durable_cells/presentation/dto.rs -> edge/presentation
durable_cells/presentation/dto.rs -> workloads/presentation
edge/presentation/controllers/domain_claim_commands_controller.rs -> identity/presentation
edge/presentation/controllers/domain_claim_queries_controller.rs -> identity/presentation
edge/presentation/controllers/gateway_scope_commands_controller.rs -> identity/presentation
edge/presentation/controllers/gateway_scope_queries_controller.rs -> identity/presentation
edge/presentation/controllers/mcp_credential_commands_controller.rs -> identity/presentation
edge/presentation/controllers/mcp_credential_queries_controller.rs -> identity/presentation
edge/presentation/controllers/mcp_route_policy_commands_controller.rs -> identity/presentation
edge/presentation/controllers/mcp_route_policy_queries_controller.rs -> identity/presentation
edge/presentation/controllers/route_queries_controller.rs -> identity/presentation
edge/presentation/controllers/routes_controller.rs -> identity/presentation
executions/presentation/controllers/execution_commands_controller.rs -> identity/presentation
executions/presentation/controllers/execution_queries_controller.rs -> identity/presentation
fleet/presentation/controllers/node_management_controller.rs -> identity/presentation
fleet/presentation/controllers/node_pool_management_controller.rs -> identity/presentation
fleet/presentation/controllers/node_pool_queries_controller.rs -> identity/presentation
fleet/presentation/controllers/node_queries_controller.rs -> identity/presentation
forms/presentation/controllers/form_commands_controller.rs -> identity/presentation
forms/presentation/controllers/form_queries_controller.rs -> identity/presentation
notifications/presentation/controller.rs -> identity/presentation
operations/presentation/controllers/operations_query_controller.rs -> identity/presentation
plugins/presentation/controllers/plugin_registry_queries_controller.rs -> identity/presentation
projects/presentation/controllers/project_queries_controller.rs -> identity/presentation
projects/presentation/controllers/projects_controller.rs -> identity/presentation
secrets/presentation/controllers/secret_queries_controller.rs -> identity/presentation
secrets/presentation/controllers/secrets_controller.rs -> identity/presentation
sources/presentation/controllers/github_connections_controller.rs -> identity/presentation
sources/presentation/controllers/github_repository_subscription_queries_controller.rs -> identity/presentation
sources/presentation/controllers/github_repository_subscriptions_controller.rs -> identity/presentation
sources/presentation/controllers/source_revision_queries_controller.rs -> identity/presentation
sources/presentation/controllers/source_revisions_controller.rs -> identity/presentation
workflow/presentation/controllers/ontology_commands_controller.rs -> identity/presentation
workflow/presentation/controllers/ontology_queries_controller.rs -> identity/presentation
workflow/presentation/controllers/request.rs -> identity/presentation
workflow/presentation/controllers/workflow_commands_controller.rs -> identity/presentation
workflow/presentation/controllers/workflow_queries_controller.rs -> identity/presentation
workloads/infrastructure/persistence/postgres/resource_claims.rs -> fleet/infrastructure
workloads/presentation/controllers/workload_queries_controller.rs -> identity/presentation
workloads/presentation/controllers/workloads_controller.rs -> identity/presentation
"#,
    );
    let actual = foreign_outer_layer_sites();
    let unexpected = difference(&actual, &allowed);
    let resolved = difference(&allowed, &actual);

    assert!(
        unexpected.is_empty(),
        "new cross-context outer-layer dependencies bypass an owner port or published contract:\n{}",
        unexpected.join("\n")
    );
    assert!(
        resolved.is_empty(),
        "resolved cross-context outer-layer debt must be removed from the exact allowlist:\n{}",
        resolved.join("\n")
    );
}

#[test]
fn duplicate_physical_table_mappings_can_only_shrink() {
    let allowed = lines(
        r#"
mcp_service_profiles @ edge/infrastructure/persistence/postgres_schema.rs#McpServiceProfiles
mcp_service_profiles @ workloads/infrastructure/persistence/postgres/schema.rs#McpServiceProfiles
nodes @ edge/infrastructure/persistence/postgres_schema.rs#Nodes
nodes @ fleet/infrastructure/persistence/postgres/schema.rs#Nodes
operation_requests @ operations/infrastructure/persistence/postgres/schema.rs#OperationRequests
operation_requests @ workflow/infrastructure/persistence/workflow_run_postgres/schema.rs#OperationRequests
operation_requests @ workloads/infrastructure/persistence/postgres/schema.rs#OperationRequests
workloads @ edge/infrastructure/persistence/postgres_schema.rs#Workloads
workloads @ workloads/infrastructure/persistence/postgres/schema.rs#Workloads
workflow_runs @ workflow/infrastructure/persistence/human_task_postgres/schema.rs#WorkflowRuns
workflow_runs @ workflow/infrastructure/persistence/workflow_run_postgres/schema.rs#WorkflowRuns
"#,
    );
    let mappings = physical_table_mappings();
    let actual = mappings
        .into_iter()
        .filter(|(_, sites)| sites.len() > 1)
        .flat_map(|(table, sites)| {
            sites
                .into_iter()
                .map(move |site| format!("{table} @ {site}"))
        })
        .collect::<BTreeSet<_>>();
    let unexpected = difference(&actual, &allowed);
    let resolved = difference(&allowed, &actual);

    assert!(
        unexpected.is_empty(),
        "a physical table has more than one mapping authority:\n{}",
        unexpected.join("\n")
    );
    assert!(
        resolved.is_empty(),
        "resolved duplicate physical mappings must be removed from the exact allowlist:\n{}",
        resolved.join("\n")
    );
}

#[test]
fn domain_technical_dependency_debt_can_only_shrink() {
    const FORBIDDEN: &[&str] = &[
        "a3s_box_runtime::",
        "a3s_orm::",
        "axum::",
        "hyper::",
        "lettre::",
        "reqwest::",
        "sqlx::",
        "std::fs::",
        "std::process::",
        "tokio::",
    ];
    let allowed = BTreeSet::new();
    let mut actual = BTreeSet::new();

    visit_production_sources(|relative, source| {
        if layer(relative) != Some("domain") {
            return;
        }
        for forbidden in FORBIDDEN {
            if source.contains(forbidden) {
                actual.insert(format!("{} -> {forbidden}", display(relative)));
            }
        }
        if source
            .lines()
            .any(|line| line.trim_start().starts_with("use object_store::"))
        {
            actual.insert(format!("{} -> use object_store::", display(relative)));
        }
    });

    let unexpected = difference(&actual, &allowed);
    assert!(
        unexpected.is_empty(),
        "domain code depends on a runtime, transport, persistence, or provider mechanism:\n{}",
        unexpected.join("\n")
    );
}

#[test]
fn data_credentials_cross_one_secrets_owner_interface_and_one_adapter() {
    let mut violations = BTreeSet::new();
    let mut adapter_sites = BTreeSet::new();
    visit_production_sources(|relative, source| {
        if context(relative) != Some("data") {
            return;
        }
        if layer(relative) == Some("application") {
            for forbidden in [
                "modules::secrets::application",
                "modules::secrets::domain",
                "ISecretRepository",
                "ISecretEncryptionService",
                "ExactSecretVersionAccess::new",
                "ExactSecretMaterializer::new",
            ] {
                if source.contains(forbidden) {
                    violations.insert(format!(
                        "{} imports Secrets implementation authority {forbidden}",
                        display(relative)
                    ));
                }
            }
        }
        if layer(relative) == Some("infrastructure")
            && (source.contains("exact_secret_version_access")
                || source.contains("exact_secret_materializer"))
        {
            adapter_sites.insert(display(relative));
        }
    });
    assert!(
        violations.is_empty(),
        "Data Application bypassed the Secrets published interface:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
    assert_eq!(
        adapter_sites,
        lines("data/infrastructure/object_namespace_credentials.rs"),
        "Data must translate the Secrets exact-version boundary at one adapter site"
    );

    let credentials = std::fs::read_to_string(
        module_root().join("data/application/object_namespace_credentials.rs"),
    )
    .expect("read Data object namespace credentials");
    let credentials = production_source(&credentials);
    for required in [
        "Arc<dyn IExactSecretVersionAccess>",
        "Arc<dyn IExactSecretMaterializer>",
        "from_secret_version_access",
        "from_secret_materializer",
        "SecretPlaintext",
    ] {
        assert!(
            credentials.contains(required),
            "Data credential boundary lost interface-owned dependency {required}"
        );
    }

    let owner =
        std::fs::read_to_string(module_root().join("secrets/application/materialization.rs"))
            .expect("read Secrets exact-version owner boundary");
    let owner = production_source(&owner);
    for required in [
        "pub trait IExactSecretVersionAccess",
        "pub trait IExactSecretMaterializer",
        "pub(crate) fn exact_secret_version_access(",
        "pub(crate) fn exact_secret_materializer(",
    ] {
        assert!(
            owner.contains(required),
            "Secrets lost published exact-version boundary {required}"
        );
    }
    assert_eq!(
        owner.matches(".find_materializable_version(").count(),
        1,
        "Secrets must retain one active exact-version query mechanism"
    );
    assert_eq!(
        owner.matches(".decrypt(").count(),
        1,
        "Secrets must retain one exact-version plaintext mechanism"
    );

    let adapter = std::fs::read_to_string(
        module_root().join("data/infrastructure/object_namespace_credentials.rs"),
    )
    .expect("read Data-to-Secrets credential adapter");
    let adapter = production_source(&adapter);
    for required in [
        "exact_secret_version_access(secrets)",
        "exact_secret_materializer(secrets, encryption)",
        "Self::from_secret_version_access",
        "Self::from_secret_materializer",
    ] {
        assert!(
            adapter.contains(required),
            "Data credential adapter lost exact translation {required}"
        );
    }
    for forbidden in [
        "find_materializable_version",
        ".decrypt(",
        "SecretPlaintext::new",
        "EncryptedSecretValue",
    ] {
        assert!(
            !adapter.contains(forbidden),
            "Data credential adapter duplicated Secrets mechanism {forbidden}"
        );
    }
}

#[test]
fn runtime_contracts_enter_domains_only_through_named_published_boundaries() {
    let allowed_files = lines(
        r#"
artifacts/domain/entities/build_run.rs
fleet/domain/repositories/node_control_repository.rs
workloads/domain/services/deployment_route_updater.rs
"#,
    );
    let mut violations = BTreeSet::new();

    visit_production_sources(|relative, source| {
        if layer(relative) != Some("domain") || !source.contains("a3s_runtime::") {
            return;
        }
        let file = display(relative);
        if !allowed_files.contains(&file) {
            violations.insert(format!("{file} imports a3s-runtime"));
        }
        for line in source.lines().filter(|line| line.contains("a3s_runtime::")) {
            if !line.contains("a3s_runtime::contract::") {
                violations.insert(format!(
                    "{file} imports a non-contract runtime path: {line:?}"
                ));
            }
        }
    });

    assert!(
        violations.is_empty(),
        "a domain imported runtime execution/provider authority instead of published language:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn executions_bound_task_policy_is_local_and_runtime_translation_has_one_adapter() {
    let mut violations = BTreeSet::new();
    visit_production_sources(|relative, source| {
        if context(relative) == Some("executions")
            && matches!(layer(relative), Some("application") | Some("domain"))
            && source.contains("a3s_runtime::")
        {
            violations.insert(format!(
                "{} imports a3s-runtime outside the Executions adapter edge",
                display(relative)
            ));
        }
    });
    assert!(
        violations.is_empty(),
        "Executions product semantics imported Runtime protocol authority:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );

    let policy = std::fs::read_to_string(
        module_root().join("executions/domain/entities/execution_task_policy.rs"),
    )
    .expect("read Execution Task policy");
    for required in [
        "pub struct ExecutionTaskArtifactMount",
        "pub struct ExecutionTaskSecret",
        "pub enum ExecutionTaskSecretTarget",
        "reference: CloudSecretReference",
        "pub fn artifact_uri(&self) -> Result<String, String>",
        "impl<'de> Deserialize<'de> for ExecutionTaskPolicy",
        "migration-119 document",
    ] {
        assert!(
            policy.contains(required),
            "Executions Domain lost local bound-Task contract {required}"
        );
    }
    for forbidden in [
        "a3s_runtime::",
        "RuntimeMount",
        "RuntimeUnitSpec",
        "RuntimeProcessSpec",
        "Vec<SecretReference>",
        "pub kind: String",
        "pub subject_id: Uuid",
        "pub digest: Sha256Digest",
    ] {
        assert!(
            !production_source(&policy).contains(forbidden),
            "Executions Domain regained Runtime protocol type {forbidden}"
        );
    }

    let adapter =
        std::fs::read_to_string(module_root().join("executions/infrastructure/task_spec.rs"))
            .expect("read Execution Runtime Task adapter");
    for required in [
        "fn runtime_mount(",
        "fn runtime_secret(",
        "RuntimeMountSource::Artifact",
        "SecretTarget::Environment",
        "read_only: true",
        "spec.validate()?",
    ] {
        assert!(
            production_source(&adapter).contains(required),
            "Executions Runtime adapter lost exact translation {required}"
        );
    }
    let production_adapter = production_source(&adapter);
    for projection in ["Ok(RuntimeMount {", "SecretReference {"] {
        assert_eq!(
            production_adapter
                .lines()
                .filter(|line| line.trim() == projection)
                .count(),
            1,
            "Executions Runtime translation must have exactly one {projection} projection"
        );
    }
}

#[test]
fn durable_cells_domain_never_imports_workloads_owner_models() {
    let mut violations = BTreeSet::new();

    visit_production_sources(|relative, source| {
        if context(relative) != Some("durable_cells") || layer(relative) != Some("domain") {
            return;
        }
        for line in source
            .lines()
            .filter(|line| line.contains("crate::modules::workloads"))
        {
            violations.insert(format!("{} contains {line:?}", display(relative)));
        }
    });

    assert!(
        violations.is_empty(),
        "Durable Cells Domain imported Workloads owner models instead of a consumer-owned projection:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn durable_cells_application_never_imports_infrastructure_implementations() {
    let mut violations = BTreeSet::new();

    visit_production_sources(|relative, source| {
        if context(relative) != Some("durable_cells") || layer(relative) != Some("application") {
            return;
        }
        for line in source
            .lines()
            .filter(|line| line.contains("::infrastructure"))
        {
            violations.insert(format!("{} contains {line:?}", display(relative)));
        }
    });

    assert!(
        violations.is_empty(),
        "Durable Cells Application imported an infrastructure implementation instead of an owner Application boundary or published contract:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn flow_contract_enters_only_the_workflow_dag_compiler() {
    const ALLOWED_FILE: &str = "workflow/domain/workflow_graph.rs";
    const ALLOWED_IMPORT: &str = "use a3s_flow::{WorkflowDag, WorkflowDagEdge, WorkflowDagNode};";
    let mut violations = BTreeSet::new();

    visit_production_sources(|relative, source| {
        if layer(relative) != Some("domain") {
            return;
        }
        for line in source.lines().filter(|line| line.contains("a3s_flow::")) {
            let file = display(relative);
            if file != ALLOWED_FILE || line.trim() != ALLOWED_IMPORT {
                violations.insert(format!("{file} contains {line:?}"));
            }
        }
    });

    assert!(
        violations.is_empty(),
        "a domain imported Flow execution authority instead of the pure DAG contract:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn artifacts_domain_enters_sources_only_through_published_language() {
    const SOURCES_PREFIX: &str = "crate::modules::sources::";
    const PUBLISHED_PREFIX: &str = "crate::modules::sources::published::";
    let mut violations = BTreeSet::new();

    visit_production_sources(|relative, source| {
        if context(relative) != Some("artifacts") || layer(relative) != Some("domain") {
            return;
        }
        for line in source.lines().filter(|line| line.contains(SOURCES_PREFIX)) {
            if !line.contains(PUBLISHED_PREFIX) {
                violations.insert(format!("{} contains {line:?}", display(relative)));
            }
        }
    });

    assert!(
        violations.is_empty(),
        "Artifacts Domain imported Sources internals instead of its published language:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn assets_domain_enters_sources_only_through_published_language() {
    const SOURCES_PREFIX: &str = "crate::modules::sources::";
    const PUBLISHED_PREFIX: &str = "crate::modules::sources::published::";
    let mut violations = BTreeSet::new();

    visit_production_sources(|relative, source| {
        if context(relative) != Some("assets") || layer(relative) != Some("domain") {
            return;
        }
        for line in source.lines().filter(|line| line.contains(SOURCES_PREFIX)) {
            if !line.contains(PUBLISHED_PREFIX) {
                violations.insert(format!("{} contains {line:?}", display(relative)));
            }
        }
    });

    assert!(
        violations.is_empty(),
        "Assets Domain imported Sources internals instead of its published language:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn sources_webhook_entity_owns_pull_request_kind_as_a_value_object() {
    let source = std::fs::read_to_string(
        module_root().join("sources/domain/entities/source_webhook_delivery.rs"),
    )
    .expect("read Sources webhook delivery entity");

    assert!(
        source.contains("crate::modules::sources::domain::value_objects::{")
            && source.contains("PullRequestChangeKind")
            && !source.contains("crate::modules::sources::domain::services"),
        "Sources webhook delivery entity must depend on its value-object language, not a verifier service"
    );
}

#[test]
fn artifacts_build_source_resolver_never_loads_owner_repositories() {
    let source = std::fs::read_to_string(
        module_root().join("artifacts/infrastructure/build_source_resolver.rs"),
    )
    .expect("read Artifacts build source resolver");
    let forbidden = [
        "crate::modules::assets::domain",
        "crate::modules::sources::domain",
        "IAssetRepository",
        "IAssetGitRepository",
        "ISourceRevisionRepository",
        "publish_source_build_input",
        ".find_asset(",
        ".find_release(",
        ".admit_manifest(",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|needle| source.contains(needle))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "Artifacts build-source resolution regained a foreign aggregate, repository, or Git authority: {}",
        violations.join(", ")
    );
}

#[test]
fn artifacts_external_input_staging_uses_only_the_sources_archive_port() {
    let source = std::fs::read_to_string(
        module_root().join("artifacts/infrastructure/source_build_input_preparer.rs"),
    )
    .expect("read Artifacts build input preparer");
    assert!(
        source.contains("IExternalSourceArchivePort")
            && source.contains("ExternalSourceArchiveRequest"),
        "Artifacts external input staging lost its consumer-owned Sources port"
    );
    let violations = [
        "crate::modules::sources::domain",
        "ISourceCheckout",
        "IGithubConnectionRepository",
        "IGithubInstallationTokenService",
        "GithubInstallationTokenRequest",
        "SourceCheckoutRequest",
        "CheckedOutSource",
        "SourceProviderCredential",
    ]
    .into_iter()
    .filter(|needle| {
        source
            .split("#[cfg(test)]")
            .next()
            .is_some_and(|production| production.contains(needle))
    })
    .collect::<Vec<_>>();
    assert!(
        violations.is_empty(),
        "Artifacts external input staging regained Source provider or repository internals: {}",
        violations.join(", ")
    );
}

#[test]
fn artifacts_input_io_ports_stay_out_of_the_domain_layer() {
    let domain = std::fs::read_to_string(module_root().join("artifacts/domain/mod.rs"))
        .expect("read Artifacts Domain module");
    let services = std::fs::read_to_string(module_root().join("artifacts/domain/services/mod.rs"))
        .expect("read Artifacts Domain services module");
    for forbidden in [
        "build_input_preparer",
        "IBuildInputPreparer",
        "BuildInputPreparationError",
        "PreparedBuildInput",
    ] {
        assert!(
            !domain.contains(forbidden) && !services.contains(forbidden),
            "Artifacts Domain regained application I/O contract {forbidden}"
        );
    }

    let adapter = std::fs::read_to_string(
        module_root().join("sources/infrastructure/external_build_archive.rs"),
    )
    .expect("read Sources external build archive adapter");
    assert!(
        adapter.contains("crate::modules::artifacts::application")
            && !adapter.contains("crate::modules::artifacts::domain"),
        "Sources archive adapter must implement the consumer Application port without importing Artifacts Domain"
    );
}

#[test]
fn artifacts_candidate_reservation_reads_only_its_fact_projection() {
    let repository = std::fs::read_to_string(
        module_root().join("artifacts/infrastructure/persistence/postgres.rs"),
    )
    .expect("read Artifacts PostgreSQL repository");
    let projection = std::fs::read_to_string(
        module_root().join("artifacts/infrastructure/persistence/postgres/candidate_projection.rs"),
    )
    .expect("read Artifacts PostgreSQL candidate projection");
    let reservation = repository
        .split("async fn reserve_pending(")
        .nth(1)
        .and_then(|tail| tail.split("async fn pending_operation_starts(").next())
        .expect("isolate build candidate reservation");

    assert!(
        reservation.contains("SELECT_BUILD_CANDIDATES")
            && projection.contains(
                "const SELECT_BUILD_CANDIDATES: &str = \"select c.organization_id, c.subject_kind, c.subject_id, c.preview_id, c.project_id, c.environment_id, c.source_revision_id, c.asset_id, c.asset_release_id, c.repository_identity, c.commit_sha, c.owner_input_digest, c.requested_at from artifact_build_candidates c\"",
            ),
        "BuildRun reservation lost its Artifacts-owned fact projection"
    );
    let violations = [
        "external_source_revisions",
        "asset_releases",
        " join assets ",
        "for update of r",
    ]
    .into_iter()
    .filter(|needle| reservation.contains(needle))
    .collect::<Vec<_>>();
    assert!(
        violations.is_empty(),
        "BuildRun reservation regained cross-context candidate discovery: {}",
        violations.join(", ")
    );
}

#[test]
fn artifacts_candidate_projector_consumes_only_published_owner_facts() {
    let source = std::fs::read_to_string(
        module_root().join("artifacts/infrastructure/build_candidate_projector.rs"),
    )
    .expect("read Artifacts build candidate projector");
    for required in [
        "crate::modules::assets::published",
        "crate::modules::sources::published",
        "IArtifactBuildProjectionPort",
        "PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_EVENT_KEY",
        "PREVIEW_SOURCE_REVISION_LIFECYCLE_MAX_BYTES",
        "project_preview_build_lifecycle",
    ] {
        assert!(
            source.contains(required),
            "candidate projector lost required boundary {required}"
        );
    }
    let violations = [
        "crate::modules::assets::domain",
        "crate::modules::sources::domain",
        "IAssetRepository",
        "ISourceRevisionRepository",
        "IHostedAssetBuildInputQueryPort",
        "IBuildRunRepository",
    ]
    .into_iter()
    .filter(|needle| {
        source
            .split("#[cfg(test)]")
            .next()
            .is_some_and(|production| production.contains(needle))
    })
    .collect::<Vec<_>>();
    assert!(
        violations.is_empty(),
        "candidate projector imported owner internals or BuildRun lifecycle authority: {}",
        violations.join(", ")
    );
}

#[test]
fn source_build_input_projection_is_used_only_behind_the_owner_query() {
    let mut violations = BTreeSet::new();

    visit_production_sources(|relative, source| {
        if context(relative) == Some("sources") {
            return;
        }
        for line in source
            .lines()
            .filter(|line| line.contains("publish_source_build_input"))
        {
            violations.insert(format!("{} contains {line:?}", display(relative)));
        }
    });

    assert!(
        violations.is_empty(),
        "a production consumer bypassed ISourceBuildInputQueryPort with an owner aggregate projection:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn github_source_discovery_has_one_transient_interface_boundary_and_no_second_mechanism() {
    let application_path = "sources/application/github_source_discovery.rs";
    let application = std::fs::read_to_string(module_root().join(application_path))
        .expect("read Sources discovery Application service");
    let production_application = production_source(&application);
    let compact_application = production_application
        .split_whitespace()
        .collect::<String>();
    for required in [
        "Arc<dynIGithubConnectionRepository>",
        "Arc<dynIGithubSourceDiscoveryProvider>",
        "Arc<SourceRepositoryPolicy>",
    ] {
        assert!(
            compact_application.contains(required),
            "Sources discovery lost inward boundary {required}"
        );
    }
    for forbidden in [
        "::infrastructure",
        "::presentation",
        "reqwest",
        "Postgres",
        "IOutboxRepository",
        "IEventPublisher",
        "OutboxRelay",
        "NatsProvider",
        "tokio::spawn",
        "SourceProviderCredential",
        "GithubInstallationTokenIssuer",
    ] {
        assert!(
            !production_application.contains(forbidden),
            "Sources discovery Application acquired outer-layer or duplicate mechanism {forbidden}"
        );
    }

    let decorator_path = "sources/infrastructure/revalidating_github_source_discovery.rs";
    let decorator = std::fs::read_to_string(module_root().join(decorator_path))
        .expect("read revalidating Sources discovery provider");
    let compact_decorator = production_source(&decorator)
        .split_whitespace()
        .collect::<String>();
    for required in [
        "Arc<dynIGithubConnectionAuthorityService>",
        "Arc<dynIGithubSourceDiscoveryProvider>",
    ] {
        assert!(
            compact_decorator.contains(required),
            "Sources discovery provider decorator lost authority boundary {required}"
        );
    }

    let app = std::fs::read_to_string(
        module_root()
            .parent()
            .expect("src directory")
            .join("app.rs"),
    )
    .expect("read application composition");
    assert_eq!(
        app.matches("GithubSourceDiscoveryQueryService::new")
            .count(),
        1,
        "all process roles must share one Sources discovery query composition"
    );
    assert_eq!(
        app.matches("RevalidatingGithubSourceDiscovery::new")
            .count(),
        1,
        "all process roles must share one discovery authority decorator"
    );

    for relative in [
        "sources/presentation/controllers/github_connections_controller.rs",
        "../presentation/management_mcp/sources.rs",
    ] {
        let source = std::fs::read_to_string(module_root().join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        let production = production_source(&source);
        for forbidden in [
            "IGithubConnectionRepository",
            "IGithubSourceDiscoveryProvider",
            "GithubInstallationTokenIssuer",
            "SourceRepositoryPolicy",
            "reqwest",
            "a3s_acl",
            "Postgres",
        ] {
            assert!(
                !production.contains(forbidden),
                "{relative} bypassed the Sources discovery QueryBus boundary with {forbidden}"
            );
        }
    }
}

#[test]
fn sources_owner_scope_uses_two_minimum_ports_and_one_adapter_module() {
    let port_path = "sources/application/owner_scope_access.rs";
    let port = std::fs::read_to_string(module_root().join(port_path))
        .expect("read Sources owner-scope ports");
    let compact_port = production_source(&port)
        .split_whitespace()
        .collect::<String>();
    for required in [
        "pubtraitISourceOrganizationAccess:Send+Sync",
        "require_organization(&self,organization_id:OrganizationId)->ApplicationResult<()>;",
        "pubtraitISourceEnvironmentAccess:Send+Sync",
        "require_environment(&self,organization_id:OrganizationId,project_id:ProjectId,environment_id:EnvironmentId,)->ApplicationResult<()>;",
    ] {
        assert!(
            compact_port.contains(required),
            "Sources owner-scope boundary lost minimum interface {required}"
        );
    }
    for forbidden in [
        "crate::modules::identity",
        "crate::modules::projects",
        "IOrganizationRepository",
        "IEnvironmentRepository",
        "entities::Organization",
        "entities::Environment",
    ] {
        assert!(
            !production_source(&port).contains(forbidden),
            "Sources owner-scope port imported owner authority {forbidden}"
        );
    }

    let mut application_violations = BTreeSet::new();
    let mut repository_import_sites = BTreeSet::new();
    visit_production_sources(|relative, source| {
        if context(relative) != Some("sources") {
            return;
        }
        if layer(relative) == Some("application")
            && [
                "crate::modules::identity",
                "crate::modules::projects",
                "IOrganizationRepository",
                "IEnvironmentRepository",
            ]
            .iter()
            .any(|forbidden| source.contains(forbidden))
        {
            application_violations.insert(display(relative));
        }
        if source.contains("IOrganizationRepository") || source.contains("IEnvironmentRepository") {
            repository_import_sites.insert(display(relative));
        }
    });
    assert!(
        application_violations.is_empty(),
        "Sources Application bypassed its owner-scope ports:\n{}",
        application_violations
            .into_iter()
            .collect::<Vec<_>>()
            .join("\n")
    );

    let adapter_path = "sources/infrastructure/owner_scope_access.rs";
    assert_eq!(
        repository_import_sites,
        lines(adapter_path),
        "Sources owner repositories must be confined to one Infrastructure adapter module"
    );
    let adapter = std::fs::read_to_string(module_root().join(adapter_path))
        .expect("read Sources owner-scope adapter");
    let production_adapter = production_source(&adapter);
    let compact_adapter = production_adapter.split_whitespace().collect::<String>();
    for required in [
        "implISourceOrganizationAccessforIdentitySourceOrganizationAccessAdapter",
        "implISourceEnvironmentAccessforProjectsSourceEnvironmentAccessAdapter",
        "organizations:Arc<dynIOrganizationRepository>",
        "environments:Arc<dynIEnvironmentRepository>",
        "Self::from_organization_access(",
        "Self::from_environment_access(",
    ] {
        assert!(
            compact_adapter.contains(required),
            "Sources owner-scope adapter lost boundary behavior {required}"
        );
    }
    assert_eq!(
        production_adapter.matches(".find(").count(),
        2,
        "Organization and Environment owner evidence must each have one query mechanism"
    );
    for forbidden in [
        ".create(",
        ".list(",
        ".deactivate(",
        ".accept(",
        "CommandHandler",
        "QueryHandler",
        "IOutboxRepository",
        "IEventPublisher",
        "Postgres",
        "tokio::spawn",
    ] {
        assert!(
            !production_adapter.contains(forbidden),
            "Sources owner-scope adapter introduced lifecycle or persistence authority {forbidden}"
        );
    }

    for (relative, boundary, call, next) in [
        (
            "sources/application/commands/begin_github_connection/handler.rs",
            "organization_access:Arc<dynISourceOrganizationAccess>",
            ".require_organization(",
            "letstate=matchgenerate_oauth_flow_secret",
        ),
        (
            "sources/application/commands/create_github_repository_subscription/handler.rs",
            "environment_access:Arc<dynISourceEnvironmentAccess>",
            ".require_environment(",
            "letconnection=matchconnections.find",
        ),
        (
            "sources/application/commands/deactivate_github_repository_subscription/handler.rs",
            "environment_access:Arc<dynISourceEnvironmentAccess>",
            ".require_environment(",
            "letmutsubscription=matchsubscriptions.find",
        ),
        (
            "sources/application/commands/resolve_external_source_revision/handler.rs",
            "environment_access:Arc<dynISourceEnvironmentAccess>",
            ".require_environment(",
            "letprovider=matchGitProvider::parse",
        ),
        (
            "sources/application/queries/list_source_revisions/handler.rs",
            "environment_access:Arc<dynISourceEnvironmentAccess>",
            ".require_environment(",
            "Ok(sources.list(",
        ),
        (
            "sources/application/queries/list_github_repository_subscriptions/handler.rs",
            "environment_access:Arc<dynISourceEnvironmentAccess>",
            ".require_environment(",
            "matchsubscriptions.list(",
        ),
    ] {
        let handler = std::fs::read_to_string(module_root().join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        let handler = production_source(&handler);
        let compact = handler.split_whitespace().collect::<String>();
        assert!(
            compact.contains(boundary),
            "{relative} stopped depending on Sources-owned boundary {boundary}"
        );
        assert_eq!(
            compact.matches(call).count(),
            1,
            "{relative} must enter owner-scope validation exactly once"
        );
        let boundary_call = compact
            .find(call)
            .unwrap_or_else(|| panic!("{relative} lost owner-scope call"));
        let first_domain_action = compact
            .find(next)
            .unwrap_or_else(|| panic!("{relative} lost expected domain action {next}"));
        assert!(
            boundary_call < first_domain_action,
            "{relative} must validate owner scope before its first domain action"
        );
        for forbidden in [
            "crate::modules::identity",
            "crate::modules::projects",
            "IOrganizationRepository",
            "IEnvironmentRepository",
            ".find(command.organization_id,command.project_id,command.environment_id)",
            ".find(query.organization_id,query.project_id,query.environment_id)",
        ] {
            assert!(
                !compact.contains(forbidden),
                "{relative} bypassed the owner-scope boundary with {forbidden}"
            );
        }
    }
}

#[test]
fn user_files_has_one_lifecycle_repository_one_streaming_object_port_and_no_parallel_mechanism() {
    let root = module_root();
    let repository = std::fs::read_to_string(root.join("files/domain/repository.rs"))
        .expect("read Files repository port");
    let object_store = std::fs::read_to_string(root.join("files/application/object_store.rs"))
        .expect("read Files object-store port");
    assert_eq!(
        repository.matches("pub trait IUserFileRepository").count(),
        1
    );
    assert_eq!(
        object_store
            .matches("pub trait IUserFileObjectStore")
            .count(),
        1
    );
    assert!(object_store.contains("AsyncRead"));
    assert!(!object_store.contains("Vec<u8>"));

    let events = std::fs::read_to_string(root.join("files/domain/events.rs"))
        .expect("read Files lifecycle event");
    assert_eq!(
        events
            .matches("pub struct UserFileLifecycleChanged")
            .count(),
        1
    );
    assert!(events.contains("cleanup_due_at: Option<DateTime<Utc>>"));
    for duplicate in ["UserFileCleanupRequested", "UserFileDeletionRequested"] {
        assert!(
            !events.contains(duplicate),
            "Files introduced a second cleanup event authority {duplicate}"
        );
    }

    let service = std::fs::read_to_string(root.join("files/application/service.rs"))
        .expect("read Files Application service");
    let compact_service = production_source(&service)
        .split_whitespace()
        .collect::<String>();
    for required in [
        "files:Arc<dynIUserFileRepository>",
        "objects:Arc<dynIUserFileObjectStore>",
    ] {
        assert!(
            compact_service.contains(required),
            "Files Application lost inward interface boundary {required}"
        );
    }
    for forbidden in [
        "::infrastructure",
        "::presentation",
        "crate::modules::identity",
        "ResourceAccessEvaluator",
        "ResourceGrantScope",
        "Postgres",
        "reqwest",
        "tokio::spawn",
        "Vec<u8>",
        "IOutboxRepository",
        "IEventPublisher",
    ] {
        assert!(
            !production_source(&service).contains(forbidden),
            "Files Application introduced outer-layer or duplicate mechanism {forbidden}"
        );
    }
    assert_eq!(
        compact_service.matches("access:UserFileAccess").count(),
        5,
        "every Files request must carry the Files-owned access projection"
    );

    let access = std::fs::read_to_string(root.join("files/application/resource_access.rs"))
        .expect("read Files access projection");
    let production_access = production_source(&access);
    for required in [
        "pub struct UserFileAccess",
        "pub(crate) fn organization_wide(",
        "pub(crate) fn restricted_projects(",
        "pub(crate) fn project_is_visible(",
        "pub(crate) const fn organization_quota_is_visible(",
    ] {
        assert!(
            production_access.contains(required),
            "Files access projection lost closed operation {required}"
        );
    }
    for forbidden in [
        "crate::modules::identity",
        "ResourceAccessEvaluator",
        "ResourceGrantScope",
        "MembershipRole",
        "ApiTokenScope",
    ] {
        assert!(
            !production_access.contains(forbidden),
            "Files access projection copied Identity authority {forbidden}"
        );
    }

    let mut identity_dependencies = BTreeSet::new();
    visit_production_sources(|relative, source| {
        if context(relative) != Some("files") {
            return;
        }
        for forbidden in [
            "crate::modules::identity",
            "ResourceAccessEvaluator",
            "ResourceGrantScope",
        ] {
            if source.contains(forbidden) {
                identity_dependencies.insert(format!(
                    "{} contains Identity implementation authority {forbidden}",
                    display(relative)
                ));
            }
        }
    });
    assert!(
        identity_dependencies.is_empty(),
        "Files imported Identity instead of its bounded access projection:\n{}",
        identity_dependencies
            .into_iter()
            .collect::<Vec<_>>()
            .join("\n")
    );

    let src = root.parent().expect("src directory");
    let request_context = std::fs::read_to_string(src.join("presentation/request_context.rs"))
        .expect("read root request-context ACL");
    let production_request_context = production_source(&request_context);
    for required in [
        "pub(crate) fn user_file_access(",
        "UserFileAccess::organization_wide()",
        "UserFileAccess::restricted_projects(",
        "ResourceGrantScope::Project { project_id } => Some(project_id)",
        "ResourceGrantScope::Environment { .. } | ResourceGrantScope::Node { .. } => None",
    ] {
        assert!(
            production_request_context.contains(required),
            "root Presentation ACL lost fail-closed Files translation {required}"
        );
    }
    assert_eq!(
        production_request_context
            .matches("UserFileAccess::restricted_projects(")
            .count(),
        1,
        "Identity access must enter Files through one root ACL mapping"
    );

    let presentation_policy = std::fs::read_to_string(src.join("presentation/mod.rs"))
        .expect("read root Presentation policy");
    for required in [
        "organization_tenant_file_write_controller",
        "organization_tenant_cloud_read_controller",
        "ApiTokenScope::FILE_WRITE",
        "ApiTokenScope::CLOUD_READ",
    ] {
        assert!(
            presentation_policy.contains(required),
            "root Presentation lost Files route policy {required}"
        );
    }

    let controller = std::fs::read_to_string(root.join("files/presentation/controller.rs"))
        .expect("read Files HTTP adapter");
    let production_controller = production_source(&controller);
    for required in [
        "organization_tenant_file_write_controller(controller)",
        "organization_tenant_cloud_read_controller(controller)",
        "access: user_file_access(&resource_access_evaluator(",
    ] {
        assert!(
            production_controller.contains(required),
            "Files HTTP adapter bypassed root Presentation boundary {required}"
        );
    }
    for forbidden in [
        "crate::modules::identity",
        "ApiTokenScope",
        "AUTH_SCOPES_METADATA",
        "OrganizationTenantGuard",
    ] {
        assert!(
            !production_controller.contains(forbidden),
            "Files HTTP adapter duplicated Identity route policy {forbidden}"
        );
    }

    let management_mcp = std::fs::read_to_string(src.join("presentation/management_mcp/files.rs"))
        .expect("read Files Management MCP adapter");
    assert_eq!(
        production_source(&management_mcp)
            .matches("access: user_file_access(&resource_access)")
            .count(),
        5,
        "Files Management MCP must translate every request through the one root ACL"
    );

    let postgres =
        std::fs::read_to_string(root.join("files/infrastructure/postgres_repository.rs"))
            .expect("read Files PostgreSQL adapter");
    for shared_mechanism in ["store_outbox", "store_audit", "store_idempotency"] {
        assert!(
            postgres.contains(shared_mechanism),
            "Files persistence bypassed shared {shared_mechanism} mechanism"
        );
    }
    for forbidden in [
        "content_bytes",
        "storage_provider_config",
        "scanner_provider_config",
        "user_file_cleanup_queue",
    ] {
        assert!(
            !postgres.contains(forbidden),
            "Files persistence introduced duplicate authority {forbidden}"
        );
    }

    let presentation = [
        "files/presentation/controller.rs",
        "files/presentation/mod.rs",
    ]
    .into_iter()
    .map(|relative| {
        std::fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"))
    })
    .collect::<String>();
    for forbidden in ["/upload", "Vec<u8>", "IUserFileObjectStore"] {
        assert!(
            !presentation.contains(forbidden),
            "Files public presentation introduced buffered content authority {forbidden}"
        );
    }

    let app = std::fs::read_to_string(src.join("app.rs")).expect("read production composition");
    let production_app = production_source(&app);
    assert_eq!(
        production_app
            .matches("UserFileApplicationService::new")
            .count(),
        1
    );
    assert_eq!(
        production_app
            .matches("SharedUserFileObjectStore::from_client")
            .count(),
        1
    );
    assert!(production_app.contains(".subnamespace(\"user-files\")"));

    let conformance = std::fs::read_to_string(src.join("conformance.rs"))
        .expect("read non-default persistence conformance assembly");
    for owner_port in [
        "pub repository: Arc<dyn IUserFileRepository>",
        "pub objects: Arc<dyn IUserFileObjectStore>",
    ] {
        assert!(
            conformance.contains(owner_port),
            "Files persistence conformance lost owner port {owner_port}"
        );
    }
    for adapter in [
        "PostgresUserFileRepository::new",
        "SharedUserFileObjectStore::local",
    ] {
        assert_eq!(
            conformance.matches(adapter).count(),
            1,
            "Files persistence conformance must compose the one production adapter {adapter}"
        );
    }
    assert!(conformance
        .contains("pub fn user_file_organization_access_for_conformance() -> UserFileAccess"));
    let conformance_test = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/user_files.rs"),
    )
    .expect("read Files persistence conformance test");
    assert_eq!(
        conformance_test
            .matches("access: user_file_organization_access_for_conformance()")
            .count(),
        7,
        "every external Files transition must use the test-only projection factory"
    );
    for forbidden in ["ResourceAccessEvaluator", "resource_access:"] {
        assert!(
            !conformance_test.contains(forbidden),
            "Files persistence conformance bypassed its projection with {forbidden}"
        );
    }
    for line in conformance
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub "))
    {
        assert!(
            !line.contains("PostgresUserFileRepository")
                && !line.contains("SharedUserFileObjectStore")
                && !line.starts_with("pub use "),
            "Files persistence conformance exposed a concrete adapter: {line}"
        );
    }

    let lib = std::fs::read_to_string(src.join("lib.rs")).expect("read crate facade");
    assert!(lib.contains(
        "#[cfg(feature = \"persistence-conformance\")]\n#[doc(hidden)]\npub mod conformance;"
    ));
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .expect("read control-plane manifest");
    assert!(manifest.contains("persistence-conformance = []"));
    assert!(!manifest.lines().any(|line| {
        line.trim_start().starts_with("default") && line.contains("persistence-conformance")
    }));
    let ci = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/ci.yml"),
    )
    .expect("read CI workflow");
    let user_file_gate = workflow_step(
        &ci,
        "Certify UserFile lifecycle and organization quota persistence",
    );
    assert_eq!(
        user_file_gate
            .matches("--features persistence-conformance")
            .count(),
        1,
        "the non-default Files conformance assembly must stay confined to its retained gate"
    );
    assert!(user_file_gate
        .contains("postgres_user_files_are_quota_atomic_replay_safe_and_lifecycle_fenced"));

    let migration = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations/170_user_files.sql"),
    )
    .expect("read Files migration");
    for forbidden in [
        "content_bytes",
        "storage_provider_config",
        "scanner_provider_config",
        "create table user_file_cleanup",
    ] {
        assert!(
            !migration.contains(forbidden),
            "Files migration introduced duplicate authority {forbidden}"
        );
    }
}

#[test]
fn artifacts_application_and_presentation_do_not_import_fleet_authority() {
    let mut violations = BTreeSet::new();

    visit_production_sources(|relative, source| {
        if context(relative) != Some("artifacts")
            || !matches!(layer(relative), Some("application" | "presentation"))
        {
            return;
        }
        if source.contains("crate::modules::{") || source.contains("crate::{") {
            violations.insert(format!(
                "{} uses a grouped module root that bypasses exact boundary inspection",
                display(relative)
            ));
        }
        if source.lines().any(|line| {
            let line = line.trim();
            line == "use crate::modules;" || line.starts_with("use crate::modules as ")
        }) {
            violations.insert(format!(
                "{} aliases the module root and bypasses exact boundary inspection",
                display(relative)
            ));
        }
        for line in source
            .lines()
            .filter(|line| line.contains("crate::modules::fleet"))
        {
            violations.insert(format!("{} contains {line:?}", display(relative)));
        }
    });

    assert!(
        violations.is_empty(),
        "Artifacts imported Fleet logs, placement, or response DTOs instead of its owner log-query port:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn artifacts_application_never_reaches_into_assets() {
    let mut violations = BTreeSet::new();

    visit_production_sources(|relative, source| {
        if context(relative) != Some("artifacts") || layer(relative) != Some("application") {
            return;
        }
        for line in source
            .lines()
            .filter(|line| line.contains("crate::modules::assets"))
        {
            violations.insert(format!("{} contains {line:?}", display(relative)));
        }
    });

    assert!(
        violations.is_empty(),
        "Artifacts Application attempted to coordinate or mutate Assets instead of publishing an owner fact:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn artifacts_finalization_never_mutates_asset_storage() {
    let source = std::fs::read_to_string(
        module_root().join("artifacts/infrastructure/persistence/postgres.rs"),
    )
    .expect("read Artifacts PostgreSQL persistence");
    let forbidden = [
        "crate::modules::assets",
        "update asset_releases",
        "insert into asset_releases",
        "delete from asset_releases",
        "persist_release_transition",
        "plan_hosted_release",
        "apply_hosted_release",
    ];
    let violations = forbidden
        .into_iter()
        .filter(|needle| source.contains(needle))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "Artifacts finalization regained foreign Asset write authority: {}",
        violations.join(", ")
    );
}

#[test]
fn artifacts_finalization_has_no_consumer_rejection_protocol() {
    let files = [
        "artifacts/domain/repositories/build_run_repository.rs",
        "artifacts/infrastructure/build_flow/steps/validation.rs",
        "artifacts/infrastructure/persistence/postgres.rs",
        "artifacts/infrastructure/persistence/in_memory.rs",
    ];
    let forbidden = [
        "enum BuildRunFinalization {",
        "BuildRunFinalization::Rejected",
        "rejected hosted build",
        "hosted release rejection",
    ];
    let mut violations = BTreeSet::new();

    for relative in files {
        let source = std::fs::read_to_string(module_root().join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        for needle in forbidden {
            if source.contains(needle) {
                violations.insert(format!("{relative} contains {needle:?}"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Artifacts finalization regained a consumer-owned rejection or compensation protocol:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn assets_domain_enters_artifacts_only_through_published_language() {
    const ARTIFACTS_PREFIX: &str = "crate::modules::artifacts::";
    const PUBLISHED_PREFIX: &str = "crate::modules::artifacts::published::";
    let mut violations = BTreeSet::new();

    visit_production_sources(|relative, source| {
        if context(relative) != Some("assets") || layer(relative) != Some("domain") {
            return;
        }
        for line in source
            .lines()
            .filter(|line| line.contains(ARTIFACTS_PREFIX))
        {
            if !line.contains(PUBLISHED_PREFIX) {
                violations.insert(format!("{} contains {line:?}", display(relative)));
            }
        }
    });

    assert!(
        violations.is_empty(),
        "Assets Domain imported an Artifacts aggregate or implementation instead of its Published Language:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn workloads_domain_never_imports_artifacts_aggregates() {
    let mut violations = BTreeSet::new();

    visit_production_sources(|relative, source| {
        if context(relative) != Some("workloads") || layer(relative) != Some("domain") {
            return;
        }
        for line in source
            .lines()
            .filter(|line| line.contains("crate::modules::artifacts"))
        {
            violations.insert(format!("{} contains {line:?}", display(relative)));
        }
    });

    assert!(
        violations.is_empty(),
        "Workloads Domain imported Artifacts instead of receiving a Workloads-owned admission value:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn developer_workflows_domain_uses_only_local_models_shared_kernel_or_published_language() {
    let violations = bounded_context_internal_model_references("developer_workflows", "domain");

    assert!(
        violations.is_empty(),
        "Developer Workflows Domain imported a foreign owner model instead of local proposal language, an exact Shared Kernel reference, or Published Language:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn developer_workflows_application_uses_only_local_models_ports_or_published_language() {
    let violations =
        bounded_context_internal_model_references("developer_workflows", "application");

    assert!(
        violations.is_empty(),
        "Developer Workflows Application imported a foreign owner model instead of local ports, Shared Kernel, or Published Language:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn developer_workflows_sources_owner_models_are_confined_to_two_query_adapters() {
    let preview_adapter_path = "developer_workflows/infrastructure/preview_source_subscription.rs";
    let mut source_owner_imports = BTreeSet::new();
    visit_production_sources(|relative, source| {
        if context(relative) == Some("developer_workflows")
            && source.contains("crate::modules::sources::domain")
        {
            source_owner_imports.insert(display(relative));
        }
    });

    assert_eq!(
        source_owner_imports,
        lines(
            "developer_workflows/infrastructure/preview_source_subscription.rs\n\
             developer_workflows/infrastructure/source_revision.rs",
        ),
        "Developer Workflows must translate Sources owner models only in its two read-only Infrastructure adapters"
    );

    let adapter = std::fs::read_to_string(module_root().join(preview_adapter_path))
        .expect("read Preview source-subscription adapter");
    let production = production_source(&adapter);
    let compact = production.split_whitespace().collect::<String>();
    for required in [
        "Arc<dynISourceSubscriptionRepository>",
        "implIPreviewSourceSubscriptionQueryPort",
        "GithubRepositorySubscription::restore",
        ".find(",
    ] {
        assert!(
            compact.contains(required),
            "Preview source-subscription adapter lost its read-only owner boundary {required}"
        );
    }
    for forbidden in [
        "CreateGithubRepositorySubscription",
        "DeactivateGithubRepositorySubscription",
        ".create(",
        ".list(",
        ".deactivate(",
        "Postgres",
        "IOutboxRepository",
        "IEventPublisher",
        "CommandHandler",
        "tokio::spawn",
    ] {
        assert!(
            !production.contains(forbidden),
            "Preview source-subscription adapter acquired a write, persistence, or delivery mechanism {forbidden}"
        );
    }
}

#[test]
fn developer_workflows_preview_projection_reuses_the_single_outbox_relay() {
    let mut projector_sites = BTreeSet::new();
    let mut duplicate_mechanisms = BTreeSet::new();
    const FORBIDDEN: &[&str] = &[
        "A3sEventPublisher",
        "IEventPublisher",
        "IOutboxRepository",
        "NatsProvider",
        "OutboxRelay",
        "retry_delay(",
        "tokio::spawn",
    ];

    visit_production_sources(|relative, source| {
        if context(relative) != Some("developer_workflows") {
            return;
        }
        if source.contains("impl IIntegrationEventProjector") {
            projector_sites.insert(display(relative));
        }
        for forbidden in FORBIDDEN {
            if source.contains(forbidden) {
                duplicate_mechanisms.insert(format!("{} contains {forbidden}", display(relative)));
            }
        }
    });

    assert_eq!(
        projector_sites,
        lines("developer_workflows/infrastructure/pull_request_preview_projector.rs"),
        "Developer Workflows must enter the existing Outbox Relay through one projector"
    );
    assert!(
        duplicate_mechanisms.is_empty(),
        "Developer Workflows introduced another publisher, relay, queue, worker, or retry mechanism:\n{}",
        duplicate_mechanisms
            .into_iter()
            .collect::<Vec<_>>()
            .join("\n")
    );

    let app = std::fs::read_to_string(
        module_root()
            .parent()
            .expect("src directory")
            .join("app.rs"),
    )
    .expect("read application composition");
    assert_eq!(
        app.matches("PullRequestPreviewProjector::new").count(),
        1,
        "all process roles must share one Preview projector composition path"
    );
    assert_eq!(
        app.matches("ProjectsPreviewEnvironmentAdapter::new")
            .count(),
        1,
        "all process roles must share one Projects Environment handoff adapter"
    );
}

#[test]
fn developer_workflows_projects_boundaries_are_confined_to_two_infrastructure_adapters() {
    let mut projects_imports = BTreeSet::new();
    visit_production_sources(|relative, source| {
        if context(relative) != Some("developer_workflows") {
            return;
        }
        if source.contains("crate::modules::projects") {
            projects_imports.insert(display(relative));
        }
    });

    assert_eq!(
        projects_imports,
        lines(
            "developer_workflows/infrastructure/authorization.rs\n\
             developer_workflows/infrastructure/preview_environment.rs",
        ),
        "Developer Workflows must reach Projects only through its authorization-read and Preview-lifecycle Infrastructure adapters"
    );

    let projector = std::fs::read_to_string(
        module_root().join("developer_workflows/infrastructure/pull_request_preview_projector.rs"),
    )
    .expect("read Preview projector");
    assert!(
        projector.contains("Arc<dyn IPreviewEnvironmentPort>")
            && !projector.contains("Option<Arc<dyn IPreviewEnvironmentPort>>")
            && projector.contains("crate::modules::developer_workflows::published"),
        "the production Preview projector must require the owner handoff boundary and consume the owner Published Language"
    );
}

#[test]
fn developer_workflows_workloads_admission_is_confined_to_one_infrastructure_adapter() {
    let adapter_path = "developer_workflows/infrastructure/service_profile.rs";
    let mut workloads_imports = BTreeSet::new();
    visit_production_sources(|relative, source| {
        if context(relative) == Some("developer_workflows")
            && source.contains("crate::modules::workloads")
        {
            workloads_imports.insert(display(relative));
        }
    });

    assert_eq!(
        workloads_imports,
        lines(adapter_path),
        "Developer Workflows must reach Workloads only through its consumer-owned Application port and one Infrastructure adapter"
    );

    let adapter = std::fs::read_to_string(module_root().join(adapter_path))
        .expect("read Workloads Service-profile adapter");
    let production_adapter = adapter
        .split("#[cfg(test)]")
        .next()
        .expect("production Workloads adapter source");
    let compact_adapter = production_adapter.split_whitespace().collect::<String>();
    assert!(
        compact_adapter
            .contains("implIServiceProfileAdmissionPortforWorkloadsServiceProfileAdapter")
            && production_adapter.contains("ServiceTemplate")
            && compact_adapter.contains("template.digest()"),
        "the Workloads adapter must implement the consumer port and use the owner's exact template validation/digest contract"
    );
    for forbidden in [
        "IWorkloadRepository",
        "CreateDeploymentBundle",
        "Deployment::create",
        "OperationRequest",
        "IOutboxRepository",
        "OutboxRelay",
        "tokio::spawn",
    ] {
        assert!(
            !production_adapter.contains(forbidden),
            "the component-only Workloads adapter introduced owner lifecycle or delivery mechanism {forbidden}"
        );
    }
}

#[test]
fn developer_workflows_executions_admission_is_confined_to_one_infrastructure_adapter() {
    let adapter_path = "developer_workflows/infrastructure/scheduled_task_profile.rs";
    let mut executions_imports = BTreeSet::new();
    visit_production_sources(|relative, source| {
        if context(relative) == Some("developer_workflows")
            && source.contains("crate::modules::executions")
        {
            executions_imports.insert(display(relative));
        }
    });

    assert_eq!(
        executions_imports,
        lines(adapter_path),
        "Developer Workflows must reach Executions only through its consumer-owned Application port and one Infrastructure adapter"
    );

    let adapter = std::fs::read_to_string(module_root().join(adapter_path))
        .expect("read Executions scheduled-Task adapter");
    let production_adapter = adapter
        .split("#[cfg(test)]")
        .next()
        .expect("production Executions adapter source");
    let compact_adapter = production_adapter.split_whitespace().collect::<String>();
    assert!(
        compact_adapter.contains(
            "implIScheduledTaskProfileAdmissionPortforExecutionsScheduledTaskProfileAdapter"
        ) && production_adapter.contains("ExecutionTemplate")
            && compact_adapter.contains("template.digest()"),
        "the Executions adapter must implement the consumer port and use the owner's exact template validation/digest contract"
    );
    for forbidden in [
        "IExecutionRepository",
        "IExecutionTemplateRepository",
        "CreateExecutionTemplateCommand",
        "CreateExecutionHandler",
        "Execution::create",
        "OperationRequest",
        "IOutboxRepository",
        "OutboxRelay",
        "tokio::spawn",
    ] {
        assert!(
            !production_adapter.contains(forbidden),
            "the component-only Executions adapter introduced owner lifecycle or delivery mechanism {forbidden}"
        );
    }
}

#[test]
fn developer_workflows_artifacts_outcome_handoff_has_one_anti_corruption_adapter() {
    let adapter_path = "developer_workflows/infrastructure/build_outcome.rs";
    let mut artifacts_imports = BTreeSet::new();
    visit_production_sources(|relative, source| {
        if context(relative) == Some("developer_workflows")
            && source.contains("crate::modules::artifacts")
        {
            artifacts_imports.insert(display(relative));
        }
    });

    assert_eq!(
        artifacts_imports,
        lines(adapter_path),
        "Developer Workflows must consume Artifacts only through one Infrastructure anti-corruption adapter"
    );

    let adapter = std::fs::read_to_string(module_root().join(adapter_path))
        .expect("read Artifacts build-outcome adapter");
    let production_adapter = production_source(&adapter);
    let compact_adapter = production_adapter.split_whitespace().collect::<String>();
    assert!(
        compact_adapter.contains(
            "implIWorkloadBuildOutcomePortforArtifactsWorkloadBuildOutcomeAdapter"
        ) && production_adapter.contains("Arc<dyn IExternalSourceBuildOutcomeQueryPort>")
            && production_adapter.contains("Arc<dyn IBuildPlanRepository>"),
        "the Artifacts adapter must implement the consumer port and combine only the owner outcome query with the local accepted-plan authority"
    );
    for forbidden in [
        "crate::modules::artifacts::domain",
        "crate::modules::artifacts::infrastructure",
        "IBuildRunRepository",
        "BuildRunStatus",
        "BuildEvidence",
        "IOutboxRepository",
        "IIntegrationEventProjector",
        "OperationRequest",
        "CommandHandler",
        "tokio::spawn",
    ] {
        assert!(
            !production_adapter.contains(forbidden),
            "the component-only Artifacts adapter imported owner lifecycle or duplicate delivery mechanism {forbidden}"
        );
    }

    let owner_fact = std::fs::read_to_string(
        module_root().join("artifacts/published/external_source_build_outcome.rs"),
    )
    .expect("read Artifacts external-source build fact");
    let owner_query = std::fs::read_to_string(
        module_root().join("artifacts/application/external_source_build_outcome.rs"),
    )
    .expect("read Artifacts external-source build query");
    let production_query = production_source(&owner_query);
    for forbidden in [
        "developer_workflows",
        "BuildPlan",
        "IOutboxRepository",
        "IIntegrationEventProjector",
        "CommandHandler",
        "tokio::spawn",
    ] {
        assert!(
            !owner_fact.contains(forbidden) && !production_query.contains(forbidden),
            "the Artifacts owner fact/query adopted consumer or duplicate lifecycle vocabulary {forbidden}"
        );
    }
}

#[test]
fn developer_workflows_accepted_profile_compilation_keeps_one_read_only_interface_boundary() {
    let source = std::fs::read_to_string(
        module_root().join("developer_workflows/application/accepted_profile_compilation.rs"),
    )
    .expect("read accepted workload-profile compilation query");
    let production = production_source(&source);
    let compact = production.split_whitespace().collect::<String>();

    for required in [
        "Arc<dynIBuildPlanRepository>",
        "Arc<dynIWorkloadProfileRepository>",
        "Arc<WorkloadProfileCompilationService>",
        "implQueryHandler<CompileAcceptedWorkloadProfile>",
    ] {
        assert!(
            compact.contains(required),
            "accepted-profile compilation lost its local Application interface boundary {required}"
        );
    }

    for forbidden in [
        "crate::modules::artifacts",
        "crate::modules::workloads",
        "crate::modules::executions",
        "Postgres",
        "IBuildRunRepository",
        "IWorkloadRepository",
        "IExecutionRepository",
        "IOutboxRepository",
        "IIntegrationEventProjector",
        "CommandHandler",
        "tokio::spawn",
    ] {
        assert!(
            !production.contains(forbidden),
            "accepted-profile compilation imported foreign owner internals or duplicate lifecycle mechanism {forbidden}"
        );
    }
}

#[test]
fn developer_workflows_build_plan_detection_query_keeps_concrete_detectors_out_of_application() {
    let source = std::fs::read_to_string(
        module_root().join("developer_workflows/application/build_plan_detection_query.rs"),
    )
    .expect("read BuildPlan detection query");
    let production = production_source(&source);
    let compact = production.split_whitespace().collect::<String>();

    assert!(
        compact.contains("Arc<BuildPlanDetectionService>")
            && compact.contains("Arc<dynIBuildPlanSourceLayoutPort>")
            && compact.contains("Arc<dynIDeveloperWorkflowAuthorizationPort>")
            && compact.contains("authorize_environment_action(")
            && compact.contains("implQueryHandler<DetectBuildPlanProposals>"),
        "BuildPlan detection must enter Application through authorization, one source-layout port, and the local detector service"
    );
    for forbidden in [
        "AssetAclBuildPlanDetector",
        "DockerfileBuildPlanDetector",
        "crate::modules::assets",
        "crate::modules::sources",
        "Repository",
        "Postgres",
        "CommandHandler",
        "IOutboxRepository",
        "IIntegrationEventProjector",
        "tokio::spawn",
    ] {
        assert!(
            !production.contains(forbidden),
            "BuildPlan detection query imported a concrete adapter or lifecycle mechanism {forbidden}"
        );
    }
}

#[test]
fn developer_workflows_build_plan_reads_have_one_application_authority() {
    let query = std::fs::read_to_string(
        module_root().join("developer_workflows/application/build_plan_queries.rs"),
    )
    .expect("read accepted BuildPlan queries");
    let production_query = production_source(&query);
    let compact_query = production_query.split_whitespace().collect::<String>();

    for required in [
        "pubstructBuildPlanQueryService",
        "plans:Arc<dynIBuildPlanRepository>",
        "authorization:Arc<dynIDeveloperWorkflowAuthorizationPort>",
        "DeveloperWorkflowAction::ReadBuildPlan",
        "implQueryHandler<GetAcceptedBuildPlan>",
        "implQueryHandler<ListAcceptedBuildPlans>",
    ] {
        assert!(
            compact_query.contains(required),
            "accepted BuildPlan reads lost their single Application authority {required}"
        );
    }
    for forbidden in [
        "crate::modules::identity",
        "crate::modules::projects",
        "crate::modules::sources",
        "ResourceAccessEvaluator",
        "Postgres",
        "InMemory",
        "CommandBus",
        "CommandHandler",
        "IOutboxRepository",
        "IIntegrationEventProjector",
        "tokio::spawn",
    ] {
        assert!(
            !production_query.contains(forbidden),
            "accepted BuildPlan read authority imported foreign policy, a concrete adapter, or lifecycle mechanism {forbidden}"
        );
    }

    let controller = std::fs::read_to_string(
        module_root().join("developer_workflows/presentation/controller.rs"),
    )
    .expect("read Developer Workflows controller");
    let production_controller = production_source(&controller);
    for required in [
        "DetectBuildPlanProposals",
        "GetAcceptedBuildPlan",
        "ListAcceptedBuildPlans",
        ".execute(",
    ] {
        assert!(
            production_controller.contains(required),
            "Developer Workflows presentation stopped dispatching the Application boundary {required}"
        );
    }
    for forbidden in [
        "IBuildPlanRepository",
        "developer_workflows::infrastructure",
        "authorize_environment_action",
        "IMembershipRepository",
        "IResourceGrantRepository",
        ".find(",
        ".list_for_source(",
        "Postgres",
        "InMemory",
    ] {
        assert!(
            !production_controller.contains(forbidden),
            "Developer Workflows presentation bypassed its Application boundary with {forbidden}"
        );
    }

    let management_mcp = std::fs::read_to_string(
        module_root()
            .parent()
            .expect("control-plane source root")
            .join("presentation/management_mcp/developer_workflows.rs"),
    )
    .expect("read Developer Workflows Management MCP adapter");
    let production_management_mcp = production_source(&management_mcp);
    for required in [
        "DetectBuildPlanProposals",
        "AcceptBuildPlan",
        "GetAcceptedBuildPlan",
        "ListAcceptedBuildPlans",
        ".execute(",
    ] {
        assert!(
            production_management_mcp.contains(required),
            "Developer Workflows Management MCP stopped dispatching the Application boundary {required}"
        );
    }
    for forbidden in [
        "IBuildPlanRepository",
        "developer_workflows::infrastructure",
        "authorize_environment_action",
        "IMembershipRepository",
        "IResourceGrantRepository",
        ".find(",
        ".list_for_source(",
        "Postgres",
        "InMemory",
    ] {
        assert!(
            !production_management_mcp.contains(forbidden),
            "Developer Workflows Management MCP bypassed its Application boundary with {forbidden}"
        );
    }
}

#[test]
fn developer_workflows_workload_profile_public_surface_has_one_application_authority() {
    let query = std::fs::read_to_string(
        module_root().join("developer_workflows/application/workload_profile_queries.rs"),
    )
    .expect("read accepted WorkloadProfile queries");
    let production_query = production_source(&query);
    let compact_query = production_query.split_whitespace().collect::<String>();
    for required in [
        "pubstructWorkloadProfileQueryService",
        "profiles:Arc<dynIWorkloadProfileRepository>",
        "authorization:Arc<dynIDeveloperWorkflowAuthorizationPort>",
        "DeveloperWorkflowAction::ReadWorkloadProfile",
        "implQueryHandler<GetCurrentAcceptedWorkloadProfileRevision>",
        "implQueryHandler<GetAcceptedWorkloadProfileRevision>",
        "implQueryHandler<ListAcceptedWorkloadProfileRevisions>",
    ] {
        assert!(
            compact_query.contains(required),
            "accepted WorkloadProfile reads lost their single Application authority {required}"
        );
    }
    for forbidden in [
        "crate::modules::identity",
        "crate::modules::projects",
        "crate::modules::workloads",
        "crate::modules::executions",
        "ResourceAccessEvaluator",
        "Postgres",
        "InMemory",
        "CommandBus",
        "CommandHandler",
        "IOutboxRepository",
        "IIntegrationEventProjector",
        "tokio::spawn",
    ] {
        assert!(
            !production_query.contains(forbidden),
            "accepted WorkloadProfile read authority imported foreign policy, a concrete adapter, or lifecycle mechanism {forbidden}"
        );
    }

    let controller = std::fs::read_to_string(
        module_root().join("developer_workflows/presentation/workload_profile_controller.rs"),
    )
    .expect("read WorkloadProfile controller");
    let production_controller = production_source(&controller);
    for required in [
        "AcceptWorkloadProfile",
        "GetCurrentAcceptedWorkloadProfileRevision",
        "GetAcceptedWorkloadProfileRevision",
        "ListAcceptedWorkloadProfileRevisions",
        ".execute(",
    ] {
        assert!(
            production_controller.contains(required),
            "WorkloadProfile presentation stopped dispatching the Application boundary {required}"
        );
    }
    for forbidden in [
        "IWorkloadProfileRepository",
        "developer_workflows::infrastructure",
        "authorize_environment_action",
        "IMembershipRepository",
        "IResourceGrantRepository",
        "WorkloadProfileContract::parse_acl",
        "a3s_acl",
        ".find_current(",
        ".find_revision(",
        ".list_revisions(",
        "Postgres",
        "InMemory",
    ] {
        assert!(
            !production_controller.contains(forbidden),
            "WorkloadProfile presentation bypassed its Application boundary with {forbidden}"
        );
    }

    let management_mcp = std::fs::read_to_string(
        module_root()
            .parent()
            .expect("control-plane source root")
            .join("presentation/management_mcp/developer_workflows.rs"),
    )
    .expect("read Developer Workflows Management MCP adapter");
    let production_management_mcp = production_source(&management_mcp);
    for required in [
        "AcceptWorkloadProfile",
        "GetCurrentAcceptedWorkloadProfileRevision",
        "GetAcceptedWorkloadProfileRevision",
        "ListAcceptedWorkloadProfileRevisions",
        ".execute(",
    ] {
        assert!(
            production_management_mcp.contains(required),
            "WorkloadProfile Management MCP stopped dispatching the Application boundary {required}"
        );
    }
    for forbidden in [
        "IWorkloadProfileRepository",
        "developer_workflows::infrastructure",
        "authorize_environment_action",
        "IMembershipRepository",
        "IResourceGrantRepository",
        "WorkloadProfileContract::parse_acl",
        "a3s_acl",
        ".find_current(",
        ".find_revision(",
        ".list_revisions(",
        "Postgres",
        "InMemory",
    ] {
        assert!(
            !production_management_mcp.contains(forbidden),
            "WorkloadProfile Management MCP bypassed its Application boundary with {forbidden}"
        );
    }
}

#[test]
fn workload_identity_foundation_has_one_acl_owner_and_no_parallel_runtime_or_secret_mechanism() {
    let root = module_root();
    let trust_path = "identity/domain/value_objects/trust_domain_contract.rs";
    let policy_path = "identity/domain/value_objects/workload_identity_policy_contract.rs";
    let revision_path = "identity/domain/entities/workload_identity.rs";
    let repository_path = "identity/domain/repositories/workload_identity_repository.rs";
    let provider_path = "identity/domain/services/workload_identity_provider.rs";
    let provider_profile_path =
        "identity/domain/value_objects/workload_identity_provider_profile.rs";
    let provider_adapter_path =
        "identity/infrastructure/spiffe_https_web_workload_identity_provider.rs";

    let trust = std::fs::read_to_string(root.join(trust_path)).expect("read trust-domain ACL");
    let policy =
        std::fs::read_to_string(root.join(policy_path)).expect("read workload identity policy ACL");
    let revisions =
        std::fs::read_to_string(root.join(revision_path)).expect("read identity revisions");
    let repositories =
        std::fs::read_to_string(root.join(repository_path)).expect("read identity repositories");
    let provider =
        std::fs::read_to_string(root.join(provider_path)).expect("read identity provider port");
    let provider_profile = std::fs::read_to_string(root.join(provider_profile_path))
        .expect("read identity provider profile");
    let provider_adapter = std::fs::read_to_string(root.join(provider_adapter_path))
        .expect("read identity provider adapter");

    assert_eq!(trust.matches("pub struct TrustDomainContract {").count(), 1);
    assert_eq!(
        policy
            .matches("pub struct WorkloadIdentityPolicyContract {")
            .count(),
        1
    );
    for required in [
        "a3s_acl",
        "canonical_digest",
        "parse_acl",
        "generate_acl",
        "cloud.identity.trust-domain.v1",
    ] {
        assert!(
            trust.contains(required),
            "trust-domain contract lost canonical ACL boundary {required}"
        );
    }
    for required in [
        "a3s_acl",
        "a3s_cloud_contracts::{RuntimeIsolationLevel, RuntimeUnitClass}",
        "cloud.identity.workload-policy.v1",
        "validate_against_trust_domain",
    ] {
        assert!(
            policy.contains(required),
            "workload identity contract lost unified boundary {required}"
        );
    }
    for forbidden in [
        "RuntimeTask",
        "RuntimeService",
        "AgentRuntime",
        "FunctionRuntime",
        "CellRuntime",
        "private_key",
        "certificate_pem",
        "serde_yaml",
        "toml::",
    ] {
        assert!(
            !production_source(&format!("{trust}\n{policy}")).contains(forbidden),
            "Identity ACL introduced duplicate runtime, secret, or configuration mechanism {forbidden}"
        );
    }

    for required in [
        "a3s_acl",
        "canonical_digest",
        "parse_acl",
        "generate_acl",
        "cloud.identity.workload-provider.v1",
    ] {
        assert!(
            provider_profile.contains(required),
            "provider profile lost canonical ACL boundary {required}"
        );
    }
    for forbidden in ["crate::config", "serde_yaml", "toml::", "private_key"] {
        assert!(
            !production_source(&format!("{provider_profile}\n{provider_adapter}"))
                .contains(forbidden),
            "provider profile or adapter acquired root configuration or secret authority {forbidden}"
        );
    }

    assert_eq!(
        revisions
            .matches("pub struct AcceptedTrustDomainRevision {")
            .count(),
        1
    );
    assert_eq!(
        revisions
            .matches("pub struct AcceptedWorkloadIdentityPolicyRevision {")
            .count(),
        1
    );
    assert!(revisions.contains("Uuid::new_v5"));
    assert_eq!(
        repositories
            .matches("pub trait ITrustDomainRepository")
            .count(),
        1
    );
    assert_eq!(
        repositories
            .matches("pub trait IWorkloadIdentityPolicyRepository")
            .count(),
        1
    );
    for forbidden in ["Postgres", "InMemory", "reqwest", "redis", "a3s_lane"] {
        assert!(
            !production_source(&repositories).contains(forbidden),
            "Identity repository port imported concrete mechanism {forbidden}"
        );
    }

    assert_eq!(
        provider
            .matches("pub trait IWorkloadIdentityProviderService")
            .count(),
        1
    );
    assert!(provider.contains("async fn inspect("));
    assert!(provider.contains("observed_federation_bundle_digests"));
    assert!(provider.contains("observed_identity_formats"));
    assert!(provider.contains("declared_node_attestation_profile_digests"));
    assert!(provider.contains("declared_max_credential_lifetime_seconds"));
    for forbidden in [
        "async fn issue",
        "private_key",
        "certificate_pem",
        "supports_federation",
        "RuntimeUnitSpec",
        "NodeId",
        "Postgres",
        "reqwest",
    ] {
        assert!(
            !production_source(&provider).contains(forbidden),
            "WI1 provider inspection port prematurely acquired WI2/WI3 or concrete authority {forbidden}"
        );
    }
}

#[test]
fn workload_trust_persistence_reuses_one_atomic_identity_authority() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repository = std::fs::read_to_string(
        manifest.join("src/modules/identity/domain/repositories/workload_identity_repository.rs"),
    )
    .expect("read workload trust repository ports");
    let contract = std::fs::read_to_string(
        manifest
            .join("src/modules/identity/domain/value_objects/workload_identity_policy_contract.rs"),
    )
    .expect("read workload identity policy contract");
    let persistence = std::fs::read_to_string(
        manifest.join("src/modules/identity/infrastructure/persistence/postgres_workload_trust.rs"),
    )
    .expect("read workload trust PostgreSQL adapter");
    let compact_persistence = persistence
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    let in_memory = std::fs::read_to_string(manifest.join(
        "src/modules/identity/infrastructure/persistence/in_memory_privileged_management.rs",
    ))
    .expect("read fail-closed in-memory privileged adapter");
    let migration =
        std::fs::read_to_string(manifest.join("../../migrations/179_workload_trust_authority.sql"))
            .expect("read workload trust authority migration");

    assert!(contract.contains("pub trust_domain_revision_id: TrustDomainRevisionId"));
    for required in [
        "pub actor_principal_id: PrincipalId",
        "pub credential_id: ApiTokenId",
        "pub request_id: Uuid",
        "ReadCurrentTrustDomain",
        "ReadCurrentWorkloadIdentityPolicyForWorkload",
    ] {
        assert!(
            repository.contains(required),
            "workload trust port lost closed authority context {required}"
        );
    }
    assert!(!repository.contains("actor_is_platform_admin"));

    assert_eq!(
        persistence.matches("impl ITrustDomainRepository").count(),
        1
    );
    assert_eq!(
        persistence
            .matches("impl IWorkloadIdentityPolicyRepository")
            .count(),
        1
    );
    for required in [
        "lock_installation(transaction",
        "issue_privileged_authorization(",
        "PlatformPermission::WorkloadTrustManage",
        "PlatformPermission::WorkloadTrustRead",
        "store_outbox(",
        "store_audit(",
        "store_idempotency(",
    ] {
        assert!(
            persistence.contains(required),
            "workload trust persistence lost atomic shared rail {required}"
        );
    }
    for required in [
        "idempotency_replay::<AcceptedTrustDomainRevision",
        "idempotency_replay::<AcceptedWorkloadIdentityPolicyRevision",
    ] {
        assert!(
            compact_persistence.contains(required),
            "workload trust persistence lost formatting-independent atomic rail {required}"
        );
    }
    for forbidden in [
        "actor_is_platform_admin",
        "create table",
        "redis",
        "a3s_lane",
        "distributed_lock",
        "new Authorization",
    ] {
        assert!(
            !production_source(&persistence).contains(forbidden),
            "workload trust adapter introduced a parallel authority through {forbidden}"
        );
    }

    assert!(in_memory.contains("impl ITrustDomainRepository for InMemoryIdentityRepository"));
    assert!(
        in_memory.contains("impl IWorkloadIdentityPolicyRepository for InMemoryIdentityRepository")
    );
    assert!(in_memory.contains("privileged management requires the PostgreSQL Identity authority"));

    for required in [
        "create table trust_domain_revisions",
        "create table trust_domain_heads",
        "create table workload_identity_policy_revisions",
        "create table workload_identity_policy_heads",
        "for update of installation",
        "references organizations",
        "references workloads",
        "references node_pools",
    ] {
        assert!(
            migration.contains(required),
            "workload trust migration lost invariant {required}"
        );
    }
    for forbidden in [
        "workload_trust_audit",
        "workload_trust_outbox",
        "workload_trust_idempotency",
        "workload_trust_locks",
        "redis",
        "a3s_lane",
    ] {
        assert!(
            !migration.contains(forbidden),
            "workload trust migration duplicated a shared mechanism through {forbidden}"
        );
    }
}

#[test]
fn workload_runtime_evidence_is_one_non_authorizing_identity_projection() {
    let evidence = std::fs::read_to_string(
        module_root().join("identity/domain/entities/workload_runtime_evidence_binding.rs"),
    )
    .expect("read workload Runtime evidence binding");
    let production = production_source(&evidence);
    for required in [
        "cloud.identity.workload-runtime-evidence-binding.v1",
        "pub struct WorkloadRuntimeEvidenceCandidate",
        "pub struct WorkloadRuntimeEvidenceBinding",
        "pub resource_claim_digest: Sha256Digest",
        "pub resource_claim_aggregate_version: u64",
        "pub node_pool_aggregate_version: u64",
        "pub node_aggregate_version: u64",
        "pub node_capabilities_digest: Sha256Digest",
        "pub identity_attachment_digest: Sha256Digest",
        "pub runtime_attestation_binding_digest: Sha256Digest",
        "pub node_attestation_binding_digest: Option<Sha256Digest>",
        "Uuid::new_v5",
        "pub const fn authorizes_credential_issuance(&self) -> bool",
    ] {
        assert!(
            evidence.contains(required),
            "WI2-C1 evidence lost closed binding invariant {required}"
        );
    }
    assert!(production.contains("self.node_attestation_binding_digest.is_some()"));
    assert!(production.contains("false"));
    assert_eq!(
        production
            .matches("pub struct WorkloadRuntimeEvidenceBinding {")
            .count(),
        1,
        "Identity acquired a second Runtime evidence projection"
    );
    for forbidden in [
        "crate::modules::workloads",
        "crate::modules::fleet",
        "a3s_runtime::",
        "a3s_box_runtime::",
        "Postgres",
        "InMemory",
        "redis",
        "a3s_lane",
        "tokio::",
        "async fn",
        "private_key",
        "certificate_pem",
    ] {
        assert!(
            !production.contains(forbidden),
            "WI2-C1 domain evidence imported a foreign lifecycle or mechanism {forbidden}"
        );
    }
}

#[test]
fn workload_runtime_evidence_uses_one_owner_port_chain_and_one_identity_adapter() {
    let root = module_root();
    let identity_port =
        std::fs::read_to_string(root.join("identity/application/workload_runtime_evidence.rs"))
            .expect("read Identity workload Runtime evidence port");
    let adapter =
        std::fs::read_to_string(root.join("identity/infrastructure/workload_runtime_evidence.rs"))
            .expect("read Identity workload Runtime evidence adapter");
    let workload_query =
        std::fs::read_to_string(root.join("workloads/application/bound_runtime_claim.rs"))
            .expect("read Workloads bound Runtime Claim query");
    let fleet_query =
        std::fs::read_to_string(root.join("fleet/application/runtime_node_evidence.rs"))
            .expect("read Fleet Runtime Node evidence query");
    let production_port = production_source(&identity_port);
    let production_adapter = production_source(&adapter);
    let compact_adapter = production_adapter.split_whitespace().collect::<String>();

    assert!(production_port.contains("pub trait IWorkloadRuntimeEvidenceCandidatePort"));
    assert!(!production_port.contains("crate::modules::workloads"));
    assert!(!production_port.contains("crate::modules::fleet"));
    assert!(
        workload_query.contains("list_deployment_replica_member_bindings")
            && !workload_query.contains("find_deployment_replica_binding")
            && workload_query.contains("IWorkloadPlacementGroupRepository")
            && workload_query.contains("find_placement_group_for_replica_generation")
            && workload_query.contains("project_placement_group_runtime_spec_with_execution"),
        "Workloads evidence must resolve the exact replica member for both ordinary and placement-group Deployments"
    );
    assert!(
        workload_query
            .contains("R: IWorkloadRepository + IWorkloadPlacementGroupRepository + 'static")
            && fleet_query.contains(
                "R: INodePoolRepository + INodeRepository + INodeControlRepository + 'static"
            ),
        "Fleet must share one concrete repository and Workloads must share its Workload/placement-group repository without collapsing the separate Claim aggregate"
    );
    assert!(
        workload_query.contains("require_stable_owner_snapshot")
            && workload_query.contains("current_claim != *claim")
            && fleet_query.contains("require_stable_owner_snapshot")
            && fleet_query.contains("current_record != *record"),
        "owner facts must use an optimistic double collect instead of emitting a torn concurrent read"
    );
    assert!(
        compact_adapter.contains(
            "implIWorkloadRuntimeEvidenceCandidatePortforOwnerWorkloadRuntimeEvidenceAdapter"
        ) && production_adapter.contains("Arc<dyn IBoundRuntimeClaimQueryPort>")
            && production_adapter.contains("Arc<dyn IRuntimeNodeEvidenceQueryPort>")
            && production_adapter.contains("RuntimeConsumerRequirements")
            && production_adapter.contains("RuntimeAttestationBinding")
            && production_adapter.contains("fleet.organization_id() != request.organization_id()"),
        "Identity must combine only the two owner ports through Runtime's public admission contract"
    );
    for forbidden in [
        "crate::modules::workloads::domain",
        "crate::modules::workloads::infrastructure",
        "crate::modules::fleet::domain",
        "crate::modules::fleet::infrastructure",
        "IResourceClaimRepository",
        "INodeRepository",
        "INodeControlRepository",
        "Postgres",
        "InMemory",
        "redis",
        "a3s_lane",
        "tokio::spawn",
    ] {
        assert!(
            !production_adapter.contains(forbidden),
            "WI2-C2 adapter imported an owner lifecycle or duplicate mechanism {forbidden}"
        );
    }

    for (owner, source) in [("Workloads", workload_query), ("Fleet", fleet_query)] {
        let production = production_source(&source);
        assert!(
            !production.contains("crate::modules::identity"),
            "{owner} owner query adopted Identity consumer vocabulary"
        );
        for forbidden in [
            "IOutboxRepository",
            "IIntegrationEventProjector",
            "tokio::spawn",
            "redis",
            "a3s_lane",
        ] {
            assert!(
                !production.contains(forbidden),
                "{owner} owner query duplicated delivery or coordination through {forbidden}"
            );
        }
    }

    let node_control =
        std::fs::read_to_string(root.join("fleet/domain/repositories/node_control_repository.rs"))
            .expect("read Fleet Node control repository contract");
    let in_memory =
        std::fs::read_to_string(root.join("fleet/infrastructure/persistence/in_memory_control.rs"))
            .expect("read Fleet in-memory observation persistence");
    let postgres = std::fs::read_to_string(
        root.join("fleet/infrastructure/persistence/postgres/control/observations.rs"),
    )
    .expect("read Fleet PostgreSQL observation persistence");
    assert!(node_control.contains("pub agent_instance_id: Uuid"));
    assert!(in_memory.contains("received_at: stored.received_at"));
    assert!(postgres.contains(
        "select report_id, node_id, agent_instance_id, command_id, observed_at, received_at, observation"
    ));
}

#[test]
fn workload_runtime_evidence_history_is_one_typed_identity_authority() {
    let root = module_root();
    let entity = std::fs::read_to_string(
        root.join("identity/domain/entities/workload_runtime_evidence_binding.rs"),
    )
    .expect("read Runtime evidence entity");
    let repository = std::fs::read_to_string(
        root.join("identity/domain/repositories/workload_runtime_evidence_repository.rs"),
    )
    .expect("read Runtime evidence repository port");
    let recorder = std::fs::read_to_string(
        root.join("identity/application/workload_runtime_evidence_recorder.rs"),
    )
    .expect("read Runtime evidence recorder");
    let persistence = std::fs::read_to_string(
        root.join("identity/infrastructure/persistence/postgres_workload_runtime_evidence.rs"),
    )
    .expect("read Runtime evidence PostgreSQL adapter");
    let schema =
        std::fs::read_to_string(root.join(
            "identity/infrastructure/persistence/postgres_workload_runtime_evidence_schema.rs",
        ))
        .expect("read Runtime evidence ORM schema");
    let in_memory = std::fs::read_to_string(
        root.join("identity/infrastructure/persistence/in_memory_privileged_management.rs"),
    )
    .expect("read fail-closed Identity adapter");
    let migration = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../migrations/181_workload_runtime_evidence_history.sql"),
    )
    .expect("read Runtime evidence migration");

    assert_eq!(
        entity
            .matches("pub struct WorkloadRuntimeEvidenceRecord {")
            .count(),
        1
    );
    assert!(entity.contains("cloud.identity.workload-runtime-evidence-record.v1"));
    assert!(entity.contains("pub const fn authorizes_credential_issuance(&self) -> bool"));
    assert!(production_source(&entity).contains("false"));
    assert_eq!(
        repository
            .matches("pub trait IWorkloadRuntimeEvidenceRepository")
            .count(),
        1
    );
    assert!(repository.contains("cloud.identity.workload-runtime-evidence-admission.v1"));
    assert!(repository.contains("workload_runtime_evidence_idempotency("));
    for required in [
        "async fn replay_admission(",
        "async fn record(",
        "async fn read(",
        "async fn list_history(",
    ] {
        assert!(repository.contains(required));
    }

    let replay = recorder
        .find(".replay_admission(")
        .expect("historic replay gate");
    let current_policy = recorder
        .find(".read_current_for_runtime(")
        .expect("current policy gate");
    let owner_candidate = recorder
        .find(".read_candidate(")
        .expect("owner evidence gate");
    let commit = recorder.find(".record(").expect("history commit");
    assert!(
        replay < current_policy && current_policy < owner_candidate && owner_candidate < commit
    );

    assert_eq!(
        persistence
            .matches("impl IWorkloadRuntimeEvidenceRepository for PostgresIdentityRepository")
            .count(),
        1
    );
    for required in [
        "idempotency_replay::<WorkloadRuntimeEvidenceRecord>",
        "lock_installation_for_authorization",
        "load_current_runtime_policy_under_installation_fence",
        "advisory_xact_lock(",
        "insert_into::<WorkloadRuntimeEvidenceHistory>",
        "select_from::<WorkloadRuntimeEvidenceHistory>",
        "store_idempotency(",
    ] {
        assert!(
            persistence.contains(required),
            "Runtime evidence persistence lost {required}"
        );
    }
    assert!(!production_source(&persistence).contains("sql_query"));
    assert_eq!(schema.matches("orm_table!").count(), 1);
    assert_eq!(
        schema
            .matches("=> \"workload_runtime_evidence_history\"")
            .count(),
        1
    );
    assert!(in_memory
        .contains("impl IWorkloadRuntimeEvidenceRepository for InMemoryIdentityRepository"));

    for required in [
        "create table workload_runtime_evidence_history",
        "for key share of installation",
        "workload_identity_policy_heads",
        "trust_domain_heads",
        "history is immutable",
        "node_attestation_binding_digest is null",
        "interval '120 seconds'",
    ] {
        assert!(
            migration.to_ascii_lowercase().contains(required),
            "Runtime evidence migration lost {required}"
        );
    }
    assert_eq!(
        migration
            .to_ascii_lowercase()
            .matches("create table workload_runtime_evidence_history")
            .count(),
        1
    );
    for forbidden in [
        "create table resource_claim",
        "create table node",
        "create table runtime_unit",
        "create queue",
        "redis",
        "a3s_lane",
        "on delete cascade",
        "on delete set null",
    ] {
        assert!(
            !migration.to_ascii_lowercase().contains(forbidden),
            "Runtime evidence history duplicated an owner or mechanism through {forbidden}"
        );
    }
    for source in [&repository, &recorder, &persistence] {
        let production = production_source(source);
        for forbidden in ["redis", "a3s_lane", "tokio::spawn", "IOutboxRepository"] {
            assert!(
                !production.contains(forbidden),
                "Runtime evidence authority duplicated coordination through {forbidden}"
            );
        }
    }
}

#[test]
fn deployment_runtime_execution_has_one_owner_fact_one_acl_and_one_immutable_binding() {
    let root = module_root();
    let identity_fact = std::fs::read_to_string(
        root.join("identity/published/workload_runtime_execution_authorization.rs"),
    )
    .expect("read Identity Runtime execution owner fact");
    let identity_query = std::fs::read_to_string(
        root.join("identity/application/workload_runtime_execution_authorization.rs"),
    )
    .expect("read Identity Runtime execution owner query");
    let identity_persistence = std::fs::read_to_string(
        root.join("identity/infrastructure/persistence/postgres_workload_trust.rs"),
    )
    .expect("read Identity workload trust persistence");
    let admission =
        std::fs::read_to_string(root.join("workloads/application/runtime_execution_admission.rs"))
            .expect("read Workloads Runtime execution admission port");
    let adapter = std::fs::read_to_string(
        root.join("workloads/infrastructure/identity_runtime_execution_admission.rs"),
    )
    .expect("read Workloads Identity admission ACL");
    let binding = std::fs::read_to_string(
        root.join("workloads/domain/entities/runtime_execution_binding.rs"),
    )
    .expect("read Workloads Deployment Runtime binding");
    let repository =
        std::fs::read_to_string(root.join("workloads/domain/repositories/workload_repository.rs"))
            .expect("read Workloads repository port");
    let owner_query =
        std::fs::read_to_string(root.join("workloads/application/bound_runtime_claim.rs"))
            .expect("read Workloads bound Runtime Claim owner query");
    let ordinary =
        std::fs::read_to_string(root.join("workloads/infrastructure/deployment_flow/steps.rs"))
            .expect("read ordinary Deployment flow");
    let placement = std::fs::read_to_string(
        root.join("workloads/infrastructure/deployment_flow/placement_group_workflow_v2.rs"),
    )
    .expect("read placement-group Deployment flow v2");
    let reconciliation =
        std::fs::read_to_string(root.join("workloads/infrastructure/reconciliation/mod.rs"))
            .expect("read Workloads reconciliation");
    let in_memory =
        std::fs::read_to_string(root.join("workloads/infrastructure/persistence/in_memory.rs"))
            .expect("read in-memory Workloads persistence");
    let postgres_replicas = std::fs::read_to_string(
        root.join("workloads/infrastructure/persistence/postgres/replicas.rs"),
    )
    .expect("read PostgreSQL Workloads replica persistence");
    let application = std::fs::read_to_string(root.join("../app.rs"))
        .expect("read production application composition");
    let migration = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../migrations/180_deployment_runtime_execution_bindings.sql"),
    )
    .expect("read Deployment Runtime binding migration");

    for required in [
        "a3s.cloud.workload-runtime-execution-authorization.v1",
        "pub struct WorkloadRuntimeExecutionAuthorization",
        "workload_revision_id: WorkloadRevisionId",
        "node_pool_id: NodePoolId",
        "runtime_class: RuntimeUnitClass",
        "isolation_level: RuntimeIsolationLevel",
        "semantics_profile_digest: Sha256Digest",
        "identity_attachment_digest: Sha256Digest",
        "authorized_at: DateTime<Utc>",
    ] {
        assert!(
            identity_fact.contains(required),
            "Identity Runtime owner fact lost generic invariant {required}"
        );
    }
    for forbidden in [
        "WorkloadIdentityPolicyId",
        "WorkloadIdentityPolicyRevisionId",
        "credential_id",
        "private_key",
        "certificate",
        "crate::modules::workloads",
        "Postgres",
        "InMemory",
    ] {
        assert!(
            !production_source(&identity_fact).contains(forbidden),
            "Identity Runtime owner fact leaked lifecycle or infrastructure through {forbidden}"
        );
    }

    assert!(identity_query.contains("pub trait IWorkloadRuntimeExecutionAuthorizationQueryPort"));
    assert!(identity_query.contains("read_current_for_runtime"));
    let runtime_read = identity_persistence
        .split("async fn read_current_for_runtime(")
        .nth(1)
        .and_then(|source| source.split("async fn list_revisions(").next())
        .expect("isolate internal Runtime policy read");
    let installation_lock = runtime_read
        .find("lock_installation_for_authorization")
        .expect("Runtime policy read must fence Installation");
    let shared_runtime_read = identity_persistence
        .split("pub(super) async fn load_current_runtime_policy_under_installation_fence(")
        .nth(1)
        .and_then(|source| {
            source
                .split("async fn load_workload_policy_revision(")
                .next()
        })
        .expect("isolate shared Runtime policy read");
    let organization_lock = shared_runtime_read
        .find("for key share of organization")
        .expect("Runtime policy read must fence organization lineage");
    let trust_lock = shared_runtime_read
        .find("load_current_trust_domain")
        .expect("Runtime policy read must lock current TrustDomain");
    let policy_lock = shared_runtime_read
        .rfind("load_current_workload_policy(")
        .expect("Runtime policy read must lock current policy");
    assert!(installation_lock < runtime_read.len());
    assert!(organization_lock < trust_lock && trust_lock < policy_lock);
    assert!(runtime_read.contains("load_organization_installation_for_runtime"));
    assert!(runtime_read.contains("load_current_runtime_policy_under_installation_fence"));
    assert!(shared_runtime_read.contains("return Ok(None)"));
    assert!(admission.contains("pub trait IWorkloadRuntimeExecutionAdmissionPort"));
    assert!(!production_source(&admission).contains("crate::modules::identity"));
    assert!(adapter.contains("IWorkloadRuntimeExecutionAuthorizationQueryPort"));
    assert!(
        adapter.contains("current Identity policy does not authorize the exact Deployment lineage")
    );
    for forbidden in [
        "IWorkloadIdentityPolicyRepository",
        "Postgres",
        "InMemory",
        "redis",
        "a3s_lane",
        "tokio::spawn",
        "IOutboxRepository",
        "IIntegrationEventProjector",
    ] {
        assert!(
            !production_source(&adapter).contains(forbidden),
            "Workloads Identity ACL duplicated an owner or shared mechanism through {forbidden}"
        );
    }

    assert_eq!(
        binding
            .matches("pub struct DeploymentRuntimeExecutionBinding {")
            .count(),
        1,
        "Workloads acquired a second Deployment Runtime binding"
    );
    for required in [
        "a3s.cloud.deployment-runtime-execution-binding.v1",
        "pub fn admit_unbound(",
        "pub fn validate_admission(",
        "pub fn validate_placement_lineage(",
        "DeploymentStatus::Resolving",
        "self.calculate_binding_digest()? != self.binding_digest",
    ] {
        assert!(
            binding.contains(required),
            "Deployment Runtime binding lost invariant {required}"
        );
    }
    assert!(!production_source(&binding).contains("crate::modules::identity"));
    assert_eq!(
        repository
            .matches("async fn bind_deployment_runtime_execution(")
            .count(),
        1
    );
    assert_eq!(
        repository
            .matches("async fn find_deployment_runtime_execution_binding(")
            .count(),
        1
    );
    assert!(
        owner_query.contains("find_deployment_runtime_execution_binding")
            && owner_query.contains("!runtime_execution_binding.is_bound()")
            && !owner_query.contains("WorkloadRuntimeExecutionBinding"),
        "bound Runtime Claim callers must not synthesize execution semantics"
    );

    for current_path in [&ordinary, &placement] {
        assert!(current_path.contains("admit_deployment_runtime_execution"));
        assert!(current_path.contains("_with_execution"));
        assert!(current_path.contains("validate_placement_lineage"));
    }
    assert!(reconciliation.contains("project_replica_runtime_spec_with_execution"));
    for repository in [&in_memory, &postgres_replicas] {
        assert!(repository.contains("validate_placement_lineage"));
        assert!(repository.contains("DeploymentStatus::Resolving"));
    }
    assert!(application.contains("IdentityWorkloadRuntimeExecutionAdmissionAdapter::new"));
    assert!(application.contains("with_runtime_execution_admission"));

    assert_eq!(
        migration
            .matches("create table deployment_runtime_execution_bindings")
            .count(),
        1
    );
    for forbidden in [
        "policy_id",
        "redis",
        "a3s_lane",
        "outbox",
        "idempotency",
        "create queue",
        "legacy deployments set",
    ] {
        assert!(
            !migration.to_ascii_lowercase().contains(forbidden),
            "Deployment Runtime persistence duplicated a lifecycle or mechanism through {forbidden}"
        );
    }
}

#[test]
fn platform_scope_and_rbac_foundation_has_one_identity_authority_and_only_narrows_scope() {
    let root = module_root();
    let scope = std::fs::read_to_string(root.join("shared_kernel/domain/scope_context.rs"))
        .expect("read shared ScopeContext");
    let policy = std::fs::read_to_string(
        root.join("identity/domain/value_objects/platform_role_policy_contract.rs"),
    )
    .expect("read platform role policy ACL");
    let binding = std::fs::read_to_string(root.join("identity/domain/entities/platform_rbac.rs"))
        .expect("read platform RBAC entities");

    assert_eq!(scope.matches("pub enum ScopeContext {").count(), 1);
    for required in [
        "Installation {",
        "Organization {",
        "Project {",
        "Environment {",
        "pub fn contains(",
        "pub fn intersection(",
        "candidate.organization_id() == Some(organization_id)",
        "candidate.project_id() == Some(project_id)",
    ] {
        assert!(
            scope.contains(required),
            "ScopeContext lost exact hierarchy or narrowing rule {required}"
        );
    }
    for forbidden in [
        "Workspace",
        "HeaderMap",
        "thread_local",
        "RuntimeUnit",
        "NodeId",
        "MembershipRole",
    ] {
        assert!(
            !production_source(&scope).contains(forbidden),
            "shared ScopeContext acquired context-private or ambient authority {forbidden}"
        );
    }

    assert_eq!(policy.matches("pub enum PlatformRole {").count(), 1);
    assert_eq!(policy.matches("pub enum PlatformPermission {").count(), 1);
    assert_eq!(
        policy
            .matches("pub struct PlatformRolePolicyContract {")
            .count(),
        1
    );
    for required in [
        "a3s_acl",
        "canonical_digest",
        "parse_acl",
        "generate_acl",
        "cloud.identity.platform-role-policy.v1",
        "impl Serialize for PlatformPermission",
        "immutable permission ceiling",
        "platform:workload-trust:manage",
    ] {
        assert!(
            policy.contains(required),
            "platform RBAC lost canonical permission authority {required}"
        );
    }
    for forbidden in [
        "actor_is_platform_admin",
        "MembershipRole",
        "ResourceGrant",
        "ApiTokenScope",
        "tenant:secret",
        "tenant:payload",
        "serde_yaml",
        "toml::",
    ] {
        assert!(
            !production_source(&policy).contains(forbidden),
            "platform RBAC duplicated tenant, credential, or configuration authority {forbidden}"
        );
    }

    assert_eq!(
        binding
            .matches("pub struct AcceptedPlatformRolePolicyRevision {")
            .count(),
        1
    );
    assert_eq!(
        binding.matches("pub struct PlatformRoleBinding {").count(),
        1
    );
    assert!(binding.contains("Uuid::new_v5"));
    assert!(binding.contains("validate_against_policy"));
    for forbidden in [
        "actor_is_platform_admin",
        "IdentityPrincipalKind",
        "create table",
        "Postgres",
        "InMemory",
        "async_trait",
        "reqwest",
    ] {
        assert!(
            !production_source(&binding).contains(forbidden),
            "MT1-C1 entity prematurely acquired directory, persistence, or provider authority {forbidden}"
        );
    }
}

#[test]
fn installation_and_tenant_facts_share_one_scope_audit_and_outbox_abstraction() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let event = std::fs::read_to_string(manifest.join("../contracts/src/event.rs"))
        .expect("read public event envelope");
    let scope_reference =
        std::fs::read_to_string(manifest.join("../contracts/src/cloud_scope_ref.rs"))
            .expect("read public Cloud scope reference");
    let outbox = std::fs::read_to_string(
        manifest.join("src/modules/integration_events/domain/entities/outbox_message.rs"),
    )
    .expect("read committed Outbox message");
    let published_envelope = std::fs::read_to_string(
        manifest
            .join("src/modules/integration_events/domain/entities/published_outbox_envelope.rs"),
    )
    .expect("read published Outbox envelope");
    let publisher = std::fs::read_to_string(
        manifest.join("src/modules/integration_events/infrastructure/a3s_event_publisher.rs"),
    )
    .expect("read A3S Event publisher");
    let notification_consumer = std::fs::read_to_string(
        manifest.join("src/modules/notifications/infrastructure/outbound_event_consumer.rs"),
    )
    .expect("read notification event consumer");
    let identity_consumer = std::fs::read_to_string(manifest.join(
        "src/modules/identity/infrastructure/recipient_contact_verification_event_consumer.rs",
    ))
    .expect("read Identity event consumer");
    let postgres_gate = std::fs::read_to_string(manifest.join("tests/postgres_integration.rs"))
        .expect("read PostgreSQL provider gate");
    let notification_nats_gate =
        std::fs::read_to_string(manifest.join("tests/support/notifications/nats.rs"))
            .expect("read notification NATS provider gate");
    let notification_smtp_gate =
        std::fs::read_to_string(manifest.join("tests/support/outbound_smtp/helpers.rs"))
            .expect("read notification SMTP provider gate");
    let persistence = std::fs::read_to_string(manifest.join("src/infrastructure/postgres.rs"))
        .expect("read shared PostgreSQL persistence");
    let outbox_persistence = std::fs::read_to_string(
        manifest.join("src/modules/integration_events/infrastructure/persistence/postgres.rs"),
    )
    .expect("read Outbox persistence");
    let security_persistence =
        std::fs::read_to_string(manifest.join("src/modules/security/infrastructure/postgres.rs"))
            .expect("read Security fact projection");
    let hosted_build_gate = std::fs::read_to_string(manifest.join("tests/support/build_runs.rs"))
        .expect("read hosted build PostgreSQL gate");
    let migration = std::fs::read_to_string(
        manifest.join("../../migrations/174_installation_scoped_facts.sql"),
    )
    .expect("read Installation scope migration");
    let rolling_compatibility = std::fs::read_to_string(
        manifest.join("../../migrations/175_legacy_scoped_fact_writer_compatibility.sql"),
    )
    .expect("read scoped fact rolling compatibility migration");
    let historical_fact_lifecycle = std::fs::read_to_string(
        manifest.join("../../migrations/176_historical_fact_scope_lifecycle.sql"),
    )
    .expect("read historical fact scope lifecycle migration");

    assert!(scope_reference.contains("pub enum CloudScopeRef"));
    assert!(event.contains("pub scope: CloudScopeRef"));
    assert!(!event.contains("pub organization_id: Uuid"));
    assert!(outbox.contains("pub scope: ScopeContext"));
    assert!(!outbox.contains("pub organization_id: Uuid"));
    assert_eq!(
        [
            published_envelope.as_str(),
            publisher.as_str(),
            notification_consumer.as_str(),
            identity_consumer.as_str(),
        ]
        .into_iter()
        .map(|source| source.matches("struct PublishedOutboxEnvelope").count())
        .sum::<usize>(),
        1
    );
    assert!(published_envelope.contains("pub fn from_message(message: &OutboxMessage)"));
    assert!(published_envelope.contains("canonical_organization_id"));
    assert!(published_envelope.contains("self.organization_id != canonical_organization_id"));
    assert!(published_envelope.contains("deny_unknown_fields"));
    assert!(publisher.contains("PublishedOutboxEnvelope::from_message(message)"));
    assert!(!publisher.contains("\"scope\": message.scope"));
    assert!(postgres_gate.contains("PublishedOutboxEnvelope::from_message(&message)"));
    for provider_gate in [&notification_nats_gate, &notification_smtp_gate] {
        assert!(provider_gate.contains("published_outbox_payload("));
        assert!(!provider_gate.contains("\"organizationId\": fact.organization_id()"));
    }
    for consumer in [&notification_consumer, &identity_consumer] {
        assert!(
            consumer.contains("use crate::modules::integration_events::PublishedOutboxEnvelope")
        );
        assert!(consumer.contains(".validate()"));
        assert!(consumer.contains("envelope.require_tenant_organization_id()"));
        assert!(!consumer.contains("struct PublishedOutboxEnvelope"));
    }
    assert!(persistence.contains("pub(crate) scope: CloudScopeRef"));
    assert!(persistence.contains("async fn resolve_cloud_scope("));
    assert!(persistence.contains("ScopeContext::from_resolved_reference"));
    assert!(!persistence.contains("enum AuditAttributionScope"));
    assert!(outbox_persistence.contains("cloud_scope_document("));
    assert!(security_persistence.contains("\"cloud_scope_document\""));
    assert!(security_persistence.contains("let message: OutboxMessage = serde_json::from_value"));
    assert!(security_persistence.contains("message.domain_event()"));
    assert!(
        !security_persistence.contains("let event: DomainEventEnvelope = serde_json::from_value")
    );
    assert!(hosted_build_gate.contains("'scope', cloud_scope_document("));
    assert!(!hosted_build_gate.contains("'organization_id', organization_id"));

    assert_eq!(
        migration
            .matches("create table cloud_installations")
            .count(),
        1
    );
    assert_eq!(
        migration
            .matches("create function cloud_scope_document")
            .count(),
        1
    );
    assert!(migration.contains("alter table outbox_events"));
    assert!(migration.contains("alter table audit_records"));
    assert_eq!(
        rolling_compatibility
            .matches("create function derive_legacy_tenant_fact_scope_kind()")
            .count(),
        1
    );
    assert_eq!(
        rolling_compatibility
            .matches("execute function derive_legacy_tenant_fact_scope_kind()")
            .count(),
        2
    );
    assert!(rolling_compatibility.contains("scope_kind must be explicit for Installation facts"));
    assert!(!rolling_compatibility.contains("drop constraint outbox_events_scope_shape"));
    assert!(!rolling_compatibility.contains("drop constraint audit_records_scope_shape"));
    assert_eq!(
        historical_fact_lifecycle
            .matches("create function validate_cloud_fact_scope_lineage_at_insert()")
            .count(),
        1
    );
    assert_eq!(
        historical_fact_lifecycle
            .matches("execute function validate_cloud_fact_scope_lineage_at_insert()")
            .count(),
        2
    );
    assert!(historical_fact_lifecycle.contains("for key share of tenant, project_row"));
    assert!(
        historical_fact_lifecycle.contains("for key share of tenant, project_row, environment_row")
    );
    assert!(!historical_fact_lifecycle.contains("on delete cascade"));
    assert!(!historical_fact_lifecycle.contains("drop constraint outbox_events_scope_shape"));
    assert!(!historical_fact_lifecycle.contains("drop constraint audit_records_scope_shape"));
    for forbidden in [
        "create table platform_outbox",
        "create table tenant_outbox",
        "create table platform_audit",
        "create table tenant_audit",
    ] {
        assert!(
            !migration.contains(forbidden) && !historical_fact_lifecycle.contains(forbidden),
            "Installation scope introduced duplicate mechanism {forbidden}"
        );
    }
}

#[test]
fn identity_bootstrap_is_one_atomic_tenant_and_platform_authority() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bootstrap_port = std::fs::read_to_string(
        manifest.join("src/modules/identity/domain/repositories/identity_bootstrap_repository.rs"),
    )
    .expect("read Identity bootstrap repository port");
    let token_port = std::fs::read_to_string(
        manifest.join("src/modules/identity/domain/repositories/api_token_repository.rs"),
    )
    .expect("read API token repository port");
    let aggregate = std::fs::read_to_string(
        manifest.join("src/modules/identity/domain/entities/identity_bootstrap.rs"),
    )
    .expect("read Identity bootstrap aggregate");
    let handler = std::fs::read_to_string(
        manifest.join("src/modules/identity/application/commands/bootstrap_identity/handler.rs"),
    )
    .expect("read Identity bootstrap handler");
    let identity_persistence = std::fs::read_to_string(
        manifest.join("src/modules/identity/infrastructure/persistence/postgres.rs"),
    )
    .expect("read Identity PostgreSQL adapter");
    let platform_persistence = std::fs::read_to_string(
        manifest.join("src/modules/identity/infrastructure/persistence/postgres_platform_rbac.rs"),
    )
    .expect("read platform RBAC PostgreSQL adapter");
    let adapters = std::fs::read_to_string(manifest.join("src/app/postgres_adapters.rs"))
        .expect("read PostgreSQL composition adapters");
    let provider_gate = std::fs::read_to_string(manifest.join("tests/postgres_integration.rs"))
        .expect("read PostgreSQL integration gate");

    assert_eq!(
        bootstrap_port
            .matches("pub trait IIdentityBootstrapRepository")
            .count(),
        1
    );
    assert!(bootstrap_port.contains("async fn installation_id"));
    assert!(bootstrap_port.contains("async fn bootstrap_identity"));
    assert!(
        !token_port.contains("bootstrap"),
        "API token persistence reacquired the cross-aggregate bootstrap transaction"
    );
    for required in [
        "pub platform_rbac: PlatformRbacBootstrap",
        "self.platform_rbac.validate()",
        "self.platform_rbac.policy.accepted_by != self.principal.id",
    ] {
        assert!(
            aggregate.contains(required),
            "Identity bootstrap aggregate lost invariant {required}"
        );
    }
    for required in [
        "PlatformRolePolicyContract::baseline",
        "PlatformRole::PlatformOwner",
        "IdentityBootstrap::create",
        ".bootstrap_identity(BootstrapIdentityWrite {",
    ] {
        assert!(
            handler.contains(required),
            "Identity bootstrap handler lost authority composition {required}"
        );
    }
    assert!(!handler.contains("IApiTokenRepository"));

    let bootstrap_transaction = identity_persistence
        .split("impl IIdentityBootstrapRepository for PostgresIdentityRepository")
        .nth(1)
        .and_then(|source| source.split("impl IApiTokenRepository").next())
        .expect("Identity bootstrap persistence implementation");
    assert_eq!(bootstrap_transaction.matches(".transaction(").count(), 1);
    for required in [
        "a3s-cloud.identity.bootstrap",
        "lock_installation(",
        "insert_principal(",
        "insert_membership(",
        "insert_token(",
        "persist_platform_rbac_bootstrap_under_installation_lock(",
        "store_idempotency(",
    ] {
        assert!(
            bootstrap_transaction.contains(required),
            "Identity bootstrap transaction lost atomic write {required}"
        );
    }
    assert!(
        bootstrap_transaction
            .find("a3s-cloud.identity.bootstrap")
            .expect("bootstrap lock")
            < bootstrap_transaction
                .find("idempotency_replay::<IdentityBootstrap>")
                .expect("bootstrap idempotency replay"),
        "bootstrap idempotency must be checked after acquiring its transaction lock"
    );
    assert_eq!(
        platform_persistence
            .matches("persist_platform_rbac_bootstrap_under_installation_lock(")
            .count(),
        2,
        "one transaction-local platform bootstrap writer must serve its definition and internal caller"
    );
    for required in [
        "insert_policy_revision(",
        "insert_policy_head(",
        "insert_binding(",
        "store_policy_facts(",
        "store_binding_facts(",
    ] {
        assert!(
            platform_persistence.contains(required),
            "shared platform bootstrap writer lost {required}"
        );
    }
    assert_eq!(
        adapters
            .matches("identity_bootstrap: repository.clone()")
            .count(),
        1,
        "process composition must expose one typed Identity bootstrap adapter"
    );
    for proof in [
        "reject_identity_bootstrap_platform_fact",
        "identity and platform authorization bootstrap must roll back as one authority",
        "concurrent/replayed Identity bootstrap must elect exactly one matching platform authority root",
    ] {
        assert!(
            provider_gate.contains(proof),
            "PostgreSQL provider gate lost bootstrap proof {proof}"
        );
    }
    for forbidden in [
        "Redis",
        "a3s_lane",
        "bootstrap_outbox",
        "bootstrap_audit",
        "bootstrap_idempotency",
        "bootstrap_distributed_lock",
    ] {
        assert!(
            !bootstrap_transaction.contains(forbidden),
            "Identity bootstrap introduced duplicate mechanism {forbidden}"
        );
    }
}

#[test]
fn api_token_revocation_uses_the_canonical_privileged_authorization_lock_order() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let identity_persistence = std::fs::read_to_string(
        manifest.join("src/modules/identity/infrastructure/persistence/postgres.rs"),
    )
    .expect("read Identity PostgreSQL adapter");
    let platform_persistence = std::fs::read_to_string(
        manifest.join("src/modules/identity/infrastructure/persistence/postgres_platform_rbac.rs"),
    )
    .expect("read platform RBAC PostgreSQL adapter");
    let revocation = identity_persistence
        .split("async fn revoke(")
        .nth(1)
        .expect("API-token revocation persistence implementation");

    let installation_fence = revocation
        .find("lock_canonical_installation_for_authorization_evidence_mutation(transaction)")
        .expect("canonical Installation authorization-evidence mutation fence");
    let replay = revocation
        .find("idempotency_replay::<ApiToken>")
        .expect("API-token revocation idempotency replay");
    let token_update = revocation
        .find("update api_tokens set revoked_at")
        .expect("API-token revocation row update");
    let fact_write = revocation
        .find("store_outbox(transaction, event)")
        .expect("API-token revocation Outbox fact");

    assert!(
        installation_fence < replay && replay < token_update && token_update < fact_write,
        "API-token revocation must lock Installation before idempotency, token, and scoped fact rows"
    );
    for required in [
        "lock_canonical_installation_for_authorization_evidence_mutation",
        "where installation.singleton_key for key share of installation",
    ] {
        assert!(
            platform_persistence.contains(required),
            "canonical privileged-authorization lock authority lost {required}"
        );
    }
    for forbidden in ["Redis", "a3s_lane", "distributed_lock", "40P01"] {
        assert!(
            !revocation.contains(forbidden),
            "API-token revocation introduced a duplicate or retry-based correctness mechanism {forbidden}"
        );
    }
}

#[test]
fn privileged_management_application_surface_is_closed_and_installation_derived() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let platform_commands = std::fs::read_to_string(
        manifest.join("src/modules/identity/application/commands/manage_platform_rbac/commands.rs"),
    )
    .expect("read platform RBAC application commands");
    let support_commands = std::fs::read_to_string(
        manifest
            .join("src/modules/identity/application/commands/manage_tenant_support/commands.rs"),
    )
    .expect("read tenant support application commands");
    let platform_queries = std::fs::read_to_string(
        manifest.join("src/modules/identity/application/queries/read_platform_rbac/queries.rs"),
    )
    .expect("read platform RBAC application queries");
    let support_query = std::fs::read_to_string(
        manifest.join("src/modules/identity/application/queries/read_tenant_support/query.rs"),
    )
    .expect("read tenant support application query");
    let platform_handlers = std::fs::read_to_string(
        manifest.join("src/modules/identity/application/commands/manage_platform_rbac/handlers.rs"),
    )
    .expect("read platform RBAC application handlers");
    let support_handlers = std::fs::read_to_string(
        manifest
            .join("src/modules/identity/application/commands/manage_tenant_support/handlers.rs"),
    )
    .expect("read tenant support application handlers");
    let platform_query_handlers = std::fs::read_to_string(
        manifest.join("src/modules/identity/application/queries/read_platform_rbac/handlers.rs"),
    )
    .expect("read platform RBAC query handlers");
    let support_query_handler = std::fs::read_to_string(
        manifest.join("src/modules/identity/application/queries/read_tenant_support/handler.rs"),
    )
    .expect("read tenant support query handler");

    let commands = format!("{platform_commands}\n{support_commands}");
    let queries = format!("{platform_queries}\n{support_query}");
    let handlers = format!(
        "{platform_handlers}\n{support_handlers}\n{platform_query_handlers}\n{support_query_handler}"
    );
    assert_eq!(commands.matches("pub credential_id: ApiTokenId").count(), 7);
    assert_eq!(queries.matches("pub credential_id: ApiTokenId").count(), 5);
    for forbidden in [
        "installation_id:",
        "actor_is_platform_admin",
        "platform_permission:",
        "support_permission:",
        "action:",
        "scope:",
        "resource_id:",
    ] {
        assert!(
            !commands.contains(forbidden) && !queries.contains(forbidden),
            "privileged Application input exposed ambient or caller-authored authority {forbidden}"
        );
    }
    assert_eq!(handlers.matches("installation_id(&bootstrap)").count(), 12);
    assert_eq!(handlers.matches("::parse_acl(").count(), 2);
    assert!(platform_handlers.contains("deterministic_id("));
    for closed_read in [
        "read_current_platform_role_policy(",
        "read_platform_role_policy_revision(",
        "read_platform_role_binding(",
        "read_principal_platform_role_binding(",
        "read_tenant_support_grant(",
    ] {
        assert!(
            handlers.contains(closed_read),
            "privileged Application query bypassed closed atomic read {closed_read}"
        );
    }
    assert!(!handlers.contains("AuthorizePrivilegedAccess"));
}

#[test]
fn membership_administration_is_one_tenant_scoped_domain_service_without_platform_bypass() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let policy = std::fs::read_to_string(
        manifest.join("src/modules/identity/domain/services/membership_administration.rs"),
    )
    .expect("read membership administration domain service");
    let mut consumers = String::new();
    for relative in [
        "src/modules/identity/domain/repositories/membership_repository.rs",
        "src/modules/identity/domain/repositories/membership_invitation_repository.rs",
        "src/modules/identity/domain/repositories/resource_grant_repository.rs",
        "src/modules/identity/application/commands/create_membership/command.rs",
        "src/modules/identity/application/commands/change_membership_role/command.rs",
        "src/modules/identity/application/commands/revoke_membership/command.rs",
        "src/modules/identity/application/commands/create_membership_invitation/command.rs",
        "src/modules/identity/application/commands/revoke_membership_invitation/command.rs",
        "src/modules/identity/application/commands/create_resource_grant/command.rs",
        "src/modules/identity/application/commands/revoke_resource_grant/command.rs",
        "src/modules/identity/infrastructure/persistence/in_memory_memberships.rs",
        "src/modules/identity/infrastructure/persistence/in_memory_membership_invitations.rs",
        "src/modules/identity/infrastructure/persistence/in_memory_resource_grants.rs",
        "src/modules/identity/infrastructure/persistence/postgres_memberships.rs",
        "src/modules/identity/infrastructure/persistence/postgres_membership_invitations.rs",
        "src/modules/identity/infrastructure/persistence/postgres_resource_grants.rs",
        "src/modules/identity/presentation/controllers/membership_controller.rs",
        "src/modules/identity/presentation/controllers/membership_invitation_controller.rs",
        "src/modules/identity/presentation/controllers/resource_grant_controller.rs",
        "src/presentation/management_mcp/identity.rs",
    ] {
        let source = std::fs::read_to_string(manifest.join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        consumers.push_str(&production_source(&source));
        consumers.push('\n');
    }

    let policy = production_source(&policy);
    assert_eq!(
        policy
            .matches("pub struct MembershipAdministration;")
            .count(),
        1
    );
    assert!(policy.contains("membership.organization_id == organization_id"));
    assert!(policy.contains("membership.is_active()"));
    assert!(policy.contains("can_manage_memberships()"));
    assert_eq!(
        consumers
            .matches("MembershipAdministration::authorize(")
            .count(),
        14,
            "both adapters must reuse the one domain service for membership, invitation, and Resource Grant administration"
    );
    let mut token_issuance_consumers = String::new();
    let mut token_issuance_boundaries = String::new();
    for relative in [
        "src/modules/identity/domain/repositories/api_token_repository.rs",
        "src/modules/identity/application/commands/create_api_token/command.rs",
        "src/modules/identity/application/commands/create_api_token/handler.rs",
        "src/modules/identity/presentation/controllers/api_token_controller.rs",
        "src/modules/identity/infrastructure/persistence/in_memory.rs",
        "src/modules/identity/infrastructure/persistence/postgres.rs",
    ] {
        let source = std::fs::read_to_string(manifest.join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        let source = production_source(&source);
        token_issuance_consumers.push_str(&source);
        token_issuance_consumers.push('\n');
        if !relative.contains("/persistence/") {
            token_issuance_boundaries.push_str(&source);
            token_issuance_boundaries.push('\n');
        }
    }
    assert_eq!(
        token_issuance_consumers
            .matches("MembershipAdministration::authorize(")
            .count(),
        2,
        "both API token adapters must reuse the one tenant-scoped domain service"
    );
    assert!(
        !token_issuance_consumers.contains("issuer_is_platform_admin"),
        "API token issuance retained a caller-authored platform administrator bypass"
    );
    assert!(
        !token_issuance_boundaries.contains("is_platform_admin"),
        "API token issuance derived authority from a presentation-layer platform role"
    );
    for forbidden in [
        "actor_is_platform_admin",
        "issuer_is_platform_admin",
        "has_role(\"platform_admin\")",
        "fn authorize_management(",
    ] {
        assert!(
            !consumers.contains(forbidden),
            "tenant administration retained a platform-role bypass via {forbidden}"
        );
    }
    for forbidden in ["PlatformRole", "TenantSupportGrant", "ApiTokenScope"] {
        assert!(
            !policy.contains(forbidden),
            "membership administration acquired unrelated authorization authority {forbidden}"
        );
    }
}

#[test]
fn tenant_resource_authorization_requires_membership_without_platform_role_bypass() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut sources = String::new();
    for relative in [
        "src/modules/identity/domain/services/resource_authorization_decision.rs",
        "src/modules/identity/infrastructure/persistence/in_memory_resource_authorization_decisions.rs",
        "src/modules/identity/infrastructure/persistence/postgres_resource_authorization_decisions.rs",
        "src/modules/identity/presentation/guards.rs",
        "src/modules/identity/presentation/request_context.rs",
        "src/modules/identity/presentation/resource_access.rs",
        "src/modules/agents/application/commands/decide_agent_approval_checkpoint/command.rs",
        "src/modules/agents/application/commands/decide_agent_approval_checkpoint/handler.rs",
        "src/modules/agents/presentation/controllers/agent_commands_controller.rs",
        "src/modules/workflow/application/commands/submit_human_task/command.rs",
        "src/modules/workflow/application/commands/submit_human_task/handler.rs",
        "src/modules/workflow/presentation/controllers/workflow_commands_controller.rs",
        "src/presentation/management_mcp/catalog.rs",
        "src/presentation/management_mcp/dispatch.rs",
        "src/presentation/management_mcp/handler.rs",
        "src/presentation/management_mcp/workflow.rs",
    ] {
        let source = std::fs::read_to_string(manifest.join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        sources.push_str(&production_source(&source));
        sources.push('\n');
    }

    for forbidden in [
        "actor_is_platform_admin",
        "issuer_is_platform_admin",
        "is_platform_admin",
        "issue_platform_administrator",
        "PlatformAdministrator",
        "has_role(\"platform_admin\")",
    ] {
        assert!(
            !sources.contains(forbidden),
            "tenant resource authorization retained ambient platform authority via {forbidden}"
        );
    }
    assert_eq!(
        sources
            .matches("ResourceAuthorizationDecision::issue_membership(")
            .count(),
        2,
        "both resource-authorization adapters must issue the one membership-based decision"
    );
    assert!(sources.contains("requested != authenticated"));
    assert!(sources.contains("claim(\"organization_role\")"));
}

#[test]
fn organization_catalog_has_one_atomic_credential_bound_authority() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let read = |relative: &str| {
        std::fs::read_to_string(manifest.join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"))
    };
    let port = read("src/modules/identity/domain/repositories/organization_repository.rs");
    let handler = read("src/modules/identity/application/queries/list_organizations/handler.rs");
    let controller = production_source(&read(
        "src/modules/identity/presentation/controllers/organizations_query_controller.rs",
    ));
    let verifier = production_source(&read(
        "src/modules/identity/infrastructure/api_token_verifier.rs",
    ));
    let postgres = production_source(&read(
        "src/modules/identity/infrastructure/persistence/postgres.rs",
    ));
    let in_memory = production_source(&read(
        "src/modules/identity/infrastructure/persistence/in_memory.rs",
    ));
    let provider_gate = read("tests/support/privileged_authorization_decisions.rs");

    for required in [
        "pub struct ReadOrganizationCatalog",
        "pub installation_id: InstallationId",
        "pub actor_principal_id: PrincipalId",
        "pub credential_id: ApiTokenId",
        "pub request_id: Uuid",
        "async fn list_visible(",
    ] {
        assert!(
            port.contains(required),
            "organization catalog port lost exact authority input {required}"
        );
    }
    assert!(
        !port.contains("async fn list(&self)"),
        "organization repository regained an unguarded global list mechanism"
    );
    for required in [
        "installation_id(&bootstrap)",
        "ReadOrganizationCatalog",
        "actor_principal_id: query.actor_principal_id",
        "credential_id: query.credential_id",
    ] {
        assert!(
            handler.contains(required),
            "organization catalog application boundary lost {required}"
        );
    }
    for required in [
        "authenticated_credential_actor",
        "request_id(&request)",
        "AUTH_SCOPES_METADATA",
        "ApiTokenScope::CLOUD_READ",
    ] {
        assert!(
            controller.contains(required),
            "organization catalog controller lost trusted request input {required}"
        );
    }
    for forbidden in [
        "has_role(\"platform_admin\")",
        "with_role(\"platform_admin\")",
        "actor_is_platform_admin",
    ] {
        assert!(
            !format!("{controller}\n{verifier}").contains(forbidden),
            "organization catalog regained ambient platform authority via {forbidden}"
        );
    }
    for required in [
        ".transaction(",
        "lock_installation_for_authorization",
        "issue_privileged_authorization",
        "platform_authorization_request",
        "PlatformPermission::TenantLifecycleRead",
        "load_active_principal_for_authorization",
        "load_api_token_by_id_for_authorization",
        "credential.principal_id == principal.id",
        "credential.is_active_at(decided_at)",
        "credential.grants_scope(ApiTokenScope::CLOUD_READ)",
        "identity.organization-catalog.read",
    ] {
        assert!(
            postgres.contains(required),
            "PostgreSQL organization catalog lost atomic authority rule {required}"
        );
    }
    for required in [
        "credential.principal_id == principal.id",
        "credential.is_active_at(Utc::now())",
        "credential.grants_scope(ApiTokenScope::CLOUD_READ)",
        "Ok(vec![organization])",
    ] {
        assert!(
            in_memory.contains(required),
            "in-memory organization catalog lost fail-closed rule {required}"
        );
    }
    for required in [
        "IOrganizationRepository",
        "ReadOrganizationCatalog",
        "PlatformPermission::TenantLifecycleRead",
        "a credential without cloud:read must not receive tenant or Installation catalog access",
        "tenant-only catalog fallback must not manufacture privileged allow evidence",
    ] {
        assert!(
            provider_gate.contains(required),
            "organization catalog provider proof lost {required}"
        );
    }
    for forbidden in ["Redis", "a3s_lane", "distributed_lock"] {
        assert!(
            !postgres.contains(forbidden),
            "organization catalog introduced duplicate coordination mechanism {forbidden}"
        );
    }
}

#[test]
fn privileged_management_has_one_composition_root_and_fail_closed_test_adapter() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let composition = std::fs::read_to_string(manifest.join("src/app.rs"))
        .expect("read control-plane composition root");
    let adapters = std::fs::read_to_string(manifest.join("src/app/postgres_adapters.rs"))
        .expect("read PostgreSQL adapter factory");
    let in_memory = std::fs::read_to_string(manifest.join(
        "src/modules/identity/infrastructure/persistence/in_memory_privileged_management.rs",
    ))
    .expect("read fail-closed in-memory privileged management adapter");

    for command in [
        "AcceptPlatformRolePolicy",
        "CreatePlatformRoleBinding",
        "ChangePlatformRoleBinding",
        "RevokePlatformRoleBinding",
        "ProposeTenantSupportGrant",
        "ApproveTenantSupportGrant",
        "RevokeTenantSupportGrant",
    ] {
        assert_eq!(
            composition
                .matches(&format!(
                    ".command_handler::<crate::modules::identity::{command}, _>"
                ))
                .count(),
            1,
            "privileged command {command} must have one CQRS registration"
        );
    }
    for query in [
        "GetCurrentPlatformRolePolicy",
        "GetPlatformRolePolicyRevision",
        "GetPlatformRoleBinding",
        "GetPrincipalPlatformRoleBinding",
        "GetTenantSupportGrant",
        "InspectCurrentTrustDomainProvider",
    ] {
        assert_eq!(
            composition
                .matches(&format!(
                    ".query_handler::<crate::modules::identity::{query}, _>"
                ))
                .count(),
            1,
            "privileged query {query} must have one CQRS registration"
        );
    }
    for required in [
        "pub(super) platform_rbac: Arc<dyn IPlatformRbacRepository>",
        "pub(super) tenant_support_grants: Arc<dyn ITenantSupportGrantRepository>",
        "platform_rbac: repository.clone()",
        "tenant_support_grants: repository",
    ] {
        assert!(
            adapters.contains(required),
            "Identity PostgreSQL adapter family lost {required}"
        );
    }
    assert!(in_memory.contains("privileged management requires the PostgreSQL Identity authority"));
    assert_eq!(
        in_memory
            .matches("impl IPlatformRbacRepository for InMemoryIdentityRepository")
            .count(),
        1
    );
    assert_eq!(
        in_memory
            .matches("impl ITenantSupportGrantRepository for InMemoryIdentityRepository")
            .count(),
        1
    );
    for forbidden in [
        "PlatformRolePolicyContract",
        "TenantSupportGrantContract",
        "issue_privileged_authorization",
        "actor_is_platform_admin",
        "Redis",
        "a3s_lane",
    ] {
        assert!(
            !in_memory.contains(forbidden),
            "fail-closed test composition became a duplicate authority via {forbidden}"
        );
    }
}

#[test]
fn privileged_management_rest_surface_uses_verified_credentials_and_closed_use_cases() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let controller =
        std::fs::read_to_string(manifest.join(
            "src/modules/identity/presentation/controllers/privileged_management_controller.rs",
        ))
        .expect("read privileged management REST controller");
    let requests = std::fs::read_to_string(
        manifest
            .join("src/modules/identity/presentation/dto/request/privileged_management_request.rs"),
    )
    .expect("read privileged management request DTOs");
    let responses =
        std::fs::read_to_string(manifest.join(
            "src/modules/identity/presentation/dto/response/privileged_management_response.rs",
        ))
        .expect("read privileged management response DTOs");
    let module = std::fs::read_to_string(
        manifest.join("src/modules/identity/presentation/identity_module.rs"),
    )
    .expect("read Identity module");

    for (route, expected_occurrences) in [
        ("/role-policy", 1),
        ("/role-policy/revisions/{revision_id}", 1),
        ("/role-policy/revisions", 1),
        ("/role-bindings", 1),
        ("/role-bindings/{binding_id}", 1),
        ("/role-bindings/{binding_id}/role", 1),
        ("/role-bindings/{binding_id}/revocation", 1),
        ("/principals/{principal_id}/role-binding", 1),
        ("/tenant-support-grants", 1),
        ("/tenant-support-grants/{grant_id}", 1),
        ("/tenant-support-grants/{grant_id}/approvals", 1),
        ("/tenant-support-grants/{grant_id}/revocation", 1),
        ("/trust-domains/{trust_domain_id}", 1),
        ("/trust-domains/{trust_domain_id}/provider-inspection", 1),
        ("/trust-domains/{trust_domain_id}/revisions", 2),
        (
            "/trust-domains/{trust_domain_id}/revisions/{revision_id}",
            1,
        ),
        (
            "/organizations/{organization_id}/workload-identity-policies/{policy_id}",
            1,
        ),
        (
            "/organizations/{organization_id}/workload-identity-policies/{policy_id}/revisions",
            2,
        ),
        (
            "/organizations/{organization_id}/workload-identity-policies/{policy_id}/revisions/{revision_id}",
            1,
        ),
        (
            "/organizations/{organization_id}/workloads/{workload_id}/identity-policy",
            1,
        ),
    ] {
        assert_eq!(
            controller.matches(&format!("\"{route}\"")).count(),
            expected_occurrences,
            "privileged REST route {route} has an unexpected operation count"
        );
    }
    assert_eq!(
        controller
            .matches("authenticated_credential_actor(")
            .count(),
        22,
        "every privileged REST route must derive the exact verified Principal and API Token"
    );
    assert_eq!(controller.matches("require_auth_principal()").count(), 22);
    assert_eq!(controller.matches("ApiTokenScope::CLOUD_READ").count(), 3);
    assert_eq!(
        controller.matches("ApiTokenScope::PLATFORM_WRITE").count(),
        3
    );
    for forbidden in [
        "actor_is_platform_admin",
        ".has_role(",
        "AuthorizePrivilegedAccess",
        "platform_permission",
        "support_permission",
    ] {
        assert!(
            !controller.contains(forbidden),
            "privileged REST surface introduced ambient or caller-authored authority {forbidden}"
        );
    }
    assert_eq!(requests.matches("deny_unknown_fields").count(), 8);
    assert!(responses.matches("rename_all = \"camelCase\"").count() >= 10);
    for controller_name in [
        "platform_rbac_queries_controller",
        "platform_rbac_commands_controller",
        "tenant_support_query_controller",
        "tenant_support_commands_controller",
        "workload_trust_queries_controller",
        "workload_trust_commands_controller",
    ] {
        assert_eq!(
            module.matches(&format!("{controller_name}(")).count(),
            1,
            "Identity module must register {controller_name} exactly once"
        );
    }
}

#[test]
fn privileged_management_mcp_is_one_installation_bound_application_adapter() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let adapter = std::fs::read_to_string(
        manifest.join("src/presentation/management_mcp/privileged_management.rs"),
    )
    .expect("read privileged management MCP adapter");
    let dispatch =
        std::fs::read_to_string(manifest.join("src/presentation/management_mcp/dispatch.rs"))
            .expect("read Management MCP dispatch");
    let catalog =
        std::fs::read_to_string(manifest.join("src/presentation/management_mcp/catalog.rs"))
            .expect("read Management MCP catalog");
    let handler =
        std::fs::read_to_string(manifest.join("src/presentation/management_mcp/handler.rs"))
            .expect("read Management MCP handler");

    for use_case in [
        "GetCurrentPlatformRolePolicy",
        "GetPlatformRolePolicyRevision",
        "AcceptPlatformRolePolicy",
        "GetPlatformRoleBinding",
        "GetPrincipalPlatformRoleBinding",
        "CreatePlatformRoleBinding",
        "ChangePlatformRoleBinding",
        "RevokePlatformRoleBinding",
        "GetTenantSupportGrant",
        "ProposeTenantSupportGrant",
        "ApproveTenantSupportGrant",
        "RevokeTenantSupportGrant",
        "GetCurrentTrustDomain",
        "InspectCurrentTrustDomainProvider",
        "GetTrustDomainRevision",
        "ListTrustDomainRevisions",
        "AcceptTrustDomainRevision",
        "GetCurrentWorkloadIdentityPolicy",
        "GetCurrentWorkloadIdentityPolicyForWorkload",
        "GetWorkloadIdentityPolicyRevision",
        "ListWorkloadIdentityPolicyRevisions",
        "AcceptWorkloadIdentityPolicyRevision",
    ] {
        assert!(
            adapter.contains(&format!(".execute({use_case} {{")),
            "privileged MCP stopped dispatching the Application use case {use_case}"
        );
    }
    for operation in [
        "get_current_platform_role_policy",
        "get_platform_role_policy_revision",
        "accept_platform_role_policy",
        "get_platform_role_binding",
        "get_principal_platform_role_binding",
        "create_platform_role_binding",
        "change_platform_role_binding",
        "revoke_platform_role_binding",
        "get_tenant_support_grant",
        "propose_tenant_support_grant",
        "approve_tenant_support_grant",
        "revoke_tenant_support_grant",
        "get_current_trust_domain",
        "inspect_current_trust_domain_provider",
        "get_trust_domain_revision",
        "list_trust_domain_revisions",
        "accept_trust_domain_revision",
        "get_current_workload_identity_policy",
        "get_current_workload_identity_policy_for_workload",
        "get_workload_identity_policy_revision",
        "list_workload_identity_policy_revisions",
        "accept_workload_identity_policy_revision",
    ] {
        assert_eq!(
            dispatch
                .matches(&format!("privileged_management::{operation}("))
                .count(),
            1,
            "privileged MCP operation {operation} must have one dispatch path"
        );
    }
    for name in [
        "a3s_cloud_platform_role_policy_current_get",
        "a3s_cloud_platform_role_policy_revisions_get",
        "a3s_cloud_platform_role_policy_revisions_accept",
        "a3s_cloud_platform_role_bindings_get",
        "a3s_cloud_principal_platform_role_binding_get",
        "a3s_cloud_platform_role_bindings_create",
        "a3s_cloud_platform_role_bindings_change_role",
        "a3s_cloud_platform_role_bindings_revoke",
        "a3s_cloud_tenant_support_grants_get",
        "a3s_cloud_tenant_support_grants_propose",
        "a3s_cloud_tenant_support_grants_approve",
        "a3s_cloud_tenant_support_grants_revoke",
        "a3s_cloud_trust_domains_current_get",
        "a3s_cloud_trust_domain_provider_inspect",
        "a3s_cloud_trust_domain_revisions_list",
        "a3s_cloud_trust_domain_revisions_get",
        "a3s_cloud_trust_domain_revisions_accept",
        "a3s_cloud_workload_identity_policies_current_get",
        "a3s_cloud_workload_identity_policy_revisions_list",
        "a3s_cloud_workload_identity_policy_revisions_get",
        "a3s_cloud_workload_identity_policy_revisions_accept",
        "a3s_cloud_workload_identity_policy_for_workload_get",
    ] {
        assert_eq!(
            catalog.matches(&format!("\"{name}\"")).count(),
            1,
            "privileged MCP tool {name} must be unique"
        );
    }
    assert!(catalog.contains("Some(ManagementResourceBinding::Installation)"));
    assert!(handler.contains("Some(ManagementResourceBinding::Installation) => Ok(true)"));
    for forbidden in [
        "Repository",
        "postgres",
        "sqlx",
        "Redis",
        "a3s_lane",
        "actor_is_platform_admin",
        ".has_role(",
        "AuthorizePrivilegedAccess",
        "PlatformRolePolicyContract",
        "TenantSupportGrantContract",
    ] {
        assert!(
            !adapter.contains(forbidden),
            "privileged MCP introduced a second authority via {forbidden}"
        );
    }
}

#[test]
fn platform_rbac_persistence_reuses_one_identity_and_shared_fact_authority() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let port = std::fs::read_to_string(
        manifest.join("src/modules/identity/domain/repositories/platform_rbac_repository.rs"),
    )
    .expect("read platform RBAC repository port");
    let persistence = std::fs::read_to_string(
        manifest.join("src/modules/identity/infrastructure/persistence/postgres_platform_rbac.rs"),
    )
    .expect("read platform RBAC PostgreSQL adapter");
    let migration =
        std::fs::read_to_string(manifest.join("../../migrations/177_platform_rbac_authority.sql"))
            .expect("read platform RBAC authority migration");
    let provider_gate = std::fs::read_to_string(manifest.join("tests/support/platform_rbac.rs"))
        .expect("read platform RBAC PostgreSQL provider gate");
    let workflow = std::fs::read_to_string(manifest.join("../../.github/workflows/ci.yml"))
        .expect("read CI workflow");

    assert_eq!(port.matches("pub trait IPlatformRbacRepository").count(), 1);
    assert_eq!(
        port.matches("pub credential_id: ApiTokenId").count(),
        8,
        "every non-bootstrap platform RBAC write and closed read must carry the exact verified credential identity"
    );
    assert_eq!(
        port.matches("pub expected_policy_revision_id: PlatformRolePolicyRevisionId")
            .count(),
        2,
        "binding creation and role changes must CAS the policy used to derive role semantics"
    );
    assert_eq!(
        persistence
            .matches("impl IPlatformRbacRepository for PostgresIdentityRepository")
            .count(),
        1
    );
    for required in [
        "for update of installation",
        "load_current_policy_for_update",
        "idempotency_replay",
        "store_idempotency",
        "store_outbox",
        "store_audit",
        "PlatformPermission::RolePolicyManage",
        "PlatformPermission::RoleBindingManage",
        "PlatformPermission::RolePolicyRead",
        "PlatformPermission::RoleBindingRead",
        "lock_installation_for_authorization",
        "read_current_platform_role_policy",
        "read_platform_role_policy_revision",
        "read_platform_role_binding",
        "read_principal_platform_role_binding",
        "a Principal cannot escalate its own platform permissions",
        "the last active platform owner cannot be revoked",
        "platform role policy changed before binding creation",
        "platform role policy changed before the binding role update",
    ] {
        assert!(
            persistence.contains(required),
            "platform RBAC persistence lost authority rule {required}"
        );
    }
    assert_eq!(
        persistence
            .matches("let authorization = issue_privileged_authorization(")
            .count(),
        4,
        "every non-bootstrap platform RBAC mutation must authorize inside its PostgreSQL transaction"
    );
    assert!(persistence.contains("\"authorizationDecision\": authorization"));
    for forbidden in [
        "pub permission:",
        "pub action:",
        "pub scope:",
        "pub resource_id:",
    ] {
        assert!(
            !port.contains(forbidden),
            "platform RBAC public storage port exposed caller-authored authorization input {forbidden}"
        );
    }
    for forbidden in [
        "actor_is_platform_admin",
        "Redis",
        "a3s_lane",
        "platform_outbox",
        "platform_audit",
        "platform_idempotency",
        "platform_distributed_lock",
    ] {
        assert!(
            !production_source(&persistence).contains(forbidden),
            "platform RBAC persistence introduced duplicate or ambient authority {forbidden}"
        );
    }

    assert_eq!(
        migration
            .matches("create table platform_role_policy_heads")
            .count(),
        1
    );
    assert!(migration.contains("deferrable initially deferred"));
    assert!(migration.contains("for update of installation"));
    assert!(migration.contains("the last active platform owner Principal cannot be disabled"));
    for duplicate in [
        "create table platform_outbox",
        "create table platform_audit",
        "create table platform_idempotency",
        "create table platform_distributed_locks",
    ] {
        assert!(
            !migration.contains(duplicate),
            "platform RBAC migration introduced second mechanism {duplicate}"
        );
    }

    assert!(provider_gate.contains("tokio::join!"));
    assert!(provider_gate.contains("concurrent platform RBAC bootstrap"));
    assert!(provider_gate.contains("concurrent owner revocation"));
    assert!(provider_gate.contains("concurrent policy CAS"));
    assert!(provider_gate
        .contains("business mutation and exact credential revocation were not serialized"));
    assert!(provider_gate.contains(
        "authorization decision and protected business fact must commit or roll back together"
    ));
    assert!(provider_gate.contains("database trigger must reject last-owner bypasses"));
    assert!(
        workflow.contains("postgres_platform_rbac_is_atomic_recoverable_and_multi_replica_safe")
    );
}

#[test]
fn privileged_tenant_support_reuses_one_decision_evidence_mechanism_and_never_implies_data_access()
{
    let root = module_root();
    let policy = std::fs::read_to_string(
        root.join("identity/domain/value_objects/platform_role_policy_contract.rs"),
    )
    .expect("read platform role policy ACL");
    let contract = std::fs::read_to_string(
        root.join("identity/domain/value_objects/tenant_support_grant_contract.rs"),
    )
    .expect("read tenant support grant ACL");
    let grant =
        std::fs::read_to_string(root.join("identity/domain/entities/tenant_support_grant.rs"))
            .expect("read tenant support grant entity");
    let decision = std::fs::read_to_string(
        root.join("identity/domain/services/privileged_authorization_decision.rs"),
    )
    .expect("read privileged authorization decision");
    let resource_decision = std::fs::read_to_string(
        root.join("identity/domain/services/resource_authorization_decision.rs"),
    )
    .expect("read resource authorization decision");
    let evidence_ref =
        std::fs::read_to_string(root.join("shared_kernel/domain/authorization_decision_ref.rs"))
            .expect("read shared decision evidence reference");

    assert_eq!(
        contract
            .matches("pub enum TenantSupportPermission {")
            .count(),
        1
    );
    assert_eq!(
        contract
            .matches("pub struct TenantSupportGrantContract {")
            .count(),
        1
    );
    for required in [
        "pub const ALL: [Self; 7]",
        "cloud.identity.tenant-support-grant.v1",
        "a3s_acl",
        "canonical_digest",
        "parse_acl",
        "generate_acl",
        "security_alert_required",
        "post_incident_review_required",
        "break-glass tenant support must notify tenant and security and require review",
        "ScopeContext::Organization",
        "ScopeContext::Project",
        "ScopeContext::Environment",
    ] {
        assert!(
            contract.contains(required),
            "tenant support grant lost bounded canonical intent {required}"
        );
    }
    for forbidden in [
        "tenant-support:secret",
        "tenant-support:payload",
        "tenant-support:prompt",
        "tenant-support:response",
        "tenant-support:runtime:exec",
        "serde_yaml",
        "toml::",
        "actor_is_platform_admin",
    ] {
        assert!(
            !production_source(&contract).contains(forbidden),
            "tenant support grant introduced forbidden tenant-data or configuration authority {forbidden}"
        );
    }

    assert_eq!(grant.matches("pub struct TenantSupportGrant {").count(), 1);
    for required in [
        "non-renewing",
        "revocation_generation",
        "self.scope().contains(requested_scope)",
        "self.revoked_at.is_none()",
    ] {
        assert!(
            grant.contains(required),
            "tenant support lifecycle lost terminal narrowing rule {required}"
        );
    }
    for forbidden in [
        "fn renew(",
        "fn extend(",
        "fn reactivate(",
        "Postgres",
        "Redis",
    ] {
        assert!(
            !production_source(&grant).contains(forbidden),
            "tenant support lifecycle acquired renewal or infrastructure mechanism {forbidden}"
        );
    }

    assert_eq!(
        decision
            .matches("pub struct PrivilegedAuthorizationDecision {")
            .count(),
        1
    );
    for required in [
        "DecisionEvidenceRef",
        "canonical_json_bounded",
        "validate_audit_action",
        "PlatformRolePolicyContract::restore",
        "TenantSupportGrantContract::restore",
        "PlatformPermission::TenantSupportUse",
        "IdentityPrincipalKind::Human",
        "platform role alone cannot authorize the requested scope",
        "self.scope.is_tenant_scope()",
    ] {
        assert!(
            decision.contains(required),
            "privileged decision lost exact replay or tenant-intersection evidence {required}"
        );
    }
    for forbidden in [
        "actor_is_platform_admin",
        "HeaderMap",
        "thread_local",
        "Postgres",
        "Redis",
        "a3s_lane",
        "reqwest",
        "Repository",
    ] {
        assert!(
            !production_source(&decision).contains(forbidden),
            "privileged decision acquired ambient, persistence, cache, or provider authority {forbidden}"
        );
    }

    for required in [
        "platform:tenant-support:read",
        "platform:tenant-support:manage",
        "platform:tenant-support:use",
    ] {
        assert!(
            policy.contains(required),
            "platform role policy lost support-plane permission {required}"
        );
    }
    assert!(resource_decision.contains("validate_audit_action"));
    assert!(!production_source(&resource_decision).contains("fn valid_action("));
    assert_eq!(
        evidence_ref
            .matches("pub struct AuthorizationDecisionRef {")
            .count(),
        1
    );
    assert_eq!(
        evidence_ref
            .matches("pub type DecisionEvidenceRef = AuthorizationDecisionRef;")
            .count(),
        1
    );
}

#[test]
fn tenant_support_approval_persistence_reuses_identity_and_shared_fact_authorities() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let port = std::fs::read_to_string(
        manifest
            .join("src/modules/identity/domain/repositories/tenant_support_grant_repository.rs"),
    )
    .expect("read tenant support grant repository port");
    let persistence =
        std::fs::read_to_string(manifest.join(
            "src/modules/identity/infrastructure/persistence/postgres_tenant_support_grants.rs",
        ))
        .expect("read tenant support grant PostgreSQL adapter");
    let migration = std::fs::read_to_string(
        manifest.join("../../migrations/178_tenant_support_grant_approvals.sql"),
    )
    .expect("read tenant support approval migration");
    let provider_gate =
        std::fs::read_to_string(manifest.join("tests/support/tenant_support_grants.rs"))
            .expect("read tenant support PostgreSQL provider gate");
    let workflow = std::fs::read_to_string(manifest.join("../../.github/workflows/ci.yml"))
        .expect("read CI workflow");

    assert_eq!(
        port.matches("pub trait ITenantSupportGrantRepository")
            .count(),
        1
    );
    assert_eq!(
        port.matches("pub credential_id: ApiTokenId").count(),
        4,
        "support mutations and the closed record read must carry exact verified credential identity"
    );
    assert!(!port.contains("pub authentication: DecisionEvidenceRef"));
    assert_eq!(
        persistence
            .matches("impl ITenantSupportGrantRepository for PostgresIdentityRepository")
            .count(),
        1
    );
    for required in [
        "lock_installation",
        "load_current_policy_for_update",
        "require_current_support_manager",
        "PlatformPermission::TenantSupportManage",
        "PlatformPermission::TenantSupportRead",
        "lock_installation_for_authorization",
        "read_tenant_support_grant",
        "idempotency_replay",
        "store_idempotency",
        "store_outbox",
        "store_audit",
        ".map(|recorded| recorded.approved_at)",
        ".max()",
    ] {
        assert!(
            persistence.contains(required),
            "tenant support persistence lost authority rule {required}"
        );
    }
    assert_eq!(
        persistence
            .matches("let authorization = issue_privileged_authorization(")
            .count(),
        3,
        "each support management mutation must authorize inside its PostgreSQL transaction"
    );
    assert!(persistence.contains("authorization.authentication"));
    assert!(persistence.contains("\"authorizationDecision\": authorization"));
    for forbidden in [
        "pub permission:",
        "pub action:",
        "pub scope:",
        "pub resource_id:",
    ] {
        assert!(
            !port.contains(forbidden),
            "tenant support public storage port exposed caller-authored authorization input {forbidden}"
        );
    }
    for forbidden in [
        "actor_is_platform_admin",
        "Redis",
        "a3s_lane",
        "tenant_support_outbox",
        "tenant_support_audit",
        "tenant_support_idempotency",
        "tenant_support_distributed_lock",
    ] {
        assert!(
            !production_source(&persistence).contains(forbidden),
            "tenant support persistence introduced duplicate or ambient authority {forbidden}"
        );
    }

    assert_eq!(
        migration
            .matches("create table tenant_support_grant_approvals")
            .count(),
        1
    );
    assert_eq!(
        migration
            .matches("execute function validate_cloud_fact_scope_lineage_at_insert()")
            .count(),
        1
    );
    assert!(migration.contains("deferrable initially deferred"));
    assert!(migration.contains("for update of installation"));
    assert!(migration.contains("select max(approval.approved_at)"));
    for duplicate in [
        "create table tenant_support_outbox",
        "create table tenant_support_audit",
        "create table tenant_support_idempotency",
        "create table tenant_support_locks",
    ] {
        assert!(
            !migration.contains(duplicate),
            "tenant support migration introduced second mechanism {duplicate}"
        );
    }

    for required in [
        "tokio::join!",
        "declared approver IDs alone must never activate a grant",
        "failed threshold revalidation must roll back the final approval",
        "actual approval evidence must be immutable",
    ] {
        assert!(
            provider_gate.contains(required),
            "tenant support provider gate lost proof {required}"
        );
    }
    assert!(workflow
        .contains("postgres_tenant_support_grants_require_actual_multi_replica_approval_evidence"));
}

#[test]
fn privileged_authorization_uses_one_atomic_identity_decision_and_shared_audit_authority() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let port = std::fs::read_to_string(manifest.join(
        "src/modules/identity/domain/repositories/privileged_authorization_decision_repository.rs",
    ))
    .expect("read privileged authorization repository port");
    let decision = std::fs::read_to_string(
        manifest.join("src/modules/identity/domain/services/privileged_authorization_decision.rs"),
    )
    .expect("read privileged authorization decision");
    let persistence = std::fs::read_to_string(manifest.join(
        "src/modules/identity/infrastructure/persistence/postgres_privileged_authorization_decisions.rs",
    ))
    .expect("read privileged authorization PostgreSQL adapter");
    let identity_persistence = std::fs::read_to_string(
        manifest.join("src/modules/identity/infrastructure/persistence/postgres.rs"),
    )
    .expect("read Identity PostgreSQL adapter");
    let rbac_persistence = std::fs::read_to_string(
        manifest.join("src/modules/identity/infrastructure/persistence/postgres_platform_rbac.rs"),
    )
    .expect("read platform RBAC PostgreSQL adapter");
    let support_persistence =
        std::fs::read_to_string(manifest.join(
            "src/modules/identity/infrastructure/persistence/postgres_tenant_support_grants.rs",
        ))
        .expect("read tenant support PostgreSQL adapter");
    let application =
        std::fs::read_to_string(manifest.join(
            "src/modules/identity/application/commands/authorize_privileged_access/handler.rs",
        ))
        .expect("read privileged authorization application handler");
    let provider_gate = std::fs::read_to_string(
        manifest.join("tests/support/privileged_authorization_decisions.rs"),
    )
    .expect("read privileged authorization PostgreSQL provider gate");
    let workflow = std::fs::read_to_string(manifest.join("../../.github/workflows/ci.yml"))
        .expect("read CI workflow");

    assert_eq!(
        port.matches("pub trait IPrivilegedAuthorizationDecisionRepository")
            .count(),
        1
    );
    assert_eq!(
        persistence
            .matches(
                "impl IPrivilegedAuthorizationDecisionRepository for PostgresIdentityRepository",
            )
            .count(),
        1
    );
    for required in [
        ".transaction(",
        "lock_installation_for_authorization",
        "load_active_principal_for_authorization",
        "load_api_token_by_id_for_authorization",
        "load_current_policy_for_authorization",
        "load_active_actor_binding_for_authorization",
        "load_grant_for_authorization",
        "store_audit",
        "PrivilegedAuthorizationDecision::issue_platform",
        "PrivilegedAuthorizationDecision::issue_tenant_support",
    ] {
        assert!(
            persistence.contains(required),
            "privileged authorization persistence lost atomic authority rule {required}"
        );
    }
    assert!(!persistence.contains("_for_update"));
    for required in [
        "for key share of installation",
        "for share of principal",
        "for share of head",
        "for share of binding, principal",
    ] {
        assert!(
            rbac_persistence.contains(required),
            "privileged authorization lost shared revocation fence {required}"
        );
    }
    assert!(identity_persistence.contains(".append(\" for share\")"));
    assert!(support_persistence.contains("for share of accepted_grant"));
    for forbidden in [
        "actor_is_platform_admin",
        "store_outbox",
        "store_idempotency",
        "Redis",
        "a3s_lane",
        "insert into privileged_authorization",
        "privileged_authorization_distributed_lock",
    ] {
        assert!(
            !production_source(&persistence).contains(forbidden),
            "privileged authorization introduced duplicate or ambient authority {forbidden}"
        );
    }

    for required in [
        "pub credential_id: ApiTokenId",
        "PrivilegedCredentialDecisionEvidence",
        "credential.is_active_at(decided_at)",
        "required_credential_scope",
        "self.authentication != self.credential.authentication()?",
        "grant.id != grant_id",
    ] {
        assert!(
            decision.contains(required),
            "privileged decision lost credential or exact-grant evidence {required}"
        );
    }
    let request_fields = decision
        .split_once("pub struct PrivilegedAuthorizationDecisionRequest {")
        .expect("privileged request declaration")
        .1
        .split_once('}')
        .expect("privileged request fields")
        .0;
    assert!(request_fields.contains("pub credential_id: ApiTokenId"));
    assert!(!request_fields.contains("authentication"));
    assert!(application.contains("IPrivilegedAuthorizationDecisionRepository"));
    assert!(application.contains("request.validate()"));

    assert_eq!(provider_gate.matches("tokio::join!").count(), 4);
    for required in [
        "revoke_platform_role_binding",
        "repository_b.revoke(",
        "revoke_tenant_support_grant",
        "a revoked platform binding must never retain Installation catalog access",
        "every successful standalone allow and only a successful standalone allow",
        "each protected RBAC/support mutation must persist one decision",
        "a request-time allow must not introduce a second event mechanism",
    ] {
        assert!(
            provider_gate.contains(required),
            "privileged authorization provider gate lost concurrency proof {required}"
        );
    }
    assert!(workflow
        .contains("postgres_privileged_authorization_decisions_are_atomic_and_revocation_safe"));
}

#[test]
fn build_plan_source_layout_acquisition_reuses_one_sources_access_authority() {
    let adapter_path = "sources/infrastructure/developer_workflow_source_layout.rs";
    let adapter = std::fs::read_to_string(module_root().join(adapter_path))
        .expect("read Sources BuildPlan source-layout adapter");
    let production = production_source(&adapter);
    let compact = production.split_whitespace().collect::<String>();
    for required in [
        "implIBuildPlanSourceLayoutPortforDeveloperWorkflowSourceLayoutAdapter",
        "Arc<dynISourceBuildInputQueryPort>",
        "Arc<dynIAuthorizedSourceCheckout>",
        "SourceLayoutSnapshot::new(",
        ".find_source_build_input(",
        ".checkout(",
        ".replay(",
        ".remove(",
    ] {
        assert!(
            compact.contains(required),
            "trusted source-layout acquisition lost boundary {required}"
        );
    }
    for forbidden in [
        "ISourceRevisionRepository",
        "IGithubConnectionRepository",
        "IGithubInstallationTokenService",
        "SourceProviderCredential",
        "GitSourceCheckout",
        "read_dir(",
        "Postgres",
        "IOutboxRepository",
        "IEventPublisher",
        "CommandHandler",
        "tokio::spawn",
    ] {
        assert!(
            !production.contains(forbidden),
            "source-layout adapter acquired an owner repository, credential, traversal, persistence, or delivery mechanism {forbidden}"
        );
    }

    let checkout = std::fs::read_to_string(
        module_root().join("sources/application/authorized_source_checkout.rs"),
    )
    .expect("read authorized Source checkout service");
    let production_checkout = production_source(&checkout);
    let compact_checkout = production_checkout.split_whitespace().collect::<String>();
    for required in [
        "Arc<dynISourceCheckout>",
        "Arc<dynISourceRepositoryCredentialProvider>",
        "implIAuthorizedSourceCheckoutforAuthorizedSourceCheckoutService",
    ] {
        assert!(
            compact_checkout.contains(required),
            "the sole authorized checkout service lost owner mechanism {required}"
        );
    }
    for forbidden in [
        "IGithubConnectionRepository",
        "IGithubInstallationTokenService",
        "GithubInstallationTokenRequest",
    ] {
        assert!(
            !production_checkout.contains(forbidden),
            "authorized checkout duplicated repository credential mechanism {forbidden}"
        );
    }
    let strict_replay = production_checkout
        .rsplit_once("async fn replay(")
        .map(|(_, body)| body)
        .and_then(|body| body.split_once("async fn remove(").map(|(body, _)| body))
        .expect("authorized checkout strict replay body");
    assert!(strict_replay.contains("self.checkout.replay(request)"));
    for forbidden in ["credentials", ".checkout("] {
        assert!(
            !strict_replay.contains(forbidden),
            "strict checkout replay reacquired provider bytes through {forbidden}"
        );
    }

    let git_checkout = std::fs::read_to_string(
        module_root().join("sources/infrastructure/git_source_checkout.rs"),
    )
    .expect("read Git source checkout");
    let strict_git_replay = git_checkout
        .rsplit_once("async fn replay(")
        .map(|(_, body)| body)
        .and_then(|body| body.split_once("async fn remove(").map(|(body, _)| body))
        .expect("Git checkout strict replay body");
    for required in [
        "canonical_existing_root",
        "self.replay_at(request, &checkout)",
    ] {
        assert!(strict_git_replay.contains(required));
    }
    for forbidden in ["ensure_root(", "self.prepare(", "self.git(", ".checkout("] {
        assert!(
            !strict_git_replay.contains(forbidden),
            "strict Git replay can recreate or reacquire source bytes through {forbidden}"
        );
    }

    let credentials = std::fs::read_to_string(
        module_root().join("sources/application/source_repository_credential.rs"),
    )
    .expect("read Source repository credential service");
    let production_credentials = production_source(&credentials);
    let compact_credentials = production_credentials
        .split_whitespace()
        .collect::<String>();
    for required in [
        "Arc<dynIGithubConnectionRepository>",
        "Arc<dynIGithubInstallationTokenService>",
        "implISourceRepositoryCredentialProviderforSourceRepositoryCredentialService",
    ] {
        assert!(
            compact_credentials.contains(required),
            "the sole repository credential service lost owner mechanism {required}"
        );
    }

    let resolution = std::fs::read_to_string(
        module_root()
            .join("sources/application/commands/resolve_external_source_revision/handler.rs"),
    )
    .expect("read Source revision resolution handler");
    assert!(resolution.contains("Arc<dyn ISourceRepositoryCredentialProvider>"));
    for forbidden in [
        "IGithubConnectionRepository",
        "IGithubInstallationTokenService",
        "GithubInstallationTokenRequest",
    ] {
        assert!(
            !resolution.contains(forbidden),
            "Source resolution duplicated repository credential mechanism {forbidden}"
        );
    }

    let archive = std::fs::read_to_string(
        module_root().join("sources/infrastructure/external_build_archive.rs"),
    )
    .expect("read external Source archive adapter");
    let production_archive = production_source(&archive);
    let compact_archive = production_archive.split_whitespace().collect::<String>();
    assert!(production_archive.contains("Arc<dyn IAuthorizedSourceCheckout>"));
    assert_eq!(
        compact_archive
            .matches(".checkout(request.organization_id(),&checkout_request)")
            .count(),
        1
    );
    assert_eq!(production_archive.matches(".replay(").count(), 1);
    for forbidden in [
        "IGithubConnectionRepository",
        "IGithubInstallationTokenService",
        "GithubInstallationTokenRequest",
        "SourceProviderCredential",
    ] {
        assert!(
            !production_archive.contains(forbidden),
            "external archive duplicated authorized checkout mechanism {forbidden}"
        );
    }

    let app = std::fs::read_to_string(
        module_root()
            .parent()
            .expect("src directory")
            .join("app.rs"),
    )
    .expect("read application composition");
    for constructor in [
        "GitSourceCheckout::new(",
        "SourceRepositoryCredentialService::new(",
        "AuthorizedSourceCheckoutService::new(",
        "SourceBuildInputQueryService::new(",
        "DeveloperWorkflowSourceLayoutAdapter::new(",
    ] {
        assert_eq!(
            app.matches(constructor).count(),
            1,
            "production composition must select {constructor} exactly once"
        );
    }
}

#[test]
fn developer_workflows_acceptance_reuses_owner_authorization_interfaces() {
    let adapter_path = "developer_workflows/infrastructure/authorization.rs";
    let adapter = std::fs::read_to_string(module_root().join(adapter_path))
        .expect("read Developer Workflows authorization adapter");
    let production = production_source(&adapter);
    let compact = production.split_whitespace().collect::<String>();

    for required in [
        "Arc<dynIMembershipRepository>",
        "Arc<dynIResourceGrantRepository>",
        "Arc<dynIEnvironmentRepository>",
        "ResourceAccessEvaluator::for_membership",
        "implIDeveloperWorkflowAuthorizationPort",
    ] {
        assert!(
            compact.contains(required),
            "Developer Workflows acceptance lost its owner interface boundary {required}"
        );
    }
    for forbidden in [
        "Postgres",
        "a3s_orm",
        "sqlx",
        "ApiToken",
        "IOutboxRepository",
        "IIntegrationEventProjector",
        "CommandBus",
        "CommandHandler",
        "tokio::spawn",
    ] {
        assert!(
            !production.contains(forbidden),
            "Developer Workflows authorization adapter introduced a concrete or duplicate mechanism {forbidden}"
        );
    }

    for (label, relative) in [
        ("BuildPlan", "developer_workflows/application/acceptance.rs"),
        (
            "workload profile",
            "developer_workflows/application/workload_profile_acceptance.rs",
        ),
        (
            "Preview Policy",
            "developer_workflows/application/preview_policy_acceptance.rs",
        ),
    ] {
        let acceptance = std::fs::read_to_string(module_root().join(relative))
            .unwrap_or_else(|error| panic!("read {label} acceptance handler: {error}"));
        let production_acceptance = production_source(&acceptance);
        for forbidden in [
            "crate::modules::identity",
            "crate::modules::projects",
            "crate::modules::sources",
            "ResourceAccessEvaluator",
            "Postgres",
        ] {
            assert!(
                !production_acceptance.contains(forbidden),
                "{label} Application handler imported foreign owner policy or infrastructure {forbidden}"
            );
        }
    }
}

#[test]
fn sources_preview_handoff_has_one_interface_boundary_and_no_second_delivery_mechanism() {
    let projector_path = "sources/infrastructure/pull_request_preview_source_projector.rs";
    let projector = std::fs::read_to_string(module_root().join(projector_path))
        .expect("read Sources Preview projector");
    assert!(
        projector.contains("developer_workflows::published")
            && !projector.contains("developer_workflows::domain")
            && projector.contains("Arc<dyn IPreviewSourceRevisionProjectionPort>"),
        "Sources must consume only Developer Workflows Published Language through its own Application port"
    );

    let app = std::fs::read_to_string(
        module_root()
            .parent()
            .expect("src directory")
            .join("app.rs"),
    )
    .expect("read application composition");
    assert_eq!(
        app.matches("PullRequestPreviewSourceProjector::new")
            .count(),
        1,
        "all process roles must share one Sources Preview projector composition path"
    );
    let environments = app
        .find("PullRequestPreviewProjector::new")
        .expect("Projects Preview handoff composition");
    let sources = app
        .find("PullRequestPreviewSourceProjector::new")
        .expect("Sources Preview handoff composition");
    assert!(
        environments < sources,
        "active Preview Environment ownership must be handed off before Sources creates its ordinary SourceRevision"
    );

    for relative in [
        "sources/application/preview_source_revision_projection.rs",
        projector_path,
        "sources/published/preview_source_revision_lifecycle.rs",
    ] {
        let source = std::fs::read_to_string(module_root().join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        for forbidden in [
            "IOutboxRepository",
            "IEventPublisher",
            "OutboxRelay",
            "NatsProvider",
            "tokio::spawn",
            "SOURCE_REVISION_ACCEPTED_EVENT_KEY",
        ] {
            assert!(
                !source.contains(forbidden),
                "{relative} introduced duplicate delivery or bypassed version fencing through {forbidden}"
            );
        }
    }
}

#[test]
fn published_languages_never_alias_owner_domain_models() {
    let mut violations = BTreeSet::new();

    visit_production_sources(|relative, source| {
        if layer(relative) != Some("published") {
            return;
        }
        let Some(owner) = context(relative) else {
            return;
        };
        let owner_domain = format!("crate::modules::{owner}::domain");
        for line in source.lines().filter(|line| line.contains(&owner_domain)) {
            violations.insert(format!("{} contains {line:?}", display(relative)));
        }
    });

    assert!(
        violations.is_empty(),
        "a published language aliases its owner's internal domain model:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn shared_kernel_never_depends_on_a_bounded_context() {
    let mut violations = BTreeSet::new();

    visit_production_sources(|relative, source| {
        if context(relative) != Some("shared_kernel") {
            return;
        }
        for target in module_references(source) {
            if target != "shared_kernel" {
                violations.insert(format!("{} -> {target}", display(relative)));
            }
        }
    });

    assert!(
        violations.is_empty(),
        "the Shared Kernel accumulated a bounded-context dependency:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn search_visibility_and_composition_stay_behind_the_owner_boundary() {
    let root = module_root();
    let facade =
        std::fs::read_to_string(root.join("search/mod.rs")).expect("read Search module facade");
    for required in [
        "mod infrastructure;",
        "mod presentation;",
        "use infrastructure::PostgresSearchRepository;",
        "pub(crate) fn search_persistence_adapter(",
        ") -> Arc<dyn ISearchRepository>",
        "pub(crate) use presentation::{SearchModule, SearchResultResponse};",
    ] {
        assert!(
            facade.contains(required),
            "Search facade lost its crate-private composition boundary {required}"
        );
    }
    for forbidden in [
        "pub mod infrastructure;",
        "pub mod presentation;",
        "pub use infrastructure",
        "pub use presentation",
        "pub(crate) use infrastructure::PostgresSearchRepository",
    ] {
        assert!(
            !facade.contains(forbidden),
            "Search facade exposed an outer-layer implementation with {forbidden}"
        );
    }

    for relative in [
        "search/mod.rs",
        "search/infrastructure/mod.rs",
        "search/infrastructure/persistence/mod.rs",
    ] {
        let source = std::fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        let production = production_source(&source);
        assert!(
            production.contains("PostgresSearchRepository"),
            "Search production persistence disappeared from architecture scanning at {relative}"
        );
        assert!(
            !production.contains("InMemorySearchRepository"),
            "Search test persistence entered the production module graph through {relative}"
        );
    }
    let postgres =
        std::fs::read_to_string(root.join("search/infrastructure/persistence/postgres.rs"))
            .expect("read Search PostgreSQL adapter");
    assert!(postgres.contains("pub(in crate::modules::search) struct PostgresSearchRepository"));
    assert!(postgres
        .contains("pub(in crate::modules::search) const fn new(executor: PostgresExecutor)"));

    let mut identity_dependencies = BTreeSet::new();
    visit_production_sources(|relative, source| {
        if context(relative) != Some("search")
            || !matches!(
                layer(relative),
                Some("application" | "domain" | "infrastructure")
            )
        {
            return;
        }
        for line in source
            .lines()
            .filter(|line| line.contains("crate::modules::identity"))
        {
            identity_dependencies.insert(format!("{} contains {line:?}", display(relative)));
        }
    });
    assert!(
        identity_dependencies.is_empty(),
        "Search owner code imported Identity's evaluator instead of its bounded visibility contract:\n{}",
        identity_dependencies.into_iter().collect::<Vec<_>>().join("\n")
    );

    let visibility =
        std::fs::read_to_string(root.join("search/domain/value_objects/search_visibility.rs"))
            .expect("read Search visibility contract");
    for required in [
        "pub enum SearchVisibilityScope",
        "pub struct SearchVisibility",
        "pub fn projected_resource_is_visible",
    ] {
        assert!(
            visibility.contains(required),
            "Search visibility contract lost {required}"
        );
    }
    for forbidden in ["ResourceAccessEvaluator", "ResourceGrantScope"] {
        assert!(
            !visibility.contains(forbidden),
            "Search visibility contract copied Identity authority {forbidden}"
        );
    }

    let controller =
        std::fs::read_to_string(root.join("search/presentation/controllers/search_controller.rs"))
            .expect("read Search HTTP adapter");
    for required in ["crate::presentation::{", "search_visibility", "request_id"] {
        assert!(
            controller.contains(required),
            "Search HTTP adapter stopped using root Presentation {required}"
        );
    }
    for forbidden in ["identity::presentation", "fn request_id("] {
        assert!(
            !controller.contains(forbidden),
            "Search HTTP adapter regained duplicate outer-layer mechanism {forbidden}"
        );
    }

    let management_mcp = std::fs::read_to_string(
        root.parent()
            .expect("src directory")
            .join("presentation/management_mcp/search.rs"),
    )
    .expect("read Search Management MCP adapter");
    assert!(management_mcp.contains("crate::presentation::search_visibility"));
    assert!(management_mcp.contains("search_visibility(&resource_access)"));
    for forbidden in ["ResourceGrantScope", "SearchVisibility::"] {
        assert!(
            !management_mcp.contains(forbidden),
            "Search Management MCP adapter duplicated visibility translation with {forbidden}"
        );
    }

    let request_context = std::fs::read_to_string(
        root.parent()
            .expect("src directory")
            .join("presentation/request_context.rs"),
    )
    .expect("read root Presentation request context");
    assert_eq!(request_context.matches("fn search_visibility(").count(), 1);
    assert!(request_context.contains("ResourceGrantScope::Project"));
    assert!(request_context.contains("ResourceGrantScope::Environment"));
    assert!(request_context.contains("ResourceGrantScope::Node"));

    let conformance =
        std::fs::read_to_string(root.parent().expect("src directory").join("conformance.rs"))
            .expect("read persistence conformance assembly");
    assert!(conformance.contains(") -> Arc<dyn ISearchRepository>"));
    assert_eq!(
        conformance
            .matches("search_persistence_adapter(executor)")
            .count(),
        1
    );
    assert!(!conformance.contains("PostgresSearchRepository"));
    for line in conformance
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub "))
    {
        assert!(
            !line.contains("PostgresSearchRepository") && !line.starts_with("pub use "),
            "Search persistence conformance exposed a concrete adapter: {line}"
        );
    }

    let adapters = std::fs::read_to_string(
        root.parent()
            .expect("src directory")
            .join("app/postgres_adapters.rs"),
    )
    .expect("read sole PostgreSQL adapter factory");
    assert!(adapters.contains("fn search(&self) -> Arc<dyn ISearchRepository>"));
    assert!(adapters.contains("search_persistence_adapter(self.executor.clone())"));
    assert!(!adapters.contains("PostgresSearchRepository"));
}

#[test]
fn security_composition_stays_behind_owner_and_root_presentation_boundaries() {
    let root = module_root();
    let facade =
        std::fs::read_to_string(root.join("security/mod.rs")).expect("read Security facade");
    for required in [
        "mod infrastructure;",
        "mod presentation;",
        "use infrastructure::PostgresGatewayRoutePolicyTimelineRepository;",
        "pub(crate) fn security_persistence_adapter(",
        ") -> Arc<dyn IGatewayRoutePolicyTimelineRepository>",
        "pub(crate) use presentation::{GatewayRoutePolicyTimelinePageResponse, SecurityModule};",
    ] {
        assert!(
            facade.contains(required),
            "Security facade lost its owner boundary {required}"
        );
    }
    for forbidden in [
        "pub mod infrastructure;",
        "pub mod presentation;",
        "pub use infrastructure",
        "pub use presentation",
        "pub(crate) use infrastructure::PostgresGatewayRoutePolicyTimelineRepository",
    ] {
        assert!(
            !facade.contains(forbidden),
            "Security facade exposed an outer-layer implementation with {forbidden}"
        );
    }

    for relative in ["security/mod.rs", "security/infrastructure/mod.rs"] {
        let source = std::fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        let production = production_source(&source);
        assert!(
            production.contains("PostgresGatewayRoutePolicyTimelineRepository"),
            "Security production persistence disappeared from architecture scanning at {relative}"
        );
        assert!(
            !production.contains("InMemoryGatewayRoutePolicyTimelineRepository"),
            "Security test persistence entered the production module graph through {relative}"
        );
    }
    let postgres = std::fs::read_to_string(root.join("security/infrastructure/postgres.rs"))
        .expect("read Security PostgreSQL adapter");
    assert!(postgres.contains(
        "pub(in crate::modules::security) struct PostgresGatewayRoutePolicyTimelineRepository"
    ));
    assert!(postgres
        .contains("pub(in crate::modules::security) const fn new(executor: PostgresExecutor)"));

    let controller = std::fs::read_to_string(root.join("security/presentation/controller.rs"))
        .expect("read Security HTTP adapter");
    for required in [
        "crate::presentation::{",
        "organization_administrator_read_controller",
        "request_id",
    ] {
        assert!(
            controller.contains(required),
            "Security HTTP adapter stopped using root Presentation {required}"
        );
    }
    for forbidden in [
        "crate::modules::identity",
        "OrganizationAdministratorGuard",
        "OrganizationTenantGuard",
        "AUTH_SCOPES_METADATA",
        "fn request_id(",
    ] {
        assert!(
            !controller.contains(forbidden),
            "Security HTTP adapter regained duplicate authorization/request mechanism {forbidden}"
        );
    }

    let management_mcp = std::fs::read_to_string(
        root.parent()
            .expect("src directory")
            .join("presentation/management_mcp/security.rs"),
    )
    .expect("read Security Management MCP adapter");
    assert!(management_mcp.contains("crate::modules::security::{"));
    assert!(!management_mcp.contains("security::presentation"));

    let root_presentation = std::fs::read_to_string(
        root.parent()
            .expect("src directory")
            .join("presentation/mod.rs"),
    )
    .expect("read root Presentation composition");
    assert_eq!(
        root_presentation
            .matches("fn organization_administrator_read_controller(")
            .count(),
        1
    );
    for required in [
        "OrganizationAdministratorGuard",
        "OrganizationTenantGuard",
        "ApiTokenScope::CLOUD_READ",
        "AUTH_SCOPES_METADATA",
    ] {
        assert!(root_presentation.contains(required));
    }

    let conformance =
        std::fs::read_to_string(root.parent().expect("src directory").join("conformance.rs"))
            .expect("read persistence conformance assembly");
    assert!(conformance.contains(") -> Arc<dyn IGatewayRoutePolicyTimelineRepository>"));
    assert_eq!(
        conformance
            .matches("security_persistence_adapter(executor)")
            .count(),
        1
    );
    assert!(!conformance.contains("PostgresGatewayRoutePolicyTimelineRepository"));

    let ci = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/ci.yml"),
    )
    .expect("read CI workflow");
    let security_gate = workflow_step(
        &ci,
        "Certify Gateway Route policy security timeline persistence",
    );
    for required in [
        "--features persistence-conformance",
        "postgres_gateway_route_policy_security_timeline_is_typed_correlated_and_tenant_scoped",
    ] {
        assert!(
            security_gate.contains(required),
            "Security persistence gate lost {required}"
        );
    }

    let adapters = std::fs::read_to_string(
        root.parent()
            .expect("src directory")
            .join("app/postgres_adapters.rs"),
    )
    .expect("read sole PostgreSQL adapter factory");
    assert!(adapters.contains(
        "fn security_investigations(&self) -> Arc<dyn IGatewayRoutePolicyTimelineRepository>"
    ));
    assert!(adapters.contains("security_persistence_adapter(self.executor.clone())"));
    assert!(!adapters.contains("PostgresGatewayRoutePolicyTimelineRepository"));
}

#[test]
fn public_outer_layer_facade_debt_can_only_shrink() {
    let allowed = lines(
        r#"
agents -> infrastructure
agents -> presentation
applications -> infrastructure
applications -> presentation
artifacts -> infrastructure
assets -> infrastructure
assets -> presentation
audit -> infrastructure
audit -> presentation
connectors -> infrastructure
connectors -> presentation
developer_workflows -> infrastructure
durable_cells -> infrastructure
durable_cells -> presentation
edge -> infrastructure
edge -> presentation
executions -> infrastructure
executions -> presentation
fleet -> infrastructure
fleet -> presentation
identity -> infrastructure
identity -> presentation
integration_events -> infrastructure
integration_events -> presentation
notifications -> infrastructure
notifications -> presentation
operations -> infrastructure
operations -> presentation
plugins -> infrastructure
plugins -> presentation
projects -> infrastructure
projects -> presentation
secrets -> infrastructure
secrets -> presentation
sources -> infrastructure
sources -> presentation
workflow -> infrastructure
workflow -> presentation
workloads -> infrastructure
workloads -> presentation
"#,
    );
    let mut actual = BTreeSet::new();

    visit_production_sources(|relative, source| {
        if relative.file_name().and_then(|value| value.to_str()) != Some("mod.rs")
            || relative.iter().count() != 2
        {
            return;
        }
        let Some(source_context) = context(relative) else {
            return;
        };
        for outer_layer in ["infrastructure", "presentation"] {
            let exposes_module = source
                .lines()
                .any(|line| line.trim() == format!("pub mod {outer_layer};"));
            let exposes_implementation = production_source(source)
                .split(';')
                .map(str::trim)
                .filter(|statement| statement.starts_with("pub "))
                .any(|statement| {
                    statement
                        .split(|character: char| {
                            !(character.is_ascii_alphanumeric() || character == '_')
                        })
                        .any(|segment| segment == outer_layer)
                });
            if exposes_module || exposes_implementation {
                actual.insert(format!("{source_context} -> {outer_layer}"));
            }
        }
    });

    let unexpected = difference(&actual, &allowed);
    let resolved = difference(&allowed, &actual);
    assert!(
        unexpected.is_empty(),
        "a bounded context publicly exposed a new outer-layer declaration, re-export, or alias:\n{}",
        unexpected.join("\n")
    );
    assert!(
        resolved.is_empty(),
        "resolved public outer-layer facade debt must be removed from the exact allowlist:\n{}",
        resolved.join("\n")
    );
}

#[test]
fn agents_release_admission_has_one_owner_port_and_one_cross_context_adapter() {
    let port = std::fs::read_to_string(
        module_root().join("agents/application/agent_release_admission.rs"),
    )
    .expect("read Agents release-admission port");
    let compact_port = production_source(&port)
        .split_whitespace()
        .collect::<String>();
    for required in [
        "pubstructAgentReleaseAdmissionRequest",
        "pubtraitIAgentReleaseAdmissionPort:Send+Sync",
        "ApplicationResult<AgentReleaseBinding>",
    ] {
        assert!(
            compact_port.contains(required),
            "Agents release admission lost its consumer-owned contract {required}"
        );
    }
    for forbidden in [
        "crate::modules::assets",
        "crate::modules::artifacts",
        "IAssetRepository",
        "IHostedArtifactQueryPort",
        "DeployableAgentRelease",
        "Postgres",
        "InMemory",
    ] {
        assert!(
            !port.contains(forbidden),
            "the Agents-owned release-admission port imported foreign or concrete authority {forbidden}"
        );
    }

    for relative in [
        "agents/application/commands/start_agent_execution/handler.rs",
        "agents/application/commands/fork_agent_execution/handler.rs",
        "agents/application/workflow_agent_port.rs",
    ] {
        let source = std::fs::read_to_string(module_root().join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        let production = production_source(&source);
        let compact = production.split_whitespace().collect::<String>();
        assert!(
            compact.contains("Arc<dynIAgentReleaseAdmissionPort>")
                && compact.contains(".admit(AgentReleaseAdmissionRequest{"),
            "{relative} stopped entering release admission through the one Agents-owned port"
        );
        for forbidden in [
            "crate::modules::assets",
            "crate::modules::artifacts",
            "IAssetRepository",
            "IHostedArtifactQueryPort",
            "DeployableAgentRelease",
            "load_deployable_agent_release",
        ] {
            assert!(
                !production.contains(forbidden),
                "{relative} bypassed release admission with foreign authority {forbidden}"
            );
        }
    }

    let adapter = std::fs::read_to_string(
        module_root().join("agents/infrastructure/agent_release_admission.rs"),
    )
    .expect("read Agents release-admission adapter");
    let compact_adapter = production_source(&adapter)
        .split_whitespace()
        .collect::<String>();
    for required in [
        "implIAgentReleaseAdmissionPortforAssetsAgentReleaseAdmissionAdapter",
        "assets:Arc<dynIAssetRepository>",
        "artifacts:Arc<dynIHostedArtifactQueryPort>",
        "AgentReleaseBinding::new(",
    ] {
        assert!(
            compact_adapter.contains(required),
            "the sole Agents release-admission adapter lost boundary behavior {required}"
        );
    }
    assert_eq!(
        adapter.matches("load_deployable_agent_release(").count(),
        1,
        "Agents release admission must compose the owner query exactly once"
    );
    for forbidden in [
        "Postgres",
        "InMemory",
        "IOutboxRepository",
        "IIntegrationEventProjector",
        "CommandHandler",
        "tokio::spawn",
    ] {
        assert!(
            !production_source(&adapter).contains(forbidden),
            "the release-admission adapter introduced concrete state or lifecycle mechanism {forbidden}"
        );
    }

    let app = std::fs::read_to_string(
        module_root()
            .parent()
            .expect("control-plane source root")
            .join("app.rs"),
    )
    .expect("read control-plane composition root");
    assert_eq!(
        app.matches("AssetsAgentReleaseAdmissionAdapter::new(")
            .count(),
        2,
        "API and worker composition must reuse the same adapter type"
    );
}

#[test]
fn applications_enter_workflow_timeout_admission_through_one_owner_adapter() {
    let port = std::fs::read_to_string(
        module_root().join("applications/application/workflow_run_port.rs"),
    )
    .expect("read Applications WorkflowRun port");
    let compact_port = production_source(&port)
        .split_whitespace()
        .collect::<String>();
    assert!(
        compact_port.contains(
            "fnadmit_timeout_seconds(&self,requested:Option<u64>)->ApplicationResult<u64>;"
        ),
        "Applications lost its consumer-owned Workflow timeout admission contract"
    );
    assert!(
        !production_source(&port).contains("crate::modules::workflow"),
        "the Applications-owned port imported Workflow internals"
    );

    let mut application_violations = BTreeSet::new();
    visit_production_sources(|relative, source| {
        if context(relative) == Some("applications")
            && layer(relative) == Some("application")
            && source.contains("crate::modules::workflow")
        {
            application_violations.insert(display(relative));
        }
    });
    assert!(
        application_violations.is_empty(),
        "Applications Application bypassed its Workflow port:\n{}",
        application_violations
            .into_iter()
            .collect::<Vec<_>>()
            .join("\n")
    );

    for relative in [
        "applications/application/invocation_commands.rs",
        "applications/application/delivery_commands.rs",
    ] {
        let source = std::fs::read_to_string(module_root().join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        assert_eq!(
            production_source(&source)
                .matches(".admit_timeout_seconds(")
                .count(),
            1,
            "{relative} must enter timeout admission exactly once through the owner adapter"
        );
    }

    let authority = std::fs::read_to_string(
        module_root().join("applications/domain/application_invocation_workflow_authority.rs"),
    )
    .expect("read Applications Workflow authority");
    for forbidden in [
        "APPLICATION_INVOCATION_MAX_TIMEOUT_SECONDS",
        "30 * 24 * 60 * 60",
    ] {
        assert!(
            !production_source(&authority).contains(forbidden),
            "Applications Domain copied the Workflow timeout policy {forbidden}"
        );
    }

    let adapter =
        std::fs::read_to_string(module_root().join("applications/infrastructure/workflow_run.rs"))
            .expect("read Workflow-owned Applications adapter");
    let compact_adapter = production_source(&adapter)
        .split_whitespace()
        .collect::<String>();
    assert!(
        compact_adapter.contains(
            "fnadmit_timeout_seconds(&self,requested:Option<u64>)->ApplicationResult<u64>"
        ),
        "the Workflow adapter stopped implementing timeout admission"
    );
    assert_eq!(
        production_source(&adapter)
            .matches("workflow_run_timeout_seconds(")
            .count(),
        1,
        "the Workflow timeout rule must have one Applications adapter entry point"
    );

    for relative in [
        "../presentation/api_contract/operation.rs",
        "../presentation/management_mcp/catalog.rs",
    ] {
        let source = std::fs::read_to_string(module_root().join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        assert!(
            !source.contains("\"maximum\": 2592000"),
            "{relative} copied the Workflow timeout maximum into a public schema"
        );
        assert!(
            !source.contains("\"default\": 86400"),
            "{relative} copied the Workflow timeout default into a public schema"
        );
    }

    let management_workflow =
        std::fs::read_to_string(module_root().join("../presentation/management_mcp/workflow.rs"))
            .expect("read Workflow Management MCP presentation");
    let management_workflow = production_source(&management_workflow);
    assert_eq!(
        management_workflow
            .matches("workflow_run_timeout_seconds(")
            .count(),
        1,
        "Management MCP must delegate timeout validation to Workflow's owning rule"
    );
    assert!(
        !management_workflow.contains("WORKFLOW_RUN_MAX_TIMEOUT_SECONDS"),
        "Management MCP copied Workflow's timeout-bound validation mechanism"
    );
}

#[test]
fn workflow_owns_human_task_submission_through_one_forms_adapter_and_mapper() {
    let port =
        std::fs::read_to_string(module_root().join("workflow/application/human_task_form_port.rs"))
            .expect("read Workflow HumanTask Form port");
    let compact_port = production_source(&port)
        .split_whitespace()
        .collect::<String>();
    assert!(
        compact_port.contains("traitIHumanTaskFormPort:Send+Sync"),
        "Workflow lost its consumer-owned HumanTask Form boundary"
    );
    assert!(
        !production_source(&port).contains("crate::modules::forms"),
        "the consumer-owned HumanTask Form port imported Forms internals"
    );

    let handler = std::fs::read_to_string(
        module_root().join("workflow/application/commands/submit_human_task/handler.rs"),
    )
    .expect("read SubmitHumanTask handler");
    let handler = production_source(&handler);
    assert!(handler.contains("IHumanTaskFormPort"));
    assert_eq!(handler.matches(".evaluate_submission(").count(), 1);
    assert!(
        !handler.contains("crate::modules::forms"),
        "Workflow Application bypassed its Forms port"
    );

    let mut forms_import_sites = BTreeSet::new();
    let mut port_implementations = BTreeSet::new();
    let mut submission_mappers = BTreeSet::new();
    let mut removed_mechanisms = BTreeSet::new();
    visit_production_sources(|relative, source| {
        let source = production_source(source);
        if context(relative) == Some("workflow") && source.contains("crate::modules::forms") {
            forms_import_sites.insert(display(relative));
        }
        if source.contains("impl IHumanTaskFormPort for") {
            port_implementations.insert(display(relative));
        }
        if source.contains("=> \"form_submissions\"") {
            submission_mappers.insert(display(relative));
        }
        if source.contains("IFormSubmissionRepository")
            || source.contains("PostgresFormSubmissionRepository")
        {
            removed_mechanisms.insert(display(relative));
        }
    });
    assert_eq!(
        forms_import_sites,
        BTreeSet::from(["workflow/infrastructure/human_task_form.rs".to_owned()]),
        "all Workflow-to-Forms access must be confined to the sole consumer-side adapter"
    );
    assert_eq!(
        port_implementations,
        BTreeSet::from(["workflow/infrastructure/human_task_form.rs".to_owned()]),
        "HumanTask Form access must have one consumer-side adapter"
    );
    assert_eq!(
        submission_mappers,
        BTreeSet::from([
            "workflow/infrastructure/persistence/human_task_postgres/schema.rs".to_owned()
        ]),
        "HumanTask submission evidence must have one Workflow-owned table mapper"
    );
    assert!(
        removed_mechanisms.is_empty(),
        "the removed standalone Form submission repository returned:\n{}",
        removed_mechanisms
            .into_iter()
            .collect::<Vec<_>>()
            .join("\n")
    );

    let forms_domain = module_root().join("forms/domain");
    assert!(!forms_domain.join("entities/form_submission.rs").exists());
    assert!(!forms_domain
        .join("repositories/form_submission_repository.rs")
        .exists());
    let decision_record = std::fs::read_to_string(
        module_root().join("workflow/domain/repositories/human_task_repository.rs"),
    )
    .expect("read Workflow HumanTask repository contract");
    assert!(
        production_source(&decision_record).contains("pub submission: Option<HumanTaskSubmission>"),
        "Workflow no longer owns immutable HumanTaskSubmission evidence"
    );

    let coordinator = std::fs::read_to_string(
        module_root().join("workflow/infrastructure/human_task_flow/coordinator.rs"),
    )
    .expect("read HumanTask coordinator");
    let coordinator = production_source(&coordinator);
    assert!(coordinator.contains("IHumanTaskFormPort"));
    assert_eq!(
        coordinator.matches(".resolve_interaction_release(").count(),
        1
    );
    assert!(!coordinator.contains("IFormRepository"));
}

fn foreign_outer_layer_sites() -> BTreeSet<String> {
    let mut sites = BTreeSet::new();
    visit_production_sources(|relative, source| {
        let Some(source_context) = context(relative) else {
            return;
        };
        for (target_context, target_layer) in outer_layer_references(source) {
            if target_context != source_context {
                sites.insert(format!(
                    "{} -> {target_context}/{target_layer}",
                    display(relative)
                ));
            }
        }
    });
    sites
}

fn physical_table_mappings() -> BTreeMap<String, BTreeSet<String>> {
    let mut mappings: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    visit_production_sources(|relative, source| {
        let expected_mappings = source.matches("orm_table!").count();
        if expected_mappings == 0 {
            return;
        }

        let mut parsed_mappings = 0;
        for line in source.lines() {
            let Some(marker) = line.find("=> \"") else {
                continue;
            };
            let prefix = &line[..marker];
            let words = prefix.split_whitespace().collect::<Vec<_>>();
            let Some(struct_index) = words.iter().position(|word| *word == "struct") else {
                continue;
            };
            let Some(struct_name) = words.get(struct_index + 1) else {
                continue;
            };
            let table_start = marker + 4;
            let Some(table_end) = line[table_start..].find('"') else {
                continue;
            };
            let table = &line[table_start..table_start + table_end];
            mappings
                .entry(table.into())
                .or_default()
                .insert(format!("{}#{struct_name}", display(relative)));
            parsed_mappings += 1;
        }

        assert_eq!(
            parsed_mappings,
            expected_mappings,
            "architecture test could not parse every orm_table! mapping in {}",
            display(relative)
        );
    });

    mappings
}

fn outer_layer_references(source: &str) -> BTreeSet<(String, &'static str)> {
    let mut references = BTreeSet::new();
    let mut remainder = source;
    const PREFIX: &str = "crate::modules::";

    while let Some(index) = remainder.find(PREFIX) {
        let after_prefix = &remainder[index + PREFIX.len()..];
        let target_end = after_prefix
            .find(|character: char| !(character.is_ascii_lowercase() || character == '_'))
            .unwrap_or(after_prefix.len());
        let target = &after_prefix[..target_end];
        let after_target = &after_prefix[target_end..];
        for candidate in ["infrastructure", "presentation"] {
            if after_target.starts_with(&format!("::{candidate}")) {
                references.insert((target.into(), candidate));
            }
        }
        remainder = &after_prefix[target_end..];
    }

    references
}

fn module_references(source: &str) -> BTreeSet<String> {
    let mut references = BTreeSet::new();
    let mut remainder = source;
    const PREFIX: &str = "crate::modules::";

    while let Some(index) = remainder.find(PREFIX) {
        let after_prefix = &remainder[index + PREFIX.len()..];
        let target_end = after_prefix
            .find(|character: char| !(character.is_ascii_lowercase() || character == '_'))
            .unwrap_or(after_prefix.len());
        if target_end > 0 {
            references.insert(after_prefix[..target_end].into());
        }
        remainder = &after_prefix[target_end..];
    }

    references
}

fn bounded_context_internal_model_references(
    bounded_context: &str,
    expected_layer: &str,
) -> BTreeSet<String> {
    let mut violations = BTreeSet::new();
    visit_production_sources(|relative, source| {
        if context(relative) != Some(bounded_context) || layer(relative) != Some(expected_layer) {
            return;
        }
        if source.contains("crate::modules::{") || source.contains("crate::{") {
            violations.insert(format!(
                "{} uses a grouped module root that bypasses exact boundary inspection",
                display(relative)
            ));
        }
        if source.lines().any(|line| {
            let line = line.trim();
            line == "use crate::modules;" || line.starts_with("use crate::modules as ")
        }) {
            violations.insert(format!(
                "{} aliases the module root and bypasses exact boundary inspection",
                display(relative)
            ));
        }
        if source.contains("super::super::") {
            violations.insert(format!(
                "{} uses a relative path that bypasses exact boundary inspection",
                display(relative)
            ));
        }
        for line in source
            .lines()
            .filter(|line| line.contains("crate::modules::"))
        {
            for target in module_references(line) {
                if target == bounded_context
                    || target == "shared_kernel"
                    || line.contains(&format!("crate::modules::{target}::published"))
                {
                    continue;
                }
                violations.insert(format!("{} contains {line:?}", display(relative)));
            }
        }
    });
    violations
}

fn visit_production_sources(mut visit: impl FnMut(&Path, &str)) {
    let root = module_root();
    let mut pending = vec![root.clone()];

    while let Some(path) = pending.pop() {
        if path.is_dir() {
            let entries = std::fs::read_dir(&path).expect("read module source directory");
            pending.extend(entries.map(|entry| entry.expect("read module source entry").path()));
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("rs") {
            continue;
        }

        let relative = path.strip_prefix(&root).expect("module source path");
        if is_test_only(relative) {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("read module source");
        let production = production_source(&source);
        visit(relative, &production);
    }
}

/// Keep production declarations that follow a test-only import, while still
/// excluding the inline test modules conventionally placed at the end of a
/// source file. The previous first-marker truncation let one early
/// `#[cfg(test)] use ...` hide the whole production file from architecture
/// fitness checks.
fn production_source(source: &str) -> String {
    let lines = source.lines().collect::<Vec<_>>();
    let mut production = String::with_capacity(source.len());
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        if is_test_only_cfg_attribute(line) {
            let mut item = index + 1;
            while item < lines.len() && lines[item].trim().is_empty() {
                item += 1;
            }
            if item < lines.len() && is_use_declaration(lines[item]) {
                index = item;
                while index < lines.len() && !lines[index].contains(';') {
                    index += 1;
                }
                index += 1;
                continue;
            }
            break;
        }
        production.push_str(line);
        production.push('\n');
        index += 1;
    }

    production
}

fn is_test_only_cfg_attribute(line: &str) -> bool {
    let line = line.trim();
    line == "#[cfg(test)]" || (line.starts_with("#[cfg(all(") && line.contains("test"))
}

fn is_use_declaration(line: &str) -> bool {
    let line = line.trim_start();
    if line.starts_with("use ") {
        return true;
    }
    let Some(visible) = line.strip_prefix("pub") else {
        return false;
    };
    let visible = visible.trim_start();
    if let Some(restricted) = visible.strip_prefix('(') {
        let Some((_, declaration)) = restricted.split_once(')') else {
            return false;
        };
        declaration.trim_start().starts_with("use ")
    } else {
        visible.starts_with("use ")
    }
}

#[test]
fn production_source_does_not_hide_code_after_a_test_only_import() {
    let source = "#[cfg(test)]\nuse crate::test_support::Fixture;\n#[cfg(test)]\npub(crate) use crate::test_support::{\n    AnotherFixture,\n};\n#[cfg(all(test, target_os = \"linux\"))]\npub use crate::test_support::LinuxFixture;\nuse crate::modules::foreign::infrastructure::Adapter;\n#[cfg(test)]\nmod tests;\n";
    let production = production_source(source);

    assert!(!production.contains("Fixture"));
    assert!(production.contains("foreign::infrastructure::Adapter"));
    assert!(!production.contains("mod tests"));
}

fn is_test_only(relative: &Path) -> bool {
    let file = relative.file_name().and_then(|value| value.to_str());
    let components = relative
        .iter()
        .filter_map(|value| value.to_str())
        .collect::<BTreeSet<_>>();

    components.contains("tests")
        || components.contains("test_support")
        || matches!(
            file,
            Some("architecture_tests.rs")
                | Some("authority_tests.rs")
                | Some("real_conformance.rs")
                | Some("test_support.rs")
                | Some("tests.rs")
        )
        || file.is_some_and(|name| name.ends_with("_tests.rs"))
}

fn module_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/modules")
}

fn workflow_step<'a>(workflow: &'a str, name: &str) -> &'a str {
    let marker = format!("      - name: {name}");
    let (_, tail) = workflow
        .split_once(&marker)
        .unwrap_or_else(|| panic!("workflow step is missing: {name}"));
    tail.split("\n      - name: ").next().unwrap_or(tail)
}

fn context(path: &Path) -> Option<&str> {
    path.iter().next().and_then(|value| value.to_str())
}

fn layer(path: &Path) -> Option<&str> {
    path.iter().nth(1).and_then(|value| value.to_str())
}

fn display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn lines(value: &str) -> BTreeSet<String> {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

fn difference(actual: &BTreeSet<String>, allowed: &BTreeSet<String>) -> Vec<String> {
    actual.difference(allowed).cloned().collect()
}
