use super::{Authorities, TestResult};
use a3s_orm::{sql_query, Database, PostgresDialect};
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) async fn seed_identity_authority(
    executor: &a3s_orm::PostgresExecutor,
    authorities: Authorities,
    created_at: DateTime<Utc>,
) -> TestResult<()> {
    let database = Database::new(PostgresDialect, executor.clone());
    for (organization_id, name, name_key) in [
        (
            authorities.organization_id,
            "HumanTask Flow organization",
            "human-task-flow-organization",
        ),
        (
            authorities.other_organization_id,
            "Other HumanTask organization",
            "other-human-task-organization",
        ),
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
                .bind(name_key)
                .append(", 1, ")
                .bind(created_at)
                .append(")"),
            )
            .await?;
    }
    for (organization_id, project_id, name, name_key) in [
        (
            authorities.organization_id,
            authorities.project_id,
            "HumanTask Flow project",
            "human-task-flow-project",
        ),
        (
            authorities.other_organization_id,
            authorities.other_project_id,
            "Other HumanTask project",
            "other-human-task-project",
        ),
    ] {
        database
            .execute(
                sql_query::<()>(
                    "insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (",
                )
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(project_id.as_uuid())
                .append(", ")
                .bind(name)
                .append(", ")
                .bind(name_key)
                .append(", 1, ")
                .bind(created_at)
                .append(")"),
            )
            .await?;
    }
    for (principal_id, name) in [
        (authorities.actor, "Workflow coordinator"),
        (authorities.reviewer, "Workflow reviewer"),
    ] {
        database
            .execute(
                sql_query::<()>(
                    "insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (",
                )
                .bind(principal_id.as_uuid())
                .append(", 'human', ")
                .bind(name)
                .append(", 1, ")
                .bind(created_at)
                .append(", null)"),
            )
            .await?;
        database
            .execute(
                sql_query::<()>(
                    "insert into organization_memberships (id, organization_id, principal_id, role, aggregate_version, created_at, updated_at, revoked_at) values (",
                )
                .bind(Uuid::now_v7())
                .append(", ")
                .bind(authorities.organization_id.as_uuid())
                .append(", ")
                .bind(principal_id.as_uuid())
                .append(", 'member', 1, ")
                .bind(created_at)
                .append(", ")
                .bind(created_at)
                .append(", null)"),
            )
            .await?;
    }
    Ok(())
}
