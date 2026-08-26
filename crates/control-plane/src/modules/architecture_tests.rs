use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// These tests are executable architecture decisions, not a description of the
/// desired end state. Existing violations are recorded as removable debt: a
/// site may disappear, but a new site cannot be added without changing an
/// explicit architecture decision in this file.

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
edge/infrastructure/route_target_reader.rs -> workloads/infrastructure
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
search/presentation/controllers/search_controller.rs -> identity/presentation
secrets/presentation/controllers/secret_queries_controller.rs -> identity/presentation
secrets/presentation/controllers/secrets_controller.rs -> identity/presentation
security/presentation/controller.rs -> identity/presentation
sources/presentation/controllers/github_connections_controller.rs -> identity/presentation
sources/presentation/controllers/github_repository_subscription_queries_controller.rs -> identity/presentation
sources/presentation/controllers/github_repository_subscriptions_controller.rs -> identity/presentation
sources/presentation/controllers/source_revision_queries_controller.rs -> identity/presentation
sources/presentation/controllers/source_revisions_controller.rs -> identity/presentation
workflow/infrastructure/persistence/human_task_postgres.rs -> forms/infrastructure
workflow/infrastructure/persistence/human_task_postgres/rows.rs -> forms/infrastructure
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

    assert!(
        unexpected.is_empty(),
        "new cross-context outer-layer dependencies bypass an owner port or published contract:\n{}",
        unexpected.join("\n")
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

    assert!(
        unexpected.is_empty(),
        "a physical table has more than one mapping authority:\n{}",
        unexpected.join("\n")
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
fn runtime_contracts_enter_domains_only_through_named_published_boundaries() {
    let allowed_files = lines(
        r#"
artifacts/domain/entities/build_run.rs
executions/domain/entities/execution_task_policy.rs
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
            && compact.contains("implQueryHandler<DetectBuildPlanProposals>"),
        "BuildPlan detection must enter Application through one local service and query boundary"
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
forms -> infrastructure
forms -> presentation
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
search -> infrastructure
search -> presentation
secrets -> infrastructure
secrets -> presentation
security -> infrastructure
security -> presentation
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
            if source
                .lines()
                .any(|line| line.trim() == format!("pub mod {outer_layer};"))
            {
                actual.insert(format!("{source_context} -> {outer_layer}"));
            }
        }
    });

    let unexpected = difference(&actual, &allowed);
    assert!(
        unexpected.is_empty(),
        "a bounded context publicly exposed a new outer-layer module:\n{}",
        unexpected.join("\n")
    );
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
