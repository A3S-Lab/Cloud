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
artifacts/infrastructure/persistence/postgres.rs -> assets/infrastructure
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
    let allowed = lines(
        r#"
artifacts/domain/services/node_artifact_store.rs -> tokio::
"#,
    );
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
artifacts/domain/services/node_artifact_store.rs
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
artifacts -> presentation
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
