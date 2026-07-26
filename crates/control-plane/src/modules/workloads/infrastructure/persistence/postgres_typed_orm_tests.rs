use std::fs;
use std::path::Path;

#[test]
fn postgres_workload_persistence_uses_only_the_typed_a3s_orm_api() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/modules/workloads/infrastructure/persistence/postgres");
    let mut violations = Vec::new();
    find_untyped_database_access(&directory, &mut violations);

    assert!(
        violations.is_empty(),
        "untyped database access escaped into Workloads persistence: {}",
        violations.join(", ")
    );
}

fn find_untyped_database_access(directory: &Path, violations: &mut Vec<String>) {
    for entry in fs::read_dir(directory).expect("read Workloads PostgreSQL source directory") {
        let entry = entry.expect("read Workloads PostgreSQL source entry");
        let path = entry.path();
        if path.is_dir() {
            find_untyped_database_access(&path, violations);
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }

        let source = fs::read_to_string(&path).expect("read Workloads PostgreSQL source");
        for forbidden in ["sql_query", "SqlQuery", "sqlx::", "tokio_postgres"] {
            if source.contains(forbidden) {
                violations.push(format!("{} contains {forbidden}", path.display()));
            }
        }
    }
}
