use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn automation_contracts_remain_free_of_transport_and_persistence() {
    let repository = repository_root();
    let source = read_tree(&repository.join("crates/contracts/src/automation"));
    for forbidden in [
        "reqwest::",
        "hyper::",
        "tokio::spawn",
        "sqlx::",
        "a3s_box",
        "Nats",
        "raw_credential",
        "create table",
        "insert into",
    ] {
        assert!(
            !source.contains(forbidden),
            "automation contracts acquired forbidden runtime or persistence mechanism {forbidden}"
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
