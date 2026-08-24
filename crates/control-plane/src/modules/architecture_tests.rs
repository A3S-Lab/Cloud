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
durable_cells/infrastructure/provider_runtime.rs -> workloads/infrastructure
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
        // Production imports are top-level. Dropping the first cfg(test) item and
        // everything below it keeps inline fixtures out of the debt inventory.
        let production = source
            .split_once("#[cfg(test)]")
            .map_or(source.as_str(), |(before_tests, _)| before_tests);
        visit(relative, production);
    }
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
