use std::fs;
use std::path::Path;

#[test]
fn postgres_edge_persistence_uses_only_the_typed_a3s_orm_api() {
    let directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/modules/edge/infrastructure/persistence");
    let mut violations = Vec::new();
    find_untyped_database_access(&directory, false, &mut violations);

    assert!(
        violations.is_empty(),
        "untyped database access escaped into Edge persistence: {}",
        violations.join(", ")
    );
}

fn find_untyped_database_access(
    directory: &Path,
    inside_postgres_module: bool,
    violations: &mut Vec<String>,
) {
    for entry in fs::read_dir(directory).expect("read Edge persistence source directory") {
        let entry = entry.expect("read Edge persistence source entry");
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let is_postgres_module = inside_postgres_module || name.starts_with("postgres");
        if path.is_dir() {
            if is_postgres_module {
                find_untyped_database_access(&path, true, violations);
            }
            continue;
        }
        if !is_postgres_module
            || path.extension().and_then(|extension| extension.to_str()) != Some("rs")
            || name == "postgres_typed_orm_tests.rs"
        {
            continue;
        }

        let source = fs::read_to_string(&path).expect("read Edge PostgreSQL source");
        for forbidden in ["sql_query", "SqlQuery", "sqlx::", "tokio_postgres"] {
            if source.contains(forbidden) {
                violations.push(format!("{} contains {forbidden}", path.display()));
            }
        }
    }
}
