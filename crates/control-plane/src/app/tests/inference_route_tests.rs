use super::*;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use uuid::Uuid;

const INFERENCE_ROUTE_READ_TOKEN: &str =
    "a3s_3333333333333333333333333333333333333333333333333333333333333333";
const INFERENCE_ROUTE_WRITE_TOKEN: &str =
    "a3s_4444444444444444444444444444444444444444444444444444444444444444";

#[tokio::test]
async fn inference_route_publish_and_retire_require_write_scope_and_idempotency() -> Result<()> {
    let app = build_test_application(
        Arc::new(InMemoryIdentityRepository::new()),
        Arc::new(InMemoryProjectsRepository::new()),
    )?;
    let organization =
        bootstrap_organization(&app, "inference-route-http", "Inference route tenant").await?;
    let project = create_project(
        &app,
        &organization,
        "inference-route-project",
        "Inference Route",
    )
    .await?;
    let environment = create_environment(
        &app,
        &organization,
        &project,
        "inference-route-environment",
        "Production",
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "inference-route-read-token",
        "inference-route-read",
        INFERENCE_ROUTE_READ_TOKEN,
        &[ApiTokenScope::INFERENCE_READ],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "inference-route-write-token",
        "inference-route-write",
        INFERENCE_ROUTE_WRITE_TOKEN,
        &[ApiTokenScope::INFERENCE_WRITE],
        None,
    )
    .await?;

    let routes_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference/routes"
    );
    let body = publish_body();

    assert_eq!(
        app.call(
            BootRequest::new(HttpMethod::Post, &routes_path)
                .with_header("content-type", "application/json")
                .with_header("idempotency-key", "inference-route:unauth")
                .with_body(body.to_string().into_bytes()),
        )
        .await?
        .status(),
        401
    );

    let missing_idempotency = app
        .call(
            BootRequest::new(HttpMethod::Post, &routes_path)
                .with_header(
                    "authorization",
                    format!("Bearer {INFERENCE_ROUTE_WRITE_TOKEN}"),
                )
                .with_header("content-type", "application/json")
                .with_body(body.to_string().into_bytes()),
        )
        .await?;
    assert_eq!(missing_idempotency.status(), 400);

    let insufficient = app
        .call(post_json_as(
            &routes_path,
            "inference-route:read-denied",
            body.clone(),
            INFERENCE_ROUTE_READ_TOKEN,
        ))
        .await?;
    assert_eq!(insufficient.status(), 403);

    let published = app
        .call(post_json_as(
            &routes_path,
            "inference-route:publish",
            body,
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(published.status(), 202);
    assert_no_store(&published);
    let route_id = response_id(&published)?;
    assert_eq!(
        response_json(&published)?["data"]["router"],
        json!("inference")
    );

    let retire_path = format!("{routes_path}/{route_id}/retire");
    let missing_retire_idempotency = app
        .call(
            BootRequest::new(HttpMethod::Post, &retire_path)
                .with_header(
                    "authorization",
                    format!("Bearer {INFERENCE_ROUTE_WRITE_TOKEN}"),
                )
                .with_header("content-type", "application/json"),
        )
        .await?;
    assert_eq!(missing_retire_idempotency.status(), 400);

    let retired = app
        .call(
            BootRequest::new(HttpMethod::Post, &retire_path)
                .with_header(
                    "authorization",
                    format!("Bearer {INFERENCE_ROUTE_WRITE_TOKEN}"),
                )
                .with_header("content-type", "application/json")
                .with_header("idempotency-key", "inference-route:retire"),
        )
        .await?;
    assert_eq!(retired.status(), 202);
    assert_no_store(&retired);
    assert!(response_json(&retired)?["data"]["retiredAt"].is_string());
    Ok(())
}

fn publish_body() -> Value {
    json!({
        "router": "inference",
        "models": [{
            "alias": "chat-model",
            "modelId": "55555555-5555-4555-8555-555555555555",
            "targets": [{
                "targetId": "66666666-6666-4666-8666-666666666666",
                "service": "model-service",
                "upstreamModel": "internal/model-v1",
                "priority": 0,
                "weight": 100
            }]
        }],
        "grants": [{
            "credentialId": "33333333-3333-4333-8333-333333333333",
            "credentialGeneration": 3,
            "models": ["chat-model"],
            "endpoints": ["models", "chat-completions"],
            "limits": {
                "maxConcurrentRequests": 2,
                "requestsPerMinute": 60,
                "requestBurst": 2,
                "tokensPerMinute": 10000
            }
        }],
        "binding": {
            "domainClaimId": Uuid::now_v7(),
            "gatewayScopeId": Uuid::now_v7(),
            "hostname": "api.example.com",
            "pathPrefix": "/v1",
            "bindingGeneration": 1
        }
    })
}

async fn create_environment(
    app: &BootApplication,
    organization: &str,
    project: &str,
    idempotency_key: &str,
    name: &str,
) -> Result<String> {
    let response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            idempotency_key,
            json!({"name": name}),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    response_id(&response)
}
