use super::flow::{BOOT_SCHEMA, FLOW_SCHEMA};
use a3s_orm::{
    CompiledQuery, Executor, PostgresError, PostgresExecutor, PostgresTransactionOptions,
    Transaction,
};
use std::time::Duration;

const MIGRATION_LEDGER: &str = "a3s_orm_migrations";
const SERVING_SCHEMAS: [&str; 3] = ["public", FLOW_SCHEMA, BOOT_SCHEMA];
const SERVING_ACCESS_LOCK_NAMESPACE: &str = "a3s.cloud.postgres";
const SERVING_ACCESS_LOCK_KEY: &str = "serving-access";
const SERVING_ACCESS_LOCK_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) enum PostgresServingAccessError {
    MissingRole,
    MigrationRoleCollision,
    MigrationRoleMembership,
    PrivilegedRole,
    Database(PostgresError),
}

pub(super) struct PostgresServingAccessPlan {
    database: String,
    role: String,
}

pub(super) async fn prepare_postgres_serving_access(
    executor: &PostgresExecutor,
    serving_role: &str,
) -> Result<PostgresServingAccessPlan, PostgresServingAccessError> {
    let client = executor
        .connection()
        .await
        .map_err(PostgresServingAccessError::Database)?;
    let identifiers = client
        .query_one(
            "select
               quote_ident(current_database()),
               quote_ident($1::text),
               current_user::text,
               exists(select 1 from pg_catalog.pg_roles where rolname = $1),
               coalesce((select not (
                 rolsuper or rolcreatedb or rolcreaterole or rolreplication or rolbypassrls
               ) from pg_catalog.pg_roles where rolname = $1), false),
               case
                 when exists(select 1 from pg_catalog.pg_roles where rolname = $1)
                 then pg_has_role($1::name, current_user::text, 'MEMBER')
                 else false
               end",
            &[&serving_role],
        )
        .await
        .map_err(PostgresError::Database)
        .map_err(PostgresServingAccessError::Database)?;
    let database: String = identifiers.get(0);
    let role: String = identifiers.get(1);
    let migration_role: String = identifiers.get(2);
    let role_exists: bool = identifiers.get(3);
    let role_is_unprivileged: bool = identifiers.get(4);
    let inherits_migration_role: bool = identifiers.get(5);
    if !role_exists {
        return Err(PostgresServingAccessError::MissingRole);
    }
    if serving_role == migration_role {
        return Err(PostgresServingAccessError::MigrationRoleCollision);
    }
    if inherits_migration_role {
        return Err(PostgresServingAccessError::MigrationRoleMembership);
    }
    if !role_is_unprivileged {
        return Err(PostgresServingAccessError::PrivilegedRole);
    }
    Ok(PostgresServingAccessPlan { database, role })
}

pub(super) async fn reconcile_postgres_serving_access(
    executor: &PostgresExecutor,
    plan: &PostgresServingAccessPlan,
) -> Result<(), PostgresError> {
    let database = &plan.database;
    let role = &plan.role;
    let schemas = SERVING_SCHEMAS
        .iter()
        .map(|schema| format!("\"{schema}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let ledgers = SERVING_SCHEMAS
        .iter()
        .map(|schema| format!("\"{schema}\".\"{MIGRATION_LEDGER}\""))
        .collect::<Vec<_>>()
        .join(", ");

    // Reconcile after every owner migration. This covers existing databases and
    // role recreation without a second migration manifest or bootstrap-only
    // default grants. Revoke legacy global and schema-scoped defaults before
    // granting only current objects. Migration ledgers stay readable for
    // admission but cannot be changed by a serving credential.
    let statements = [
        format!("ALTER DEFAULT PRIVILEGES REVOKE ALL PRIVILEGES ON TABLES FROM {role}"),
        format!("ALTER DEFAULT PRIVILEGES REVOKE ALL PRIVILEGES ON SEQUENCES FROM {role}"),
        format!("ALTER DEFAULT PRIVILEGES REVOKE ALL PRIVILEGES ON FUNCTIONS FROM {role}"),
        format!("ALTER DEFAULT PRIVILEGES REVOKE ALL PRIVILEGES ON SCHEMAS FROM {role}"),
        format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA {schemas} REVOKE ALL PRIVILEGES ON TABLES FROM {role}"
        ),
        format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA {schemas} REVOKE ALL PRIVILEGES ON SEQUENCES FROM {role}"
        ),
        format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA {schemas} REVOKE ALL PRIVILEGES ON FUNCTIONS FROM {role}"
        ),
        format!("REVOKE CONNECT, TEMPORARY ON DATABASE {database} FROM PUBLIC"),
        format!("REVOKE ALL PRIVILEGES ON DATABASE {database} FROM {role}"),
        format!("GRANT CONNECT ON DATABASE {database} TO {role}"),
        format!("REVOKE CREATE ON SCHEMA {schemas} FROM PUBLIC"),
        format!("REVOKE ALL PRIVILEGES ON SCHEMA {schemas} FROM {role}"),
        format!("GRANT USAGE ON SCHEMA {schemas} TO {role}"),
        format!("REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA {schemas} FROM {role}"),
        format!(
            "GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA {schemas} TO {role}"
        ),
        format!(
            "REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA {schemas} FROM {role}"
        ),
        format!(
            "GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA {schemas} TO {role}"
        ),
        format!("REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA {schemas} FROM {role}"),
        format!("GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA {schemas} TO {role}"),
        format!(
            "REVOKE INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER ON TABLE {ledgers} FROM {role}"
        ),
        format!("GRANT SELECT ON TABLE {ledgers} TO {role}"),
    ];
    let transaction = executor
        .begin_with(
            PostgresTransactionOptions::new().with_lock_timeout(SERVING_ACCESS_LOCK_TIMEOUT),
        )
        .await?;
    transaction
        .advisory_xact_lock(SERVING_ACCESS_LOCK_NAMESPACE, SERVING_ACCESS_LOCK_KEY)
        .await?;
    for sql in statements {
        transaction
            .execute(&CompiledQuery {
                sql,
                parameters: Vec::new(),
            })
            .await?;
    }
    transaction.commit().await
}
