use super::*;
use crate::modules::edge::domain::events::{DomainClaimChanged, GatewayScopeCreated};
use crate::modules::edge::domain::repositories::{
    CreateDomainClaimWrite, CreateGatewayScopeWrite, TransitionDomainClaim,
};
use crate::modules::edge::domain::{DomainClaim, DomainNamePattern, GatewayScope};
use crate::modules::edge::InMemoryEdgeRepository;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, GatewayScopeId, IdempotencyRequest, NodeId,
};
use chrono::Duration;
use uuid::Uuid;

const INFERENCE_ROUTE_READ_TOKEN: &str =
    "a3s_3333333333333333333333333333333333333333333333333333333333333333";
const INFERENCE_ROUTE_WRITE_TOKEN: &str =
    "a3s_4444444444444444444444444444444444444444444444444444444444444444";

#[tokio::test]
async fn inference_route_publish_and_retire_require_write_scope_and_idempotency() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let app = build_test_application_with_edge(identity, projects, Arc::clone(&edge))?;
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

    let organization_id = OrganizationId::from_uuid(parse_uuid(&organization, "organization")?);
    let project_id = ProjectId::from_uuid(parse_uuid(&project, "project")?);
    let environment_id = EnvironmentId::from_uuid(parse_uuid(&environment, "environment")?);
    let (domain_claim_id, gateway_scope_id) = seed_verified_binding(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "api.example.com",
    )
    .await?;
    let (credential_id, credential_generation) = create_inference_key(
        &app,
        &organization,
        &project,
        &environment,
        "inference-route:create-key",
    )
    .await?;

    let routes_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference/routes"
    );
    let body = publish_body(
        domain_claim_id,
        gateway_scope_id,
        "api.example.com",
        credential_id,
        credential_generation,
    );

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
    let aggregate_version = response_json(&published)?["data"]["aggregateVersion"]
        .as_u64()
        .expect("aggregateVersion");
    assert_eq!(
        response_json(&published)?["data"]["router"],
        json!("inference")
    );

    let retire_path = format!("{routes_path}/{route_id}/retire");
    let retire_body = json!({ "expectedAggregateVersion": aggregate_version });
    let missing_retire_idempotency = app
        .call(
            BootRequest::new(HttpMethod::Post, &retire_path)
                .with_header(
                    "authorization",
                    format!("Bearer {INFERENCE_ROUTE_WRITE_TOKEN}"),
                )
                .with_header("content-type", "application/json")
                .with_body(retire_body.to_string().into_bytes()),
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
                .with_header("idempotency-key", "inference-route:retire")
                .with_body(retire_body.to_string().into_bytes()),
        )
        .await?;
    assert_eq!(retired.status(), 202);
    assert_no_store(&retired);
    assert!(response_json(&retired)?["data"]["retiredAt"].is_string());

    let stale_retire = app
        .call(
            BootRequest::new(HttpMethod::Post, &retire_path)
                .with_header(
                    "authorization",
                    format!("Bearer {INFERENCE_ROUTE_WRITE_TOKEN}"),
                )
                .with_header("content-type", "application/json")
                .with_header("idempotency-key", "inference-route:retire-stale")
                .with_body(
                    json!({ "expectedAggregateVersion": aggregate_version })
                        .to_string()
                        .into_bytes(),
                ),
        )
        .await?;
    assert_eq!(stale_retire.status(), 409);

    let zero_retire = app
        .call(
            BootRequest::new(HttpMethod::Post, &retire_path)
                .with_header(
                    "authorization",
                    format!("Bearer {INFERENCE_ROUTE_WRITE_TOKEN}"),
                )
                .with_header("content-type", "application/json")
                .with_header("idempotency-key", "inference-route:retire-zero")
                .with_body(
                    json!({ "expectedAggregateVersion": 0 })
                        .to_string()
                        .into_bytes(),
                ),
        )
        .await?;
    assert_eq!(zero_retire.status(), 422);
    Ok(())
}

#[tokio::test]
async fn inference_route_publish_rejects_stale_grant_credential_generation() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let app = build_test_application_with_edge(identity, projects, Arc::clone(&edge))?;
    let organization = bootstrap_organization(
        &app,
        "inference-route-grant-http",
        "Inference grant admission",
    )
    .await?;
    let project = create_project(
        &app,
        &organization,
        "inference-route-grant-project",
        "Inference Grant",
    )
    .await?;
    let environment = create_environment(
        &app,
        &organization,
        &project,
        "inference-route-grant-environment",
        "Production",
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "inference-route-grant-write-token",
        "inference-route-grant-write",
        INFERENCE_ROUTE_WRITE_TOKEN,
        &[ApiTokenScope::INFERENCE_WRITE],
        None,
    )
    .await?;

    let organization_id = OrganizationId::from_uuid(parse_uuid(&organization, "organization")?);
    let project_id = ProjectId::from_uuid(parse_uuid(&project, "project")?);
    let environment_id = EnvironmentId::from_uuid(parse_uuid(&environment, "environment")?);
    let (domain_claim_id, gateway_scope_id) = seed_verified_binding(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "grant.example.com",
    )
    .await?;
    let (credential_id, credential_generation) = create_inference_key(
        &app,
        &organization,
        &project,
        &environment,
        "inference-route:grant-create-key",
    )
    .await?;

    let routes_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference/routes"
    );
    let rejected = app
        .call(post_json_as(
            &routes_path,
            "inference-route:grant-stale-generation",
            publish_body(
                domain_claim_id,
                gateway_scope_id,
                "grant.example.com",
                credential_id,
                credential_generation + 1,
            ),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(rejected.status(), 422);
    let body = response_json(&rejected)?;
    let message = body["message"]
        .as_str()
        .or_else(|| body["error"].as_str())
        .or_else(|| body["details"].as_str())
        .unwrap_or_default();
    let serialized = body.to_string();
    assert!(
        message.contains("INFERENCE_GRANT_CREDENTIAL_INVALID")
            || serialized.contains("INFERENCE_GRANT_CREDENTIAL_INVALID"),
        "expected grant credential admission fail-closed, got {body}"
    );
    Ok(())
}

#[tokio::test]
async fn inference_route_publish_rejects_unverified_edge_binding() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let app = build_test_application_with_edge(identity, projects, Arc::clone(&edge))?;
    let organization = bootstrap_organization(
        &app,
        "inference-route-binding-http",
        "Inference binding admission",
    )
    .await?;
    let project = create_project(
        &app,
        &organization,
        "inference-route-binding-project",
        "Inference Binding",
    )
    .await?;
    let environment = create_environment(
        &app,
        &organization,
        &project,
        "inference-route-binding-environment",
        "Production",
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "inference-route-binding-write-token",
        "inference-route-binding-write",
        INFERENCE_ROUTE_WRITE_TOKEN,
        &[ApiTokenScope::INFERENCE_WRITE],
        None,
    )
    .await?;

    let organization_id = OrganizationId::from_uuid(parse_uuid(&organization, "organization")?);
    let project_id = ProjectId::from_uuid(parse_uuid(&project, "project")?);
    let environment_id = EnvironmentId::from_uuid(parse_uuid(&environment, "environment")?);
    let (credential_id, credential_generation) = create_inference_key(
        &app,
        &organization,
        &project,
        &environment,
        "inference-route:binding-create-key",
    )
    .await?;

    // Pending (unverified) DomainClaim must not invent Edge binding admission.
    let now = Utc::now();
    let claim = DomainClaim::create(
        DomainClaimId::new(),
        organization_id,
        project_id,
        environment_id,
        DomainNamePattern::parse("pending.example.com").map_err(BootError::Internal)?,
        format!("a3s-cloud-verification={}", Uuid::now_v7()),
        now,
    )
    .map_err(BootError::Internal)?;
    let created = DomainClaimChanged::envelope(&claim, Uuid::now_v7())
        .map_err(|error| BootError::Internal(error.to_string()))?;
    edge.create_domain_claim(CreateDomainClaimWrite {
        claim: claim.clone(),
        idempotency: IdempotencyRequest::new(
            "test-domain-claims",
            claim.id.to_string(),
            claim.pattern.as_str().as_bytes(),
        )
        .map_err(BootError::Internal)?,
        event: created,
    })
    .await
    .map_err(|error| BootError::Internal(error.to_string()))?;
    let scope = GatewayScope::create(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        NodeId::new(),
        now,
    )
    .map_err(BootError::Internal)?;
    edge.create_gateway_scope(CreateGatewayScopeWrite {
        scope: scope.clone(),
        idempotency: IdempotencyRequest::new(
            "test-gateway-scopes",
            scope.id.to_string(),
            scope.node_id.to_string().as_bytes(),
        )
        .map_err(BootError::Internal)?,
        event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7())
            .map_err(|error| BootError::Internal(error.to_string()))?,
    })
    .await
    .map_err(|error| BootError::Internal(error.to_string()))?;

    let routes_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference/routes"
    );
    let rejected = app
        .call(post_json_as(
            &routes_path,
            "inference-route:pending-binding",
            publish_body(
                claim.id,
                scope.id,
                "pending.example.com",
                credential_id,
                credential_generation,
            ),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(rejected.status(), 422);
    let body = response_json(&rejected)?;
    let serialized = body.to_string();
    assert!(
        serialized.contains("EDGE_ROUTE_BINDING_INVALID"),
        "expected Edge binding admission fail-closed, got {body}"
    );
    Ok(())
}

#[tokio::test]
async fn inference_route_list_and_get_require_read_scope() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let app = build_test_application_with_edge(identity, projects, Arc::clone(&edge))?;
    let organization =
        bootstrap_organization(&app, "inference-route-read-http", "Inference route reads").await?;
    let project = create_project(
        &app,
        &organization,
        "inference-route-read-project",
        "Inference Route Reads",
    )
    .await?;
    let environment = create_environment(
        &app,
        &organization,
        &project,
        "inference-route-read-environment",
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

    let organization_id = OrganizationId::from_uuid(parse_uuid(&organization, "organization")?);
    let project_id = ProjectId::from_uuid(parse_uuid(&project, "project")?);
    let environment_id = EnvironmentId::from_uuid(parse_uuid(&environment, "environment")?);
    let (domain_claim_id, gateway_scope_id) = seed_verified_binding(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "read.example.com",
    )
    .await?;
    let (credential_id, credential_generation) = create_inference_key(
        &app,
        &organization,
        &project,
        &environment,
        "inference-route:create-key-for-read",
    )
    .await?;

    let routes_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference/routes"
    );
    let published = app
        .call(post_json_as(
            &routes_path,
            "inference-route:publish-for-read",
            publish_body(
                domain_claim_id,
                gateway_scope_id,
                "read.example.com",
                credential_id,
                credential_generation,
            ),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(published.status(), 202);
    let route_id = response_id(&published)?;
    let aggregate_version = response_json(&published)?["data"]["aggregateVersion"]
        .as_u64()
        .expect("aggregateVersion");
    let route_path = format!("{routes_path}/{route_id}");

    assert_eq!(
        app.call(BootRequest::new(HttpMethod::Get, &routes_path))
            .await?
            .status(),
        401
    );
    assert_eq!(
        app.call(BootRequest::new(HttpMethod::Get, &route_path))
            .await?
            .status(),
        401
    );

    let write_only_list = app
        .call(BootRequest::new(HttpMethod::Get, &routes_path).with_header(
            "authorization",
            format!("Bearer {INFERENCE_ROUTE_WRITE_TOKEN}"),
        ))
        .await?;
    assert_eq!(write_only_list.status(), 403);

    let listed = app
        .call(BootRequest::new(HttpMethod::Get, &routes_path).with_header(
            "authorization",
            format!("Bearer {INFERENCE_ROUTE_READ_TOKEN}"),
        ))
        .await?;
    assert_eq!(listed.status(), 200);
    let listed_json = response_json(&listed)?;
    assert_eq!(listed_json["data"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(listed_json["data"]["items"][0]["id"], json!(route_id));
    assert!(listed_json["data"]["items"][0]["retiredAt"].is_null());
    assert!(listed_json["data"]["nextCursor"].is_null());

    let fetched = app
        .call(BootRequest::new(HttpMethod::Get, &route_path).with_header(
            "authorization",
            format!("Bearer {INFERENCE_ROUTE_READ_TOKEN}"),
        ))
        .await?;
    assert_eq!(fetched.status(), 200);
    let fetched_json = response_json(&fetched)?;
    assert_eq!(fetched_json["data"]["id"], json!(route_id));
    assert_eq!(fetched_json["data"]["policyRevision"], json!(1));
    assert!(fetched_json["data"]["models"].is_array());
    assert!(fetched_json["data"]["grants"].is_array());
    assert!(fetched_json["data"]["binding"].is_object());

    let retire_path = format!("{routes_path}/{route_id}/retire");
    let retired = app
        .call(
            BootRequest::new(HttpMethod::Post, &retire_path)
                .with_header(
                    "authorization",
                    format!("Bearer {INFERENCE_ROUTE_WRITE_TOKEN}"),
                )
                .with_header("content-type", "application/json")
                .with_header("idempotency-key", "inference-route:retire-for-read")
                .with_body(
                    json!({ "expectedAggregateVersion": aggregate_version })
                        .to_string()
                        .into_bytes(),
                ),
        )
        .await?;
    assert_eq!(retired.status(), 202);

    let listed_after_retire = app
        .call(BootRequest::new(HttpMethod::Get, &routes_path).with_header(
            "authorization",
            format!("Bearer {INFERENCE_ROUTE_READ_TOKEN}"),
        ))
        .await?;
    assert_eq!(listed_after_retire.status(), 200);
    assert!(response_json(&listed_after_retire)?["data"]["items"]
        .as_array()
        .unwrap()
        .is_empty());

    let fetched_retired = app
        .call(BootRequest::new(HttpMethod::Get, &route_path).with_header(
            "authorization",
            format!("Bearer {INFERENCE_ROUTE_READ_TOKEN}"),
        ))
        .await?;
    assert_eq!(fetched_retired.status(), 200);
    assert!(response_json(&fetched_retired)?["data"]["retiredAt"].is_string());
    Ok(())
}

#[tokio::test]
async fn inference_route_revise_requires_write_scope_cas_and_idempotency() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let app = build_test_application_with_edge(identity, projects, Arc::clone(&edge))?;
    let organization =
        bootstrap_organization(&app, "inference-route-revise", "Inference revise tenant").await?;
    let project = create_project(
        &app,
        &organization,
        "inference-route-revise-project",
        "Inference Revise",
    )
    .await?;
    let environment = create_environment(
        &app,
        &organization,
        &project,
        "inference-route-revise-environment",
        "Production",
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "inference-route-revise-read-token",
        "inference-route-revise-read",
        INFERENCE_ROUTE_READ_TOKEN,
        &[ApiTokenScope::INFERENCE_READ],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "inference-route-revise-write-token",
        "inference-route-revise-write",
        INFERENCE_ROUTE_WRITE_TOKEN,
        &[ApiTokenScope::INFERENCE_WRITE],
        None,
    )
    .await?;

    let organization_id = OrganizationId::from_uuid(parse_uuid(&organization, "organization")?);
    let project_id = ProjectId::from_uuid(parse_uuid(&project, "project")?);
    let environment_id = EnvironmentId::from_uuid(parse_uuid(&environment, "environment")?);
    let (domain_claim_id, gateway_scope_id) = seed_verified_binding(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "revise.example.com",
    )
    .await?;
    let (credential_id, credential_generation) = create_inference_key(
        &app,
        &organization,
        &project,
        &environment,
        "inference-route:revise-create-key",
    )
    .await?;

    let routes_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference/routes"
    );
    let published = app
        .call(post_json_as(
            &routes_path,
            "inference-route:revise-publish",
            publish_body(
                domain_claim_id,
                gateway_scope_id,
                "revise.example.com",
                credential_id,
                credential_generation,
            ),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(published.status(), 202);
    let route_id = response_id(&published)?;
    let aggregate_version = response_json(&published)?["data"]["aggregateVersion"]
        .as_u64()
        .ok_or_else(|| BootError::Internal("missing publish aggregateVersion".into()))?;
    assert_eq!(
        response_json(&published)?["data"]["policyRevision"],
        json!(1)
    );

    let revise_path = format!("{routes_path}/{route_id}/revisions");
    let revise_request = revise_body(
        aggregate_version,
        domain_claim_id,
        gateway_scope_id,
        "revise.example.com",
        credential_id,
        credential_generation,
    );

    let missing_idempotency = app
        .call(
            BootRequest::new(HttpMethod::Post, &revise_path)
                .with_header(
                    "authorization",
                    format!("Bearer {INFERENCE_ROUTE_WRITE_TOKEN}"),
                )
                .with_header("content-type", "application/json")
                .with_body(revise_request.to_string().into_bytes()),
        )
        .await?;
    assert_eq!(missing_idempotency.status(), 400);

    let insufficient = app
        .call(
            BootRequest::new(HttpMethod::Post, &revise_path)
                .with_header(
                    "authorization",
                    format!("Bearer {INFERENCE_ROUTE_READ_TOKEN}"),
                )
                .with_header("content-type", "application/json")
                .with_header("idempotency-key", "inference-route:revise-read")
                .with_body(revise_request.to_string().into_bytes()),
        )
        .await?;
    assert_eq!(insufficient.status(), 403);

    let revised = app
        .call(
            BootRequest::new(HttpMethod::Post, &revise_path)
                .with_header(
                    "authorization",
                    format!("Bearer {INFERENCE_ROUTE_WRITE_TOKEN}"),
                )
                .with_header("content-type", "application/json")
                .with_header("idempotency-key", "inference-route:revise")
                .with_body(revise_request.to_string().into_bytes()),
        )
        .await?;
    assert_eq!(revised.status(), 202);
    assert_no_store(&revised);
    let revised_json = response_json(&revised)?;
    assert_eq!(revised_json["data"]["id"], json!(route_id));
    assert_eq!(revised_json["data"]["policyRevision"], json!(2));
    assert_eq!(
        revised_json["data"]["aggregateVersion"],
        json!(aggregate_version + 1)
    );
    assert_eq!(
        revised_json["data"]["models"][0]["alias"],
        json!("chat-model-v2")
    );

    let replayed = app
        .call(
            BootRequest::new(HttpMethod::Post, &revise_path)
                .with_header(
                    "authorization",
                    format!("Bearer {INFERENCE_ROUTE_WRITE_TOKEN}"),
                )
                .with_header("content-type", "application/json")
                .with_header("idempotency-key", "inference-route:revise")
                .with_body(revise_request.to_string().into_bytes()),
        )
        .await?;
    assert_eq!(replayed.status(), 202);
    assert_eq!(
        response_json(&replayed)?["data"]["aggregateVersion"],
        revised_json["data"]["aggregateVersion"]
    );

    let stale = app
        .call(
            BootRequest::new(HttpMethod::Post, &revise_path)
                .with_header(
                    "authorization",
                    format!("Bearer {INFERENCE_ROUTE_WRITE_TOKEN}"),
                )
                .with_header("content-type", "application/json")
                .with_header("idempotency-key", "inference-route:revise-stale")
                .with_body(
                    revise_body(
                        aggregate_version,
                        domain_claim_id,
                        gateway_scope_id,
                        "revise.example.com",
                        credential_id,
                        credential_generation,
                    )
                    .to_string()
                    .into_bytes(),
                ),
        )
        .await?;
    assert_eq!(stale.status(), 409);

    Ok(())
}

#[tokio::test]
async fn inference_route_revise_rejects_stale_grant_credential_generation() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let app = build_test_application_with_edge(identity, projects, Arc::clone(&edge))?;
    let organization = bootstrap_organization(
        &app,
        "inference-route-revise-grant-http",
        "Inference revise grant admission",
    )
    .await?;
    let project = create_project(
        &app,
        &organization,
        "inference-route-revise-grant-project",
        "Inference Revise Grant",
    )
    .await?;
    let environment = create_environment(
        &app,
        &organization,
        &project,
        "inference-route-revise-grant-environment",
        "Production",
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "inference-route-revise-grant-write-token",
        "inference-route-revise-grant-write",
        INFERENCE_ROUTE_WRITE_TOKEN,
        &[ApiTokenScope::INFERENCE_WRITE, ApiTokenScope::INFERENCE_READ],
        None,
    )
    .await?;

    let organization_id = OrganizationId::from_uuid(parse_uuid(&organization, "organization")?);
    let project_id = ProjectId::from_uuid(parse_uuid(&project, "project")?);
    let environment_id = EnvironmentId::from_uuid(parse_uuid(&environment, "environment")?);
    let (domain_claim_id, gateway_scope_id) = seed_verified_binding(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "revise-grant.example.com",
    )
    .await?;
    let (credential_id, credential_generation) = create_inference_key(
        &app,
        &organization,
        &project,
        &environment,
        "inference-route:revise-grant-create-key",
    )
    .await?;

    let routes_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference/routes"
    );
    let published = app
        .call(post_json_as(
            &routes_path,
            "inference-route:revise-grant-publish",
            publish_body(
                domain_claim_id,
                gateway_scope_id,
                "revise-grant.example.com",
                credential_id,
                credential_generation,
            ),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(published.status(), 202);
    let published_json = response_json(&published)?;
    let route_id = published_json["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("missing route id".into()))?;
    let aggregate_version = published_json["data"]["aggregateVersion"]
        .as_u64()
        .ok_or_else(|| BootError::Internal("missing aggregateVersion".into()))?;

    let revise_path = format!("{routes_path}/{route_id}/revisions");
    let rejected = app
        .call(post_json_as(
            &revise_path,
            "inference-route:revise-grant-stale-generation",
            revise_body(
                aggregate_version,
                domain_claim_id,
                gateway_scope_id,
                "revise-grant.example.com",
                credential_id,
                credential_generation + 1,
            ),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(rejected.status(), 422);
    let body = response_json(&rejected)?;
    let serialized = body.to_string();
    assert!(
        serialized.contains("INFERENCE_GRANT_CREDENTIAL_INVALID"),
        "expected revise grant credential admission fail-closed, got {body}"
    );

    let fetched = app
        .call(
            BootRequest::new(HttpMethod::Get, format!("{routes_path}/{route_id}")).with_header(
                "authorization",
                format!("Bearer {INFERENCE_ROUTE_WRITE_TOKEN}"),
            ),
        )
        .await?;
    assert_eq!(fetched.status(), 200);
    assert_eq!(
        response_json(&fetched)?["data"]["aggregateVersion"],
        json!(aggregate_version),
        "failed revise must not advance aggregate version"
    );
    Ok(())
}

#[tokio::test]
async fn inference_route_revise_rejects_unverified_edge_binding() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let app = build_test_application_with_edge(identity, projects, Arc::clone(&edge))?;
    let organization = bootstrap_organization(
        &app,
        "inference-route-revise-binding-http",
        "Inference revise binding admission",
    )
    .await?;
    let project = create_project(
        &app,
        &organization,
        "inference-route-revise-binding-project",
        "Inference Revise Binding",
    )
    .await?;
    let environment = create_environment(
        &app,
        &organization,
        &project,
        "inference-route-revise-binding-environment",
        "Production",
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "inference-route-revise-binding-write-token",
        "inference-route-revise-binding-write",
        INFERENCE_ROUTE_WRITE_TOKEN,
        &[ApiTokenScope::INFERENCE_WRITE, ApiTokenScope::INFERENCE_READ],
        None,
    )
    .await?;

    let organization_id = OrganizationId::from_uuid(parse_uuid(&organization, "organization")?);
    let project_id = ProjectId::from_uuid(parse_uuid(&project, "project")?);
    let environment_id = EnvironmentId::from_uuid(parse_uuid(&environment, "environment")?);
    let (domain_claim_id, gateway_scope_id) = seed_verified_binding(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "revise-binding.example.com",
    )
    .await?;
    let (credential_id, credential_generation) = create_inference_key(
        &app,
        &organization,
        &project,
        &environment,
        "inference-route:revise-binding-create-key",
    )
    .await?;

    let routes_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference/routes"
    );
    let published = app
        .call(post_json_as(
            &routes_path,
            "inference-route:revise-binding-publish",
            publish_body(
                domain_claim_id,
                gateway_scope_id,
                "revise-binding.example.com",
                credential_id,
                credential_generation,
            ),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(published.status(), 202);
    let published_json = response_json(&published)?;
    let route_id = published_json["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("missing route id".into()))?;
    let aggregate_version = published_json["data"]["aggregateVersion"]
        .as_u64()
        .ok_or_else(|| BootError::Internal("missing aggregateVersion".into()))?;

    let now = Utc::now();
    let pending = DomainClaim::create(
        DomainClaimId::new(),
        organization_id,
        project_id,
        environment_id,
        DomainNamePattern::parse("revise-pending.example.com").map_err(BootError::Internal)?,
        format!("a3s-cloud-verification={}", Uuid::now_v7()),
        now,
    )
    .map_err(BootError::Internal)?;
    let created = DomainClaimChanged::envelope(&pending, Uuid::now_v7())
        .map_err(|error| BootError::Internal(error.to_string()))?;
    edge.create_domain_claim(CreateDomainClaimWrite {
        claim: pending.clone(),
        idempotency: IdempotencyRequest::new(
            "test-domain-claims",
            pending.id.to_string(),
            pending.pattern.as_str().as_bytes(),
        )
        .map_err(BootError::Internal)?,
        event: created,
    })
    .await
    .map_err(|error| BootError::Internal(error.to_string()))?;

    let revise_path = format!("{routes_path}/{route_id}/revisions");
    let rejected = app
        .call(post_json_as(
            &revise_path,
            "inference-route:revise-pending-binding",
            revise_body(
                aggregate_version,
                pending.id,
                gateway_scope_id,
                "revise-pending.example.com",
                credential_id,
                credential_generation,
            ),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(rejected.status(), 422);
    let body = response_json(&rejected)?;
    let serialized = body.to_string();
    assert!(
        serialized.contains("EDGE_ROUTE_BINDING_INVALID"),
        "expected revise Edge binding admission fail-closed, got {body}"
    );

    let fetched = app
        .call(
            BootRequest::new(HttpMethod::Get, format!("{routes_path}/{route_id}")).with_header(
                "authorization",
                format!("Bearer {INFERENCE_ROUTE_WRITE_TOKEN}"),
            ),
        )
        .await?;
    assert_eq!(fetched.status(), 200);
    assert_eq!(
        response_json(&fetched)?["data"]["aggregateVersion"],
        json!(aggregate_version),
        "failed revise must not advance aggregate version"
    );
    Ok(())
}

#[tokio::test]
async fn inference_route_reads_fail_closed_for_ungranted_environment() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let app = build_test_application_with_edge(identity, projects, Arc::clone(&edge))?;
    let organization = bootstrap_organization(
        &app,
        "inference-route-visibility-http",
        "Inference route visibility",
    )
    .await?;
    let project = create_project(
        &app,
        &organization,
        "inference-route-visibility-project",
        "Inference Route Visibility",
    )
    .await?;
    let environment = create_environment(
        &app,
        &organization,
        &project,
        "inference-route-visibility-environment",
        "Production",
    )
    .await?;
    let other_environment = create_environment(
        &app,
        &organization,
        &project,
        "inference-route-visibility-other",
        "Staging",
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "inference-route-visibility-write-token",
        "inference-route-visibility-write",
        INFERENCE_ROUTE_WRITE_TOKEN,
        &[ApiTokenScope::INFERENCE_WRITE],
        None,
    )
    .await?;

    let organization_id = OrganizationId::from_uuid(parse_uuid(&organization, "organization")?);
    let project_id = ProjectId::from_uuid(parse_uuid(&project, "project")?);
    let environment_id = EnvironmentId::from_uuid(parse_uuid(&environment, "environment")?);
    let (domain_claim_id, gateway_scope_id) = seed_verified_binding(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "visibility.example.com",
    )
    .await?;
    let (credential_id, credential_generation) = create_inference_key(
        &app,
        &organization,
        &project,
        &environment,
        "inference-route:visibility-create-key",
    )
    .await?;

    let routes_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference/routes"
    );
    let published = app
        .call(post_json_as(
            &routes_path,
            "inference-route:visibility-publish",
            publish_body(
                domain_claim_id,
                gateway_scope_id,
                "visibility.example.com",
                credential_id,
                credential_generation,
            ),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(published.status(), 202);
    let route_id = response_json(&published)?["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("missing route id".into()))?
        .to_owned();

    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "inference-route-visibility-membership",
            json!({"name": "Restricted route reader", "role": "restricted"}),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let membership = response_json(&membership)?;
    let membership_id = membership["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted membership has no ID".into()))?;
    let principal_id = membership["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted principal has no ID".into()))?;
    let restricted = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "inference-route-visibility-restricted-token",
            json!({
                "name": "Restricted route reader",
                "token": INFERENCE_ROUTE_READ_TOKEN,
                "scopes": [ApiTokenScope::INFERENCE_READ],
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(restricted.status(), 201);
    let grant = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/memberships/{membership_id}/resource-grants"
            ),
            "inference-route-visibility-grant",
            json!({
                "scope": {
                    "kind": "environment",
                    "projectId": project,
                    "environmentId": other_environment
                }
            }),
        ))
        .await?;
    assert_eq!(grant.status(), 201);

    let denied_list = app
        .call(get_as(&routes_path, INFERENCE_ROUTE_READ_TOKEN))
        .await?;
    assert_eq!(
        denied_list.status(),
        403,
        "restricted tokens without environment grant must fail closed on route list"
    );
    let denied_get = app
        .call(get_as(
            format!("{routes_path}/{route_id}"),
            INFERENCE_ROUTE_READ_TOKEN,
        ))
        .await?;
    assert_eq!(
        denied_get.status(),
        403,
        "restricted tokens without environment grant must fail closed on route get"
    );
    Ok(())
}

#[tokio::test]
async fn inference_route_retire_rejects_wrong_environment_path_as_not_found() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let app = build_test_application_with_edge(identity, projects, Arc::clone(&edge))?;
    let organization = bootstrap_organization(
        &app,
        "inference-route-retire-scope-http",
        "Inference retire path scope",
    )
    .await?;
    let project = create_project(
        &app,
        &organization,
        "inference-route-retire-scope-project",
        "Inference Retire Scope",
    )
    .await?;
    let environment = create_environment(
        &app,
        &organization,
        &project,
        "inference-route-retire-scope-environment",
        "Production",
    )
    .await?;
    let other_environment = create_environment(
        &app,
        &organization,
        &project,
        "inference-route-retire-scope-other",
        "Staging",
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "inference-route-retire-scope-write-token",
        "inference-route-retire-scope-write",
        INFERENCE_ROUTE_WRITE_TOKEN,
        &[ApiTokenScope::INFERENCE_WRITE, ApiTokenScope::INFERENCE_READ],
        None,
    )
    .await?;

    let organization_id = OrganizationId::from_uuid(parse_uuid(&organization, "organization")?);
    let project_id = ProjectId::from_uuid(parse_uuid(&project, "project")?);
    let environment_id = EnvironmentId::from_uuid(parse_uuid(&environment, "environment")?);
    let (domain_claim_id, gateway_scope_id) = seed_verified_binding(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "retire-scope.example.com",
    )
    .await?;
    let (credential_id, credential_generation) = create_inference_key(
        &app,
        &organization,
        &project,
        &environment,
        "inference-route:retire-scope-create-key",
    )
    .await?;

    let routes_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference/routes"
    );
    let published = app
        .call(post_json_as(
            &routes_path,
            "inference-route:retire-scope-publish",
            publish_body(
                domain_claim_id,
                gateway_scope_id,
                "retire-scope.example.com",
                credential_id,
                credential_generation,
            ),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(published.status(), 202);
    let published_json = response_json(&published)?;
    let route_id = published_json["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("missing route id".into()))?
        .to_owned();
    let aggregate_version = published_json["data"]["aggregateVersion"]
        .as_u64()
        .ok_or_else(|| BootError::Internal("missing aggregateVersion".into()))?;

    let wrong_retire_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{other_environment}/inference/routes/{route_id}/retire"
    );
    let rejected = app
        .call(post_json_as(
            &wrong_retire_path,
            "inference-route:retire-wrong-environment",
            json!({ "expectedAggregateVersion": aggregate_version }),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(rejected.status(), 404);

    let missing_env = Uuid::now_v7();
    let missing_retire_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{missing_env}/inference/routes/{route_id}/retire"
    );
    let missing = app
        .call(post_json_as(
            &missing_retire_path,
            "inference-route:retire-missing-environment",
            json!({ "expectedAggregateVersion": aggregate_version }),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(missing.status(), 404);

    let fetched = app
        .call(get_as(
            format!("{routes_path}/{route_id}"),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(fetched.status(), 200);
    let fetched_json = response_json(&fetched)?;
    assert_eq!(fetched_json["data"]["aggregateVersion"], json!(aggregate_version));
    assert!(fetched_json["data"]["retiredAt"].is_null());
    Ok(())
}

#[tokio::test]
async fn inference_route_revise_rejects_wrong_environment_path_as_not_found() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let app = build_test_application_with_edge(identity, projects, Arc::clone(&edge))?;
    let organization = bootstrap_organization(
        &app,
        "inference-route-revise-scope-http",
        "Inference revise path scope",
    )
    .await?;
    let project = create_project(
        &app,
        &organization,
        "inference-route-revise-scope-project",
        "Inference Revise Scope",
    )
    .await?;
    let environment = create_environment(
        &app,
        &organization,
        &project,
        "inference-route-revise-scope-environment",
        "Production",
    )
    .await?;
    let other_environment = create_environment(
        &app,
        &organization,
        &project,
        "inference-route-revise-scope-other",
        "Staging",
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "inference-route-revise-scope-write-token",
        "inference-route-revise-scope-write",
        INFERENCE_ROUTE_WRITE_TOKEN,
        &[ApiTokenScope::INFERENCE_WRITE, ApiTokenScope::INFERENCE_READ],
        None,
    )
    .await?;

    let organization_id = OrganizationId::from_uuid(parse_uuid(&organization, "organization")?);
    let project_id = ProjectId::from_uuid(parse_uuid(&project, "project")?);
    let environment_id = EnvironmentId::from_uuid(parse_uuid(&environment, "environment")?);
    let (domain_claim_id, gateway_scope_id) = seed_verified_binding(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "revise-scope.example.com",
    )
    .await?;
    let (credential_id, credential_generation) = create_inference_key(
        &app,
        &organization,
        &project,
        &environment,
        "inference-route:revise-scope-create-key",
    )
    .await?;

    let routes_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference/routes"
    );
    let published = app
        .call(post_json_as(
            &routes_path,
            "inference-route:revise-scope-publish",
            publish_body(
                domain_claim_id,
                gateway_scope_id,
                "revise-scope.example.com",
                credential_id,
                credential_generation,
            ),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(published.status(), 202);
    let published_json = response_json(&published)?;
    let route_id = published_json["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("missing route id".into()))?
        .to_owned();
    let aggregate_version = published_json["data"]["aggregateVersion"]
        .as_u64()
        .ok_or_else(|| BootError::Internal("missing aggregateVersion".into()))?;
    let policy_revision = published_json["data"]["policyRevision"]
        .as_u64()
        .ok_or_else(|| BootError::Internal("missing policyRevision".into()))?;

    let wrong_revise_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{other_environment}/inference/routes/{route_id}/revisions"
    );
    let rejected = app
        .call(post_json_as(
            &wrong_revise_path,
            "inference-route:revise-wrong-environment",
            revise_body(
                aggregate_version,
                domain_claim_id,
                gateway_scope_id,
                "revise-scope.example.com",
                credential_id,
                credential_generation,
            ),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(
        rejected.status(),
        404,
        "wrong-environment revise must NotFound before Edge binding admission, got {}",
        rejected.status()
    );

    let missing_env = Uuid::now_v7();
    let missing_revise_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{missing_env}/inference/routes/{route_id}/revisions"
    );
    let missing = app
        .call(post_json_as(
            &missing_revise_path,
            "inference-route:revise-missing-environment",
            revise_body(
                aggregate_version,
                domain_claim_id,
                gateway_scope_id,
                "revise-scope.example.com",
                credential_id,
                credential_generation,
            ),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(missing.status(), 404);

    let fetched = app
        .call(get_as(
            format!("{routes_path}/{route_id}"),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(fetched.status(), 200);
    let fetched_json = response_json(&fetched)?;
    assert_eq!(fetched_json["data"]["aggregateVersion"], json!(aggregate_version));
    assert_eq!(fetched_json["data"]["policyRevision"], json!(policy_revision));
    Ok(())
}

#[tokio::test]
async fn inference_route_publish_rejects_missing_environment_path_as_not_found() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let app = build_test_application_with_edge(identity, projects, Arc::clone(&edge))?;
    let organization = bootstrap_organization(
        &app,
        "inference-route-publish-missing-env",
        "Inference publish missing env",
    )
    .await?;
    let project = create_project(
        &app,
        &organization,
        "inference-route-publish-missing-env-project",
        "Inference Publish Missing Env",
    )
    .await?;
    let environment = create_environment(
        &app,
        &organization,
        &project,
        "inference-route-publish-missing-env-environment",
        "Production",
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "inference-route-publish-missing-env-write",
        "inference-route-publish-missing-env-write",
        INFERENCE_ROUTE_WRITE_TOKEN,
        &[ApiTokenScope::INFERENCE_WRITE, ApiTokenScope::INFERENCE_READ],
        None,
    )
    .await?;

    let organization_id = OrganizationId::from_uuid(parse_uuid(&organization, "organization")?);
    let project_id = ProjectId::from_uuid(parse_uuid(&project, "project")?);
    let environment_id = EnvironmentId::from_uuid(parse_uuid(&environment, "environment")?);
    let (domain_claim_id, gateway_scope_id) = seed_verified_binding(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "publish-missing.example.com",
    )
    .await?;
    let (credential_id, credential_generation) = create_inference_key(
        &app,
        &organization,
        &project,
        &environment,
        "inference-route:publish-missing-env-create-key",
    )
    .await?;

    let missing_environment = Uuid::now_v7();
    let missing_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{missing_environment}/inference/routes"
    );
    let rejected = app
        .call(post_json_as(
            &missing_path,
            "inference-route:publish-missing-environment",
            publish_body(
                domain_claim_id,
                gateway_scope_id,
                "publish-missing.example.com",
                credential_id,
                credential_generation,
            ),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(rejected.status(), 404);

    let listed = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference/routes"
            ),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(listed.status(), 200);
    assert!(
        response_json(&listed)?["data"]["items"]
            .as_array()
            .map(Vec::is_empty)
            .unwrap_or(false),
        "missing-environment publish must not persist a route in the real environment"
    );
    Ok(())
}

#[tokio::test]
async fn inference_route_get_rejects_wrong_environment_path_as_not_found() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let app = build_test_application_with_edge(identity, projects, Arc::clone(&edge))?;
    let organization = bootstrap_organization(
        &app,
        "inference-route-get-scope-http",
        "Inference get path scope",
    )
    .await?;
    let project = create_project(
        &app,
        &organization,
        "inference-route-get-scope-project",
        "Inference Get Scope",
    )
    .await?;
    let environment = create_environment(
        &app,
        &organization,
        &project,
        "inference-route-get-scope-environment",
        "Production",
    )
    .await?;
    let other_environment = create_environment(
        &app,
        &organization,
        &project,
        "inference-route-get-scope-other",
        "Staging",
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "inference-route-get-scope-write-token",
        "inference-route-get-scope-write",
        INFERENCE_ROUTE_WRITE_TOKEN,
        &[ApiTokenScope::INFERENCE_WRITE, ApiTokenScope::INFERENCE_READ],
        None,
    )
    .await?;

    let organization_id = OrganizationId::from_uuid(parse_uuid(&organization, "organization")?);
    let project_id = ProjectId::from_uuid(parse_uuid(&project, "project")?);
    let environment_id = EnvironmentId::from_uuid(parse_uuid(&environment, "environment")?);
    let (domain_claim_id, gateway_scope_id) = seed_verified_binding(
        &edge,
        organization_id,
        project_id,
        environment_id,
        "get-scope.example.com",
    )
    .await?;
    let (credential_id, credential_generation) = create_inference_key(
        &app,
        &organization,
        &project,
        &environment,
        "inference-route:get-scope-create-key",
    )
    .await?;

    let routes_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference/routes"
    );
    let published = app
        .call(post_json_as(
            &routes_path,
            "inference-route:get-scope-publish",
            publish_body(
                domain_claim_id,
                gateway_scope_id,
                "get-scope.example.com",
                credential_id,
                credential_generation,
            ),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(published.status(), 202);
    let route_id = response_json(&published)?["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("missing route id".into()))?
        .to_owned();

    let wrong_get = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{project}/environments/{other_environment}/inference/routes/{route_id}"
            ),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(wrong_get.status(), 404);

    let fetched = app
        .call(get_as(
            format!("{routes_path}/{route_id}"),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(fetched.status(), 200);
    assert_eq!(response_json(&fetched)?["data"]["id"], json!(route_id));
    Ok(())
}

fn publish_body(
    domain_claim_id: DomainClaimId,
    gateway_scope_id: GatewayScopeId,
    hostname: &str,
    credential_id: Uuid,
    credential_generation: u64,
) -> Value {
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
            "credentialId": credential_id,
            "credentialGeneration": credential_generation,
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
            "domainClaimId": domain_claim_id.as_uuid(),
            "gatewayScopeId": gateway_scope_id.as_uuid(),
            "hostname": hostname,
            "pathPrefix": "/v1",
            "bindingGeneration": 1
        }
    })
}

fn revise_body(
    expected_aggregate_version: u64,
    domain_claim_id: DomainClaimId,
    gateway_scope_id: GatewayScopeId,
    hostname: &str,
    credential_id: Uuid,
    credential_generation: u64,
) -> Value {
    let mut body = publish_body(
        domain_claim_id,
        gateway_scope_id,
        hostname,
        credential_id,
        credential_generation,
    );
    body["expectedAggregateVersion"] = json!(expected_aggregate_version);
    body["models"] = json!([{
        "alias": "chat-model-v2",
        "modelId": "55555555-5555-4555-8555-555555555555",
        "targets": [{
            "targetId": "66666666-6666-4666-8666-666666666666",
            "service": "model-service",
            "upstreamModel": "internal/model-v2",
            "priority": 0,
            "weight": 100
        }]
    }]);
    body["grants"] = json!([{
        "credentialId": credential_id,
        "credentialGeneration": credential_generation,
        "models": ["chat-model-v2"],
        "endpoints": ["models", "chat-completions"],
        "limits": {
            "maxConcurrentRequests": 2,
            "requestsPerMinute": 60,
            "requestBurst": 2,
            "tokensPerMinute": 10000
        }
    }]);
    body
}

async fn create_inference_key(
    app: &BootApplication,
    organization: &str,
    project: &str,
    environment: &str,
    idempotency_key: &str,
) -> Result<(Uuid, u64)> {
    let path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference/keys"
    );
    let response = app
        .call(post_json_as(
            &path,
            idempotency_key,
            json!({ "expiresAt": (Utc::now() + Duration::hours(2)).to_rfc3339() }),
            INFERENCE_ROUTE_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(response.status(), 201);
    let body = response_json(&response)?;
    let credential_id = body["data"]["credential"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("missing inference key id".into()))
        .and_then(parse_uuid_label)?;
    let generation = body["data"]["credential"]["generation"]
        .as_u64()
        .ok_or_else(|| BootError::Internal("missing inference key generation".into()))?;
    Ok((credential_id, generation))
}

fn parse_uuid_label(value: &str) -> Result<Uuid> {
    parse_uuid(value, "inference credential")
}

async fn seed_verified_binding(
    edge: &Arc<InMemoryEdgeRepository>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    pattern: &str,
) -> Result<(DomainClaimId, GatewayScopeId)> {
    let now = Utc::now();
    let mut claim = DomainClaim::create(
        DomainClaimId::new(),
        organization_id,
        project_id,
        environment_id,
        DomainNamePattern::parse(pattern).map_err(BootError::Internal)?,
        format!("a3s-cloud-verification={}", Uuid::now_v7()),
        now,
    )
    .map_err(BootError::Internal)?;
    let created = DomainClaimChanged::envelope(&claim, Uuid::now_v7())
        .map_err(|error| BootError::Internal(error.to_string()))?;
    edge.create_domain_claim(CreateDomainClaimWrite {
        claim: claim.clone(),
        idempotency: IdempotencyRequest::new(
            "test-domain-claims",
            claim.id.to_string(),
            claim.pattern.as_str().as_bytes(),
        )
        .map_err(BootError::Internal)?,
        event: created,
    })
    .await
    .map_err(|error| BootError::Internal(error.to_string()))?;
    let expected_version = claim.aggregate_version;
    claim
        .verify(now + Duration::milliseconds(1))
        .map_err(BootError::Internal)?;
    let verified = DomainClaimChanged::envelope(&claim, Uuid::now_v7())
        .map_err(|error| BootError::Internal(error.to_string()))?;
    edge.transition_domain_claim(TransitionDomainClaim {
        claim: claim.clone(),
        expected_version,
        idempotency: IdempotencyRequest::new(
            "test-domain-claim-verifications",
            claim.id.to_string(),
            b"verified",
        )
        .map_err(BootError::Internal)?,
        event: verified,
    })
    .await
    .map_err(|error| BootError::Internal(error.to_string()))?;

    let scope = GatewayScope::create(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        NodeId::new(),
        now,
    )
    .map_err(BootError::Internal)?;
    edge.create_gateway_scope(CreateGatewayScopeWrite {
        scope: scope.clone(),
        idempotency: IdempotencyRequest::new(
            "test-gateway-scopes",
            scope.id.to_string(),
            scope.node_id.to_string().as_bytes(),
        )
        .map_err(BootError::Internal)?,
        event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7())
            .map_err(|error| BootError::Internal(error.to_string()))?,
    })
    .await
    .map_err(|error| BootError::Internal(error.to_string()))?;
    Ok((claim.id, scope.id))
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

fn parse_uuid(value: &str, label: &str) -> Result<Uuid> {
    Uuid::parse_str(value)
        .map_err(|error| BootError::Internal(format!("invalid {label} ID: {error}")))
}
