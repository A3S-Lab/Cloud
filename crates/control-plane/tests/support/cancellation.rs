use a3s_boot::{BootRequest, BootResponse, HttpMethod};
use a3s_cloud_control_plane::ControlPlane;
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use serde_json::{json, Value};
use uuid::Uuid;

pub struct CancellationScenario<'a> {
    pub app: &'a ControlPlane,
    pub executor: &'a PostgresExecutor,
    pub postgres_url: &'a str,
    pub organization_id: &'a str,
    pub workload_path: &'a str,
    pub workload_body: Value,
    pub active_deployment_id: &'a str,
    pub admin_token: &'a str,
}

pub async fn exercise_deployment_cancellation(
    scenario: CancellationScenario<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let CancellationScenario {
        app,
        executor,
        postgres_url,
        organization_id,
        workload_path,
        workload_body,
        active_deployment_id,
        admin_token,
    } = scenario;
    let active_cancellation_path =
        format!("/api/v1/organizations/{organization_id}/deployments/{active_deployment_id}");
    let active_cancellation = app
        .call(delete_as(
            &active_cancellation_path,
            "cancel-active-deployment",
            admin_token,
        ))
        .await?;
    assert_eq!(active_cancellation.status(), 409);
    assert_eq!(
        response_json(&active_cancellation)?["statusCode"],
        "CONFLICT"
    );

    let mut cancelled_workload_body = workload_body.clone();
    cancelled_workload_body["name"] = json!("Cancelled API fixture");
    let queued_workload = app
        .call(post_json(
            workload_path,
            "api-cancelled-workload-fixture",
            cancelled_workload_body,
            admin_token,
        ))
        .await?;
    assert_eq!(queued_workload.status(), 202);
    let queued_workload_response = response_json(&queued_workload)?;
    let queued_deployment_id = queued_workload_response["data"]["deploymentId"]
        .as_str()
        .ok_or("queued workload response omitted deploymentId")?;
    let queued_operation_id = queued_workload_response["data"]["operationId"]
        .as_str()
        .ok_or("queued workload response omitted operationId")?;
    let cancellation_path =
        format!("/api/v1/organizations/{organization_id}/deployments/{queued_deployment_id}");

    let missing_key = app
        .call(
            BootRequest::new(HttpMethod::Delete, &cancellation_path)
                .with_header("authorization", format!("Bearer {admin_token}")),
        )
        .await?;
    assert_eq!(missing_key.status(), 400);

    let other_organization = app
        .call(post_json(
            "/api/v1/organizations",
            "organization-cancellation-tenant",
            json!({"name": "CancellationTenant"}),
            admin_token,
        ))
        .await?;
    assert_eq!(other_organization.status(), 201);
    let other_organization_id = response_id(&other_organization)?;
    let cross_tenant_cancellation = app
        .call(delete_as(
            format!(
                "/api/v1/organizations/{other_organization_id}/deployments/{queued_deployment_id}"
            ),
            "cancel-cross-tenant-deployment",
            admin_token,
        ))
        .await?;
    assert_eq!(cross_tenant_cancellation.status(), 404);

    let cancellation = app
        .call(delete_as(
            &cancellation_path,
            "cancel-queued-deployment",
            admin_token,
        ))
        .await?;
    assert_eq!(cancellation.status(), 202);
    let cancellation_body = response_json(&cancellation)?;
    assert_eq!(
        cancellation_body["data"]["deploymentId"],
        queued_deployment_id
    );
    assert_eq!(
        cancellation_body["data"]["operationId"],
        queued_operation_id
    );
    assert_eq!(cancellation_body["data"]["status"], "cancelling");
    assert_eq!(cancellation_body["data"]["replayed"], false);

    let cancellation_replay = app
        .call(delete_as(
            &cancellation_path,
            "cancel-queued-deployment",
            admin_token,
        ))
        .await?;
    assert_eq!(cancellation_replay.status(), 200);
    let cancellation_replay_body = response_json(&cancellation_replay)?;
    assert_eq!(
        cancellation_replay_body["data"]["deploymentId"],
        queued_deployment_id
    );
    assert_eq!(
        cancellation_replay_body["data"]["operationId"],
        queued_operation_id
    );
    assert_eq!(cancellation_replay_body["data"]["replayed"], true);

    let cancelling_detail = app.call(get_as(&cancellation_path, admin_token)).await?;
    assert_eq!(cancelling_detail.status(), 200);
    assert_eq!(
        response_json(&cancelling_detail)?["data"]["status"],
        "cancelling"
    );
    assert_eq!(
        Database::new(PostgresDialect, executor.clone())
            .fetch_one_as(
                sql_query::<i64>("select count(*) from outbox_events where event_key = ")
                    .bind("workload.deployment.cancellation-requested"),
            )
            .await?,
        1
    );

    crate::deployment_flow_support::exercise_pre_dispatch_cancellation(
        executor,
        postgres_url,
        Uuid::parse_str(organization_id)?,
        &queued_workload_response["data"],
    )
    .await?;
    let terminal_replay = app
        .call(delete_as(
            &cancellation_path,
            "cancel-queued-deployment",
            admin_token,
        ))
        .await?;
    assert_eq!(terminal_replay.status(), 200);
    assert_eq!(response_json(&terminal_replay)?["data"]["replayed"], true);
    let terminal_detail = app.call(get_as(&cancellation_path, admin_token)).await?;
    assert_eq!(
        response_json(&terminal_detail)?["data"]["status"],
        "cancelled"
    );

    let mut dispatched_body = workload_body;
    dispatched_body["name"] = json!("Dispatched cancellation fixture");
    let dispatched_workload = app
        .call(post_json(
            workload_path,
            "api-dispatched-cancellation-fixture",
            dispatched_body,
            admin_token,
        ))
        .await?;
    assert_eq!(dispatched_workload.status(), 202);
    crate::deployment_flow_support::exercise_dispatched_cancellation(
        executor,
        postgres_url,
        Uuid::parse_str(organization_id)?,
        &response_json(&dispatched_workload)?["data"],
    )
    .await?;
    Ok(())
}

fn post_json(path: impl Into<String>, key: &str, body: Value, token: &str) -> BootRequest {
    BootRequest::new(HttpMethod::Post, path.into())
        .with_header("content-type", "application/json")
        .with_header("idempotency-key", key)
        .with_header("authorization", format!("Bearer {token}"))
        .with_body(body.to_string().into_bytes())
}

fn delete_as(path: impl Into<String>, key: &str, token: &str) -> BootRequest {
    BootRequest::new(HttpMethod::Delete, path.into())
        .with_header("idempotency-key", key)
        .with_header("authorization", format!("Bearer {token}"))
}

fn get_as(path: impl Into<String>, token: &str) -> BootRequest {
    BootRequest::new(HttpMethod::Get, path.into())
        .with_header("accept", "application/json")
        .with_header("authorization", format!("Bearer {token}"))
}

fn response_json(response: &BootResponse) -> a3s_boot::Result<Value> {
    response.body_json()
}

fn response_id(response: &BootResponse) -> a3s_boot::Result<String> {
    response_json(response)?["data"]["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| a3s_boot::BootError::Internal("response does not contain an ID".into()))
}
