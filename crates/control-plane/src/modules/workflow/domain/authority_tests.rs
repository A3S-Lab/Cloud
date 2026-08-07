use std::path::{Path, PathBuf};

#[test]
fn workflow_domain_cannot_import_execution_or_persistence_authorities() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/modules/workflow/domain");
    let mut violations = Vec::new();
    visit_rust_sources(&root, &mut |path, source| {
        if path.ends_with("authority_tests.rs") {
            return;
        }
        for forbidden in [
            "a3s_flow::",
            "a3s_runtime::",
            "a3s_orm::",
            "sqlx::",
            "reqwest::",
            "tokio::",
            "crate::modules::flow",
            "crate::modules::runtime",
            "crate::modules::operations",
            "std::fs",
            "std::net",
            "std::process",
            "FlowTaskQueue",
            "RuntimeClient",
            "EventBus",
            "object_store::",
        ] {
            if source.contains(forbidden) {
                violations.push(format!("{} contains {forbidden}", path.display()));
            }
        }
    });
    assert!(
        violations.is_empty(),
        "Workflow domain crossed an authority boundary: {}",
        violations.join(", ")
    );
}

#[test]
fn workflow_module_does_not_restore_standalone_mechanism_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/modules/workflow");
    let forbidden = [
        "flow_runtime.rs",
        "runtime_provider.rs",
        "node_runner.rs",
        "node_execution_store.rs",
        "postgres_memory.rs",
        "task_queue.rs",
    ];
    let mut found = Vec::new();
    visit_paths(&root, &mut |path| {
        if path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| forbidden.contains(&name))
        {
            found.push(path.to_path_buf());
        }
    });
    assert!(
        found.is_empty(),
        "Workflow restored a standalone mechanism instead of reusing Cloud authorities: {found:?}"
    );
}

fn visit_rust_sources(root: &Path, visit: &mut impl FnMut(&Path, &str)) {
    visit_paths(root, &mut |path| {
        if path.extension().and_then(|value| value.to_str()) != Some("rs") {
            return;
        }
        let source = std::fs::read_to_string(path).expect("read Workflow source");
        visit(path, &source);
    });
}

fn visit_paths(root: &Path, visit: &mut impl FnMut(&Path)) {
    let mut pending = vec![PathBuf::from(root)];
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            let entries = std::fs::read_dir(&path).expect("read Workflow source directory");
            pending.extend(entries.map(|entry| entry.expect("read Workflow source entry").path()));
        } else {
            visit(&path);
        }
    }
}
