use super::{Fixture, TestResult, CHECKPOINT_IDEMPOTENCY_KEY, FORK_IDEMPOTENCY_KEY};
use a3s_cloud_control_plane::infrastructure::connect_postgres;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    AgentExecutionCheckpointId, AgentExecutionId,
};
use a3s_orm::{sql_query, Database, PostgresDialect};

pub(super) async fn assert_pre_projection_gap(
    fixture: &Fixture,
    checkpoint_id: AgentExecutionCheckpointId,
    object_lease_id: uuid::Uuid,
) -> TestResult {
    let executor = connect_postgres(&fixture.postgres_url, 4).await?;
    let database = Database::new(PostgresDialect, executor);
    let projection_count = database
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from agent_execution_checkpoints where organization_id = ",
            )
            .bind(fixture.organization_id.as_uuid())
            .append(" and id = ")
            .bind(checkpoint_id.as_uuid()),
        )
        .await?;
    let outbox_count = database
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from outbox_events where event_key = 'agent.execution-checkpoint.committed' and organization_id = ",
            )
            .bind(fixture.organization_id.as_uuid())
            .append(" and aggregate_id = ")
            .bind(checkpoint_id.as_uuid()),
        )
        .await?;
    let idempotency_count = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from idempotency_records where scope_key = ")
                .bind(checkpoint_scope(fixture))
                .append(" and idempotency_key = ")
                .bind(CHECKPOINT_IDEMPOTENCY_KEY),
        )
        .await?;
    let object_lease_count = database
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from agent_execution_checkpoint_object_leases where object_ref like ",
            )
            .bind(format!(
                "organizations/{}/executions/{}/checkpoints/{checkpoint_id}/%",
                fixture.organization_id, fixture.execution_id
            ))
            .append(" and purpose = 'capture' and lease_id = ")
            .bind(object_lease_id),
        )
        .await?;
    assert_eq!(
        (
            projection_count,
            outbox_count,
            idempotency_count,
            object_lease_count
        ),
        (0, 0, 0, 1)
    );
    Ok(())
}

pub(super) async fn assert_committed_recovery_state(
    fixture: &Fixture,
    checkpoint_id: AgentExecutionCheckpointId,
    child_execution_id: AgentExecutionId,
) -> TestResult {
    let executor = connect_postgres(&fixture.postgres_url, 4).await?;
    let database = Database::new(PostgresDialect, executor);
    let checkpoint_count = database
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from agent_execution_checkpoints where organization_id = ",
            )
            .bind(fixture.organization_id.as_uuid())
            .append(" and id = ")
            .bind(checkpoint_id.as_uuid()),
        )
        .await?;
    let execution_count = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from agent_executions where organization_id = ")
                .bind(fixture.organization_id.as_uuid())
                .append(" and conversation_id = ")
                .bind(fixture.conversation_id.as_uuid()),
        )
        .await?;
    let child_event_count = database
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from agent_execution_events where organization_id = ",
            )
            .bind(fixture.organization_id.as_uuid())
            .append(" and execution_id = ")
            .bind(child_execution_id.as_uuid()),
        )
        .await?;
    let checkpoint_outbox_count = database
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from outbox_events where event_key = 'agent.execution-checkpoint.committed' and organization_id = ",
            )
            .bind(fixture.organization_id.as_uuid())
            .append(" and aggregate_id = ")
            .bind(checkpoint_id.as_uuid()),
        )
        .await?;
    let fork_outbox_count = database
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from outbox_events where event_key = 'agent.execution.forked' and organization_id = ",
            )
            .bind(fixture.organization_id.as_uuid())
            .append(" and aggregate_id = ")
            .bind(child_execution_id.as_uuid()),
        )
        .await?;
    let checkpoint_idempotency_count = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from idempotency_records where scope_key = ")
                .bind(checkpoint_scope(fixture))
                .append(" and idempotency_key = ")
                .bind(CHECKPOINT_IDEMPOTENCY_KEY),
        )
        .await?;
    let fork_idempotency_count = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from idempotency_records where scope_key = ")
                .bind(fork_scope(fixture, checkpoint_id))
                .append(" and idempotency_key = ")
                .bind(FORK_IDEMPOTENCY_KEY),
        )
        .await?;
    let object_lease_count = database
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from agent_execution_checkpoint_object_leases where organization_id = ",
            )
            .bind(fixture.organization_id.as_uuid())
            .append(" and checkpoint_id = ")
            .bind(checkpoint_id.as_uuid()),
        )
        .await?;
    assert_eq!(checkpoint_count, 1);
    assert_eq!(execution_count, 2);
    assert_eq!(child_event_count, 1);
    assert_eq!(checkpoint_outbox_count, 1);
    assert_eq!(fork_outbox_count, 1);
    assert_eq!(checkpoint_idempotency_count, 1);
    assert_eq!(fork_idempotency_count, 1);
    assert_eq!(object_lease_count, 0);
    Ok(())
}

fn checkpoint_scope(fixture: &Fixture) -> String {
    format!(
        "organizations/{}/agent-executions/{}/checkpoints",
        fixture.organization_id, fixture.execution_id
    )
}

fn fork_scope(fixture: &Fixture, checkpoint_id: AgentExecutionCheckpointId) -> String {
    format!(
        "organizations/{}/agent-executions/{}/checkpoints/{checkpoint_id}/fork",
        fixture.organization_id, fixture.execution_id
    )
}
