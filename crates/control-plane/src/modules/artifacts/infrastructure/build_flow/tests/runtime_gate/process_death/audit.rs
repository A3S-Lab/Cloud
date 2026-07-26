use super::*;
use a3s_orm::{sql_query, Database, PostgresDialect};

pub(super) struct DurableCounts {
    pub(super) logical_publications: u32,
    pub(super) evidence_documents: u32,
    pub(super) apply_commands: u32,
    pub(super) cleanup_commands: u32,
    pub(super) cleanup_acknowledgements: u32,
}

pub(super) async fn durable_counts(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    build_id: BuildRunId,
    node_id: NodeId,
) -> Result<DurableCounts, Box<dyn Error>> {
    let database = Database::new(PostgresDialect, executor.clone());
    let logical_publications = count_rows(
        &database,
        sql_query::<i64>("select count(*) from build_runs where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and id = ")
            .bind(build_id.as_uuid())
            .append(" and publication_target is not null and published_artifact is not null"),
    )
    .await?;
    let evidence_documents = count_rows(
        &database,
        sql_query::<i64>("select count(*) from build_runs where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and id = ")
            .bind(build_id.as_uuid())
            .append(" and evidence is not null"),
    )
    .await?;
    Ok(DurableCounts {
        logical_publications,
        evidence_documents,
        apply_commands: command_count(&database, build_id, node_id, "runtime_apply", false).await?,
        cleanup_commands: command_count(&database, build_id, node_id, "runtime_remove", false)
            .await?,
        cleanup_acknowledgements: command_count(
            &database,
            build_id,
            node_id,
            "runtime_remove",
            true,
        )
        .await?,
    })
}

async fn command_count(
    database: &Database<PostgresDialect, PostgresExecutor>,
    build_id: BuildRunId,
    node_id: NodeId,
    kind: &str,
    acknowledged_only: bool,
) -> Result<u32, Box<dyn Error>> {
    let mut query = sql_query::<i64>("select count(*) from node_commands where aggregate_id = ")
        .bind(build_id.as_uuid())
        .append(" and node_id = ")
        .bind(node_id.as_uuid())
        .append(" and command_kind = ")
        .bind(kind);
    if acknowledged_only {
        query = query.append(" and acknowledgement is not null");
    }
    count_rows(database, query).await
}

async fn count_rows(
    database: &Database<PostgresDialect, PostgresExecutor>,
    query: impl a3s_orm::Query<Output = i64>,
) -> Result<u32, Box<dyn Error>> {
    let count = database.fetch_one_as(query).await?;
    Ok(u32::try_from(count)?)
}
