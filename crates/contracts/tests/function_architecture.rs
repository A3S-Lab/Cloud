use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn function_contract_adds_no_parallel_lifecycle_authority() {
    let repository = repository_root();
    let modules = repository.join("crates/control-plane/src/modules");
    for forbidden_context in ["functions", "function_runtime", "function_invocations"] {
        assert!(
            !modules.join(forbidden_context).exists(),
            "FN0.1 must not add a {forbidden_context} bounded context"
        );
    }

    let function_source = read_tree(&repository.join("crates/contracts/src/function"));
    for forbidden in [
        "a3s_box",
        "reqwest::",
        "sqlx::",
        "tokio::spawn",
        "RuntimeUnitClass::Function",
        "FunctionRepository",
        "FunctionScheduler",
        "FunctionQueue",
    ] {
        assert!(
            !function_source.contains(forbidden),
            "Function value contract acquired forbidden mechanism {forbidden}"
        );
    }
    for required in [
        "FunctionOwnerV1::Executions",
        "FunctionOwnerV1::Workloads",
        "FunctionOwnerV1::Connectors",
        "RuntimeIsolationLevel",
        "a3s_acl",
    ] {
        assert!(
            function_source.contains(required),
            "Function value contract lost owner boundary {required}"
        );
    }
}

#[test]
fn function_contract_adds_no_tables_or_mutable_provider_fields() {
    let repository = repository_root();
    let migrations = read_tree(&repository.join("migrations")).to_ascii_lowercase();
    for forbidden_table in [
        "function_invocations",
        "function_runs",
        "function_retries",
        "function_schedulers",
        "function_runtime_units",
    ] {
        assert!(
            !migrations.contains(forbidden_table),
            "FN0.1 introduced duplicate durable authority {forbidden_table}"
        );
    }

    let fixtures = read_tree(&repository.join("contracts/fn0.1"));
    for forbidden_field in [
        "retry_count",
        "runtime_unit_id",
        "node_id",
        "raw_credential",
        "provider_state",
        "desired_replicas",
    ] {
        assert!(
            !fixtures.contains(forbidden_field),
            "Function profile fixture leaked foreign mutable field {forbidden_field}"
        );
    }
}

#[test]
fn fn0_product_configuration_is_acl_only() {
    let directory = repository_root().join("contracts/fn0.1");
    let mut product_files = fs::read_dir(&directory)
        .expect("read FN0.1 contract directory")
        .map(|entry| entry.expect("contract entry").path())
        .filter(|path| path.file_name().and_then(|name| name.to_str()) != Some("README.md"))
        .collect::<Vec<_>>();
    product_files.sort();
    assert!(!product_files.is_empty());
    assert!(product_files
        .iter()
        .all(|path| path.extension().and_then(|value| value.to_str()) == Some("acl")));
}

fn read_tree(root: &Path) -> String {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            pending.extend(
                fs::read_dir(&path)
                    .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
                    .map(|entry| entry.expect("tree entry").path()),
            );
        } else if path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    files
        .into_iter()
        .map(|path| {
            fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
