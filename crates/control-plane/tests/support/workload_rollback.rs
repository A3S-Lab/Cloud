use a3s_boot::{BootRequest, HttpMethod};
use a3s_cloud_control_plane::modules::shared_kernel::domain::DeploymentId;
use a3s_cloud_control_plane::modules::workloads::{
    IWorkloadRepository, PostgresWorkloadRepository,
};
use a3s_cloud_control_plane::ControlPlane;
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

const IDEMPOTENCY_KEY: &str = "api-workload-rollback";

pub struct RollbackApiScenario<'a> {
    pub app: &'a ControlPlane,
    pub executor: &'a PostgresExecutor,
    pub organization_id: &'a str,
    pub workload_id: &'a str,
    pub source_revision_id: &'a str,
    pub current_revision_id: &'a str,
    pub artifact_digest: &'a str,
    pub token: &'a str,
}

pub struct RollbackReplayFixture {
    path: String,
    body: Value,
    deployment_id: String,
    current_revision_id: String,
}

pub async fn accept_and_cancel(
    scenario: RollbackApiScenario<'_>,
) -> Result<RollbackReplayFixture, Box<dyn std::error::Error>> {
    let RollbackApiScenario {
        app,
        executor,
        organization_id,
        workload_id,
        source_revision_id,
        current_revision_id,
        artifact_digest,
        token,
    } = scenario;
    let path = format!("/api/v1/organizations/{organization_id}/workloads/{workload_id}/rollback");
    let body = json!({"revisionId": source_revision_id});
    let rollback = app
        .call(post_json(&path, IDEMPOTENCY_KEY, body.clone(), token))
        .await?;
    assert_eq!(rollback.status(), 202);
    let rollback_json: Value = rollback.body_json()?;
    assert_eq!(rollback_json["data"]["generation"], 3);
    assert_eq!(
        rollback_json["data"]["rollbackSourceRevisionId"],
        source_revision_id
    );
    assert_eq!(rollback_json["data"]["artifactDigest"], artifact_digest);
    let rollback_revision_id = field(&rollback_json, "revisionId")?.to_owned();
    let rollback_deployment_id = field(&rollback_json, "deploymentId")?.to_owned();
    let rollback_operation_id = field(&rollback_json, "operationId")?.to_owned();

    let database = Database::new(PostgresDialect, executor.clone());
    let source_revision = database
        .fetch_one_as(
            sql_query::<(Value, String, String)>(
                "select template, template_digest, artifact_digest from workload_revisions where id = ",
            )
            .bind(Uuid::parse_str(source_revision_id)?),
        )
        .await?;
    let stored_rollback = database
        .fetch_one_as(
            sql_query::<(i64, Value, String, String, String)>(
                "select generation, template, template_digest, expected_artifact_digest, artifact_digest from workload_revisions where id = ",
            )
            .bind(Uuid::parse_str(&rollback_revision_id)?),
        )
        .await?;
    assert_eq!(stored_rollback.0, 3);
    assert_eq!(stored_rollback.1, source_revision.0);
    assert_eq!(stored_rollback.2, source_revision.1);
    assert_eq!(stored_rollback.3, source_revision.2);
    assert_eq!(stored_rollback.4, source_revision.2);

    let rollback_operation = database
        .fetch_one_as(
            sql_query::<(String, String, Value)>(
                "select workflow_name, workflow_version, input from operation_requests where operation_id = ",
            )
            .bind(Uuid::parse_str(&rollback_operation_id)?),
        )
        .await?;
    assert_eq!(rollback_operation.0, "cloud.deployment");
    assert_eq!(rollback_operation.1, "2");
    assert_eq!(
        rollback_operation.2["rollbackSourceRevisionId"],
        source_revision_id
    );
    let listed_operations = app
        .call(
            BootRequest::new(
                HttpMethod::Get,
                format!("/api/v1/organizations/{organization_id}/operations?limit=100"),
            )
            .with_header("accept", "application/json")
            .with_header("authorization", format!("Bearer {token}")),
        )
        .await?;
    assert_eq!(listed_operations.status(), 200);
    let listed_operations: Value = listed_operations.body_json()?;
    let listed_rollback = listed_operations["data"]
        .as_array()
        .and_then(|operations| {
            operations
                .iter()
                .find(|operation| operation["id"] == rollback_operation_id)
        })
        .ok_or("operations list omitted the rollback operation")?;
    assert_eq!(
        listed_rollback["rollbackSourceRevisionId"],
        source_revision_id
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from idempotency_records where scope_key = ",)
                    .bind(format!(
                        "organizations/{organization_id}/workloads/{workload_id}/rollback"
                    ))
                    .append(" and idempotency_key = ")
                    .bind(IDEMPOTENCY_KEY),
            )
            .await?,
        1
    );

    let workloads = PostgresWorkloadRepository::new(executor.clone());
    let deployment_id = DeploymentId::from_uuid(Uuid::parse_str(&rollback_deployment_id)?);
    assert_eq!(
        workloads
            .cancel(deployment_id, 1, Utc::now())
            .await?
            .status
            .as_str(),
        "cancelled"
    );
    Ok(RollbackReplayFixture {
        path,
        body,
        deployment_id: rollback_deployment_id,
        current_revision_id: current_revision_id.to_owned(),
    })
}

pub async fn assert_replay_after_workload_stop(
    app: &ControlPlane,
    fixture: RollbackReplayFixture,
    token: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let replay = app
        .call(post_json(
            &fixture.path,
            IDEMPOTENCY_KEY,
            fixture.body,
            token,
        ))
        .await?;
    assert_eq!(replay.status(), 200);
    let replay: Value = replay.body_json()?;
    assert_eq!(replay["data"]["deploymentId"], fixture.deployment_id);
    assert_eq!(replay["data"]["replayed"], true);

    let changed_replay = app
        .call(post_json(
            &fixture.path,
            IDEMPOTENCY_KEY,
            json!({"revisionId": fixture.current_revision_id}),
            token,
        ))
        .await?;
    assert_eq!(changed_replay.status(), 409);
    Ok(())
}

fn field<'a>(body: &'a Value, name: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    body["data"][name]
        .as_str()
        .ok_or_else(|| format!("rollback response omitted {name}").into())
}

fn post_json(path: &str, key: &str, body: Value, token: &str) -> BootRequest {
    BootRequest::new(HttpMethod::Post, path)
        .with_header("content-type", "application/json")
        .with_header("idempotency-key", key)
        .with_header("authorization", format!("Bearer {token}"))
        .with_body(body.to_string().into_bytes())
}
