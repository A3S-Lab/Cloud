use a3s_cloud_control_plane::infrastructure::connect_and_migrate;
use a3s_cloud_control_plane::modules::search::{
    ISearchRepository, PostgresSearchRepository, SearchQuery,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::OrganizationId;
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::Utc;
use uuid::Uuid;

const POSTGRES_URL_ENV: &str = "A3S_CLOUD_TEST_POSTGRES_URL";

#[tokio::test]
async fn postgres_search_uses_registered_tenant_projections(
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(admin_url) = std::env::var(POSTGRES_URL_ENV).ok() else {
        return Ok(());
    };
    let database_name = format!("a3s_cloud_search_test_{}", Uuid::new_v4().simple());
    let mut database_url = url::Url::parse(&admin_url)?;
    database_url.set_path(&format!("/{database_name}"));
    let admin = PostgresExecutor::connect_no_tls(&admin_url, 2)?;
    admin
        .pool()
        .get()
        .await?
        .batch_execute(&format!("create database \"{database_name}\""))
        .await?;

    let test_result = exercise_search(database_url.as_str()).await;
    let cleanup_result = admin
        .pool()
        .get()
        .await?
        .batch_execute(&format!(
            "drop database if exists \"{database_name}\" with (force)"
        ))
        .await;
    match (test_result, cleanup_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error.into()),
        (Err(test_error), Err(cleanup_error)) => Err(std::io::Error::other(format!(
            "search integration failed: {test_error}; cleanup failed: {cleanup_error}"
        ))
        .into()),
    }
}

async fn exercise_search(database_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let executor = connect_and_migrate(database_url, 4).await?;
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
        )
        .await?;
    if allowed.len() != 1
        || allowed[0].id != allowed_project
        || allowed[0].title != "Cloud platform"
    {
        return Err(std::io::Error::other("allowed search projection was not isolated").into());
    }
    let denied = repository
        .search(
            denied_organization,
            &SearchQuery::parse("cloud").map_err(std::io::Error::other)?,
            20,
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
        )
        .await?;
    if !wildcard.is_empty() {
        return Err(std::io::Error::other("search treated a literal as a wildcard").into());
    }
    Ok(())
}
