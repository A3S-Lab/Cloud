//! Focused A0.4 real Box published Agent release certification gate.
//!
//! Kept as a separate integration binary so the Linux real-Box path does not
//! depend on unrelated postgres_integration support modules that currently
//! fail to compile.

use a3s_boot::{BootError, BootRequest, BootResponse, HttpMethod};
use a3s_cloud_control_plane::infrastructure::{
    connect_postgres, migrate_postgres, PostgresBootstrapError, PostgresMigrationReport,
};
use a3s_orm::PostgresExecutor;
use futures_util::FutureExt;
use serde_json::Value;
use std::panic::AssertUnwindSafe;
use uuid::Uuid;

#[path = "support/a0_4_real_box_release_support.rs"]
mod a0_4_real_box_release_support;
#[path = "support/build_evidence.rs"]
mod build_evidence_support;
#[path = "support/build_runs.rs"]
mod build_runs_support;
#[path = "support/deployment_flow.rs"]
mod deployment_flow_support;

/// Minimal HTTP helpers required by `build_runs` compilation.
/// Full postgres_fixture is intentionally not pulled in.
mod postgres_fixture {
    use super::{BootError, BootRequest, BootResponse, HttpMethod, Value};

    pub(super) const ADMIN_TOKEN: &str =
        "a3s_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    pub(super) fn post_json(
        path: impl Into<String>,
        idempotency_key: &str,
        body: Value,
    ) -> BootRequest {
        post_json_as(path, idempotency_key, body, ADMIN_TOKEN)
    }

    pub(super) fn post_json_as(
        path: impl Into<String>,
        idempotency_key: &str,
        body: Value,
        token: &str,
    ) -> BootRequest {
        BootRequest::new(HttpMethod::Post, path.into())
            .with_header("content-type", "application/json")
            .with_header("idempotency-key", idempotency_key)
            .with_header("authorization", format!("Bearer {token}"))
            .with_body(body.to_string().into_bytes())
    }

    pub(super) fn get_as(path: impl Into<String>, token: &str) -> BootRequest {
        BootRequest::new(HttpMethod::Get, path.into())
            .with_header("authorization", format!("Bearer {token}"))
    }

    pub(super) fn response_json(response: &BootResponse) -> a3s_boot::Result<Value> {
        serde_json::from_slice(response.body()).map_err(|error| BootError::Internal(error.to_string()))
    }
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires the dedicated real Box provider and PostgreSQL certification environment"]
async fn postgres_published_agent_release_runs_recovers_and_cleans_through_real_box() {
    let admin_url = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL")
        .expect("real Agent release gate requires A3S_CLOUD_TEST_POSTGRES_URL");
    run_isolated_postgres(
        &admin_url,
        a0_4_real_box_release_support::exercise_published_agent_release_real_box,
    )
    .await
    .expect("published Agent release real Box lifecycle gate");
}

async fn migrate_and_connect_for_test(
    url: &str,
    max_connections: usize,
) -> Result<PostgresExecutor, PostgresBootstrapError> {
    migrate_for_test(url, max_connections).await?;
    connect_postgres(url, max_connections).await
}

async fn migrate_for_test(
    url: &str,
    max_connections: usize,
) -> Result<PostgresMigrationReport, PostgresBootstrapError> {
    let serving_role = serving_role_for_url(url)?;
    migrate_postgres(url, max_connections, &serving_role).await
}

fn serving_role_for_url(database_url: &str) -> Result<String, PostgresBootstrapError> {
    let parsed = url::Url::parse(database_url).map_err(|error| {
        PostgresBootstrapError::ComponentConfiguration(format!(
            "invalid isolated PostgreSQL URL: {error}"
        ))
    })?;
    let database_name = parsed.path().trim_start_matches('/');
    if database_name.is_empty() || database_name.contains('/') {
        return Err(PostgresBootstrapError::ComponentConfiguration(
            "isolated PostgreSQL URL must name exactly one database".into(),
        ));
    }
    Ok(format!("{database_name}_serving"))
}

struct IsolatedPostgresDatabase {
    admin_url: String,
    database_name: String,
    database_url: String,
    serving_role: String,
}

impl IsolatedPostgresDatabase {
    async fn create(admin_url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let database_name = format!("a3s_cloud_test_{}", Uuid::new_v4().simple());
        let mut database_url = url::Url::parse(admin_url)?;
        database_url.set_path(&format!("/{database_name}"));
        let serving_role = format!("{database_name}_serving");

        let admin = PostgresExecutor::connect_no_tls(admin_url, 2)?;
        let connection = admin.pool().get().await?;
        connection
            .batch_execute(&format!("create database \"{database_name}\""))
            .await?;
        if let Err(source) = connection
            .batch_execute(&format!(
                "create role \"{serving_role}\" nologin nosuperuser nocreatedb nocreaterole noreplication"
            ))
            .await
        {
            let cleanup = connection
                .batch_execute(&format!(
                    "drop database if exists \"{database_name}\" with (force)"
                ))
                .await;
            return match cleanup {
                Ok(()) => Err(source.into()),
                Err(cleanup_error) => Err(std::io::Error::other(format!(
                    "could not create isolated serving role: {source}; database cleanup also failed: {cleanup_error}"
                ))
                .into()),
            };
        }

        Ok(Self {
            admin_url: admin_url.to_owned(),
            database_name,
            database_url: database_url.to_string(),
            serving_role,
        })
    }

    fn url(&self) -> &str {
        &self.database_url
    }

    async fn cleanup(&self) -> Result<(), Box<dyn std::error::Error>> {
        let admin = PostgresExecutor::connect_no_tls(&self.admin_url, 2)?;
        let connection = admin.pool().get().await?;
        let database_cleanup = connection
            .batch_execute(&format!(
                "drop database if exists \"{}\" with (force)",
                self.database_name
            ))
            .await;
        let role_cleanup = connection
            .batch_execute(&format!("drop role if exists \"{}\"", self.serving_role))
            .await;
        match (database_cleanup, role_cleanup) {
            (Ok(()), Ok(())) => {}
            (Err(database_error), Ok(())) => return Err(database_error.into()),
            (Ok(()), Err(role_error)) => return Err(role_error.into()),
            (Err(database_error), Err(role_error)) => {
                return Err(std::io::Error::other(format!(
                    "isolated database cleanup failed: {database_error}; role cleanup also failed: {role_error}"
                ))
                .into());
            }
        }
        let row = connection
            .query_one(
                "select exists(select 1 from pg_database where datname = $1), exists(select 1 from pg_roles where rolname = $2)",
                &[&self.database_name, &self.serving_role],
            )
            .await?;
        let database_still_exists: bool = row.get(0);
        let role_still_exists: bool = row.get(1);
        if database_still_exists || role_still_exists {
            return Err(std::io::Error::other(format!(
                "isolated PostgreSQL resources still exist after cleanup (database={}, role={})",
                self.database_name, self.serving_role
            ))
            .into());
        }
        Ok(())
    }
}

async fn run_isolated_postgres<F, Fut>(
    admin_url: &str,
    exercise: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>,
{
    let isolated = IsolatedPostgresDatabase::create(admin_url).await?;
    let result = AssertUnwindSafe(Box::pin(exercise(isolated.url().to_owned())))
        .catch_unwind()
        .await;
    let cleanup = isolated.cleanup().await;

    match result {
        Ok(Ok(())) => cleanup,
        Ok(Err(test_error)) => {
            if let Err(cleanup_error) = cleanup {
                return Err(std::io::Error::other(format!(
                    "PostgreSQL integration test failed: {test_error}; isolated database cleanup also failed: {cleanup_error}"
                ))
                .into());
            }
            Err(test_error)
        }
        Err(panic_payload) => {
            if let Err(cleanup_error) = cleanup {
                eprintln!(
                    "isolated PostgreSQL database cleanup failed after test panic: {cleanup_error}"
                );
            }
            std::panic::resume_unwind(panic_payload)
        }
    }
}
