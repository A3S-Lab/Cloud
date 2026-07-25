use std::fs;
use std::path::Path;

#[test]
fn node_resource_inventory_persistence_uses_only_the_typed_a3s_orm_api() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/modules/fleet/infrastructure/persistence/postgres/control/inventory.rs");
    let source = fs::read_to_string(&file).expect("read resource inventory persistence source");
    let violations = ["sql_query", "SqlQuery", "sqlx::", "tokio_postgres"]
        .into_iter()
        .filter(|forbidden| source.contains(forbidden))
        .collect::<Vec<_>>();
    assert!(
        violations.is_empty(),
        "untyped database access escaped into resource inventory persistence: {}",
        violations.join(", ")
    );
}
