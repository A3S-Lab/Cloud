use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn aut0_1_is_a_contract_boundary_without_a_second_runtime() {
    let repository = repository_root();
    let modules = repository.join("crates/control-plane/src/modules");
    assert!(
        !modules.join("automations").exists(),
        "AUT0.1 must not add a scheduler bounded context before its persistence gate"
    );

    let source = read_tree(&repository.join("crates/contracts/src/automation"));
    for forbidden in [
        "reqwest::",
        "hyper::",
        "tokio::spawn",
        "sqlx::",
        "a3s_box",
        "Nats",
        "scheduler",
        "raw_credential",
    ] {
        assert!(
            !source.contains(forbidden),
            "AUT0.1 contract acquired forbidden runtime or mutable selector {forbidden}"
        );
    }

    let migrations = read_tree(&repository.join("migrations")).to_ascii_lowercase();
    for forbidden_table in [
        "automation_definitions",
        "automation_revisions",
        "automation_invocation_receipts",
        "automation_schedules",
    ] {
        assert!(
            !migrations.contains(forbidden_table),
            "AUT0.1 component slice introduced a durable table before its persistence gate"
        );
    }
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
