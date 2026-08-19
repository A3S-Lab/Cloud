use a3s_cloud_control_plane::infrastructure::{
    connect_postgres, migrate_postgres, PostgresBootstrapError,
};
use a3s_cloud_control_plane::modules::identity::domain::services::ResourceAccessEvaluator;
use a3s_cloud_control_plane::modules::identity::domain::value_objects::ResourceGrantScope;
use a3s_cloud_control_plane::modules::search::{
    ISearchRepository, PostgresSearchRepository, SearchQuery,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::Utc;
use uuid::Uuid;

const POSTGRES_URL_ENV: &str = "A3S_CLOUD_TEST_POSTGRES_URL";

async fn migrate_and_connect_for_test(
    url: &str,
    max_connections: usize,
    serving_role: &str,
) -> Result<PostgresExecutor, PostgresBootstrapError> {
    migrate_postgres(url, max_connections, serving_role).await?;
    connect_postgres(url, max_connections).await
}

#[tokio::test]
async fn postgres_search_uses_registered_tenant_projections(
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(admin_url) = std::env::var(POSTGRES_URL_ENV).ok() else {
        return Ok(());
    };
    let database_name = format!("a3s_cloud_search_test_{}", Uuid::new_v4().simple());
    let serving_role = format!("{database_name}_serving");
    let mut database_url = url::Url::parse(&admin_url)?;
    database_url.set_path(&format!("/{database_name}"));
    let admin = PostgresExecutor::connect_no_tls(&admin_url, 2)?;
    let admin_connection = admin.pool().get().await?;
    admin_connection
        .batch_execute(&format!("create database \"{database_name}\""))
        .await?;
    if let Err(source) = admin_connection
        .batch_execute(&format!(
            "create role \"{serving_role}\" nologin nosuperuser nocreatedb nocreaterole noreplication"
        ))
        .await
    {
        let _ = admin_connection
            .batch_execute(&format!(
                "drop database if exists \"{database_name}\" with (force)"
            ))
            .await;
        return Err(source.into());
    }

    let test_result = exercise_search(database_url.as_str(), &serving_role).await;
    let database_cleanup = admin_connection
        .batch_execute(&format!(
            "drop database if exists \"{database_name}\" with (force)"
        ))
        .await;
    let role_cleanup = admin_connection
        .batch_execute(&format!("drop role if exists \"{serving_role}\""))
        .await;
    let cleanup_errors = [
        database_cleanup.err().map(|error| error.to_string()),
        role_cleanup.err().map(|error| error.to_string()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    match (test_result, cleanup_errors.is_empty()) {
        (Ok(()), true) => Ok(()),
        (Err(error), true) => Err(error),
        (Ok(()), false) => Err(std::io::Error::other(format!(
            "search cleanup failed: {}",
            cleanup_errors.join("; ")
        ))
        .into()),
        (Err(test_error), false) => Err(std::io::Error::other(format!(
            "search integration failed: {test_error}; cleanup failed: {}",
            cleanup_errors.join("; ")
        ))
        .into()),
    }
}

async fn exercise_search(
    database_url: &str,
    serving_role: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(database_url, 4, serving_role).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let allowed_organization = OrganizationId::new();
    let denied_organization = OrganizationId::new();
    let allowed_project = Uuid::new_v4();
    let denied_project = Uuid::new_v4();
    let created_at = Utc::now();

    for (organization_id, name, key) in [
        (allowed_organization, "Allowed", "allowed"),
        (denied_organization, "Denied", "denied"),
    ] {
        database
            .execute(
                sql_query::<()>(
                    "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
                )
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(name)
                .append(", ")
                .bind(key)
                .append(", ")
                .bind(1_u64)
                .append(", ")
                .bind(created_at)
                .append(")"),
            )
            .await?;
    }
    for (organization_id, project_id, name, key) in [
        (
            allowed_organization,
            allowed_project,
            "Cloud platform",
            "cloud platform",
        ),
        (
            denied_organization,
            denied_project,
            "Cloud hidden",
            "cloud hidden",
        ),
    ] {
        database
            .execute(
                sql_query::<()>(
                    "insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (",
                )
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(project_id)
                .append(", ")
                .bind(name)
                .append(", ")
                .bind(key)
                .append(", ")
                .bind(1_u64)
                .append(", ")
                .bind(created_at)
                .append(")"),
            )
            .await?;
    }

    let repository = PostgresSearchRepository::new(executor);
    let allowed = repository
        .search(
            allowed_organization,
            &SearchQuery::parse("cloud").map_err(std::io::Error::other)?,
            20,
            &ResourceAccessEvaluator::organization_wide(),
        )
        .await?;
    if allowed.len() != 1
        || allowed[0].id != allowed_project
        || allowed[0].title != "Cloud platform"
    {
        return Err(std::io::Error::other("allowed search projection was not isolated").into());
    }
    let restricted = repository
        .search(
            allowed_organization,
            &SearchQuery::parse("cloud").map_err(std::io::Error::other)?,
            20,
            &ResourceAccessEvaluator::restricted([ResourceGrantScope::Project {
                project_id: ProjectId::from_uuid(allowed_project),
            }]),
        )
        .await?;
    if restricted.len() != 1 || restricted[0].id != allowed_project {
        return Err(std::io::Error::other("resource grant did not expose its projection").into());
    }
    let ungranted = repository
        .search(
            allowed_organization,
            &SearchQuery::parse("cloud").map_err(std::io::Error::other)?,
            20,
            &ResourceAccessEvaluator::restricted([ResourceGrantScope::Project {
                project_id: ProjectId::new(),
            }]),
        )
        .await?;
    if !ungranted.is_empty() {
        return Err(std::io::Error::other("ungranted projection was returned").into());
    }
    let denied = repository
        .search(
            denied_organization,
            &SearchQuery::parse("cloud").map_err(std::io::Error::other)?,
            20,
            &ResourceAccessEvaluator::organization_wide(),
        )
        .await?;
    if denied.len() != 1 || denied[0].id != denied_project {
        return Err(std::io::Error::other("denied tenant projection was not isolated").into());
    }
    let wildcard = repository
        .search(
            allowed_organization,
            &SearchQuery::parse("%").map_err(std::io::Error::other)?,
            20,
            &ResourceAccessEvaluator::organization_wide(),
        )
        .await?;
    if !wildcard.is_empty() {
        return Err(std::io::Error::other("search treated a literal as a wildcard").into());
    }
    Ok(())
}
