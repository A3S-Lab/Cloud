const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/204_application_delivery_credentials.sql"
));

#[test]
fn migration_204_persists_exact_anonymous_delivery_credential_bindings() {
    let lower = MIGRATION.to_ascii_lowercase();
    for expected in [
        "create table application_delivery_credentials",
        "audience text not null check (audience = 'anonymous')",
        "unique (organization_id, project_id, application_id, lookup_key)",
        "foreign key (secret_id, secret_version)",
        "references secret_versions (secret_id, version)",
        "status text not null check (status in ('active', 'disabled', 'revoked'))",
        "generation bigint not null",
        "without plaintext",
    ] {
        assert!(
            lower.contains(&expected.to_ascii_lowercase()),
            "migration 204 is missing {expected}"
        );
    }
    for forbidden in [
        "secret_material",
        "secret_value",
        "verifier_hash",
        "ciphertext",
        "api_token",
        "create table application_delivery_credential_issuances",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 204 introduced an out-of-scope authority: {forbidden}"
        );
    }
}
