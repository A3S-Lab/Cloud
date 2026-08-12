use super::*;
use crate::modules::edge::domain::{
    DomainNamePattern, Route, RouteHostname, RoutePath, RoutePortName, RouteTarget,
    UpstreamEndpoint,
};
use crate::modules::edge::InMemoryEdgeRepository;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, GatewayCertificateId, NodeId, WorkloadId, WorkloadRevisionId,
};

const RESTRICTED_ROUTE_TOKEN: &str =
    "a3s_2222222222222222222222222222222222222222222222222222222222222222";

#[tokio::test]
async fn restricted_route_detail_resolves_environment_scope_and_revocation() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let edge = Arc::new(InMemoryEdgeRepository::new());
    let app = build_test_application_with_edge(identity, projects, Arc::clone(&edge))?;
    let organization = bootstrap_organization(&app, "route-grants", "Route grants").await?;

    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "route-grants-membership",
            json!({"name": "Restricted route reader", "role": "restricted"}),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let membership = response_json(&membership)?;
    let membership_id = membership["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted route membership has no ID".into()))?;
    let principal_id = membership["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted route principal has no ID".into()))?;
    let token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "route-grants-token",
            json!({
                "name": "Restricted route reader",
                "token": RESTRICTED_ROUTE_TOKEN,
                "scopes": [ApiTokenScope::ROUTE_WRITE],
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(token.status(), 201);

    let granted_project =
        create_project(&app, &organization, "route-granted-project", "Granted").await?;
    let denied_project =
        create_project(&app, &organization, "route-denied-project", "Denied").await?;
    let granted_environment = create_environment(
        &app,
        &organization,
        &granted_project,
        "route-granted-environment",
        "Granted",
    )
    .await?;
    let denied_environment = create_environment(
        &app,
        &organization,
        &denied_project,
        "route-denied-environment",
        "Denied",
    )
    .await?;

    let organization_id = OrganizationId::from_uuid(parse_uuid(&organization, "organization")?);
    let granted_project_id = ProjectId::from_uuid(parse_uuid(&granted_project, "granted project")?);
    let denied_project_id = ProjectId::from_uuid(parse_uuid(&denied_project, "denied project")?);
    let granted_environment_id =
        EnvironmentId::from_uuid(parse_uuid(&granted_environment, "granted environment")?);
    let denied_environment_id =
        EnvironmentId::from_uuid(parse_uuid(&denied_environment, "denied environment")?);
    let granted_route = route_fixture(
        organization_id,
        granted_project_id,
        granted_environment_id,
        "granted.route.test",
        18080,
    )?;
    let denied_route = route_fixture(
        organization_id,
        denied_project_id,
        denied_environment_id,
        "denied.route.test",
        18081,
    )?;
    edge.seed_route(granted_route.clone()).await;
    edge.seed_route(denied_route.clone()).await;

    let organization_wide = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/routes/{}",
                denied_route.id
            ),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(organization_wide.status(), 200);

    let resource_grants_path =
        format!("/api/v1/organizations/{organization}/memberships/{membership_id}/resource-grants");
    let granted = app
        .call(post_json(
            &resource_grants_path,
            "route-grants-create",
            json!({
                "scope": {
                    "kind": "environment",
                    "projectId": granted_project,
                    "environmentId": granted_environment
                }
            }),
        ))
        .await?;
    assert_eq!(granted.status(), 201);
    let granted_resource_id = response_json(&granted)?["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("Route Resource Grant has no ID".into()))?
        .to_owned();

    let granted_detail = format!(
        "/api/v1/organizations/{organization}/routes/{}",
        granted_route.id
    );
    let allowed = app
        .call(get_as(&granted_detail, RESTRICTED_ROUTE_TOKEN))
        .await?;
    assert_eq!(allowed.status(), 200);
    let allowed_mcp = app
        .call(route_mcp_get(1, granted_route.id, RESTRICTED_ROUTE_TOKEN))
        .await?;
    let allowed_mcp = response_json(&allowed_mcp)?;
    assert_eq!(allowed_mcp["result"]["isError"], false);
    assert_eq!(
        allowed_mcp["result"]["structuredContent"]["data"]["id"],
        granted_route.id.to_string()
    );
    let allowed_list = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{granted_project}/environments/{granted_environment}/routes"
            ),
            RESTRICTED_ROUTE_TOKEN,
        ))
        .await?;
    assert_eq!(allowed_list.status(), 200);

    let denied_list = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{denied_project}/environments/{denied_environment}/routes"
            ),
            RESTRICTED_ROUTE_TOKEN,
        ))
        .await?;
    assert_eq!(denied_list.status(), 403);
    let denied_detail = format!(
        "/api/v1/organizations/{organization}/routes/{}",
        denied_route.id
    );
    assert_resource_not_found_equivalent(
        &app,
        get_as(&denied_detail, RESTRICTED_ROUTE_TOKEN),
        get_as(
            format!(
                "/api/v1/organizations/{organization}/routes/{}",
                RouteId::new()
            ),
            RESTRICTED_ROUTE_TOKEN,
        ),
    )
    .await?;
    let denied_mcp = app
        .call(route_mcp_get(2, denied_route.id, RESTRICTED_ROUTE_TOKEN))
        .await?;
    let missing_mcp = app
        .call(route_mcp_get(3, RouteId::new(), RESTRICTED_ROUTE_TOKEN))
        .await?;
    let denied_mcp = response_json(&denied_mcp)?;
    let missing_mcp = response_json(&missing_mcp)?;
    assert_eq!(denied_mcp["result"]["isError"], true);
    assert_eq!(missing_mcp["result"]["isError"], true);
    assert_eq!(denied_mcp["result"]["structuredContent"]["code"], 404);
    assert_eq!(missing_mcp["result"]["structuredContent"]["code"], 404);
    for field in ["code", "statusCode", "message", "details"] {
        assert_eq!(
            denied_mcp["result"]["structuredContent"][field],
            missing_mcp["result"]["structuredContent"][field]
        );
    }

    let fallback_grant = app
        .call(post_json(
            &resource_grants_path,
            "route-grants-fallback",
            json!({
                "scope": {"kind": "project", "projectId": denied_project}
            }),
        ))
        .await?;
    assert_eq!(fallback_grant.status(), 201);
    let project_grant_allows_descendant_route = app
        .call(get_as(&denied_detail, RESTRICTED_ROUTE_TOKEN))
        .await?;
    assert_eq!(project_grant_allows_descendant_route.status(), 200);
    let revoked = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/resource-grants/{granted_resource_id}/revocation"
            ),
            "route-grants-revoke",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(revoked.status(), 200);
    assert_resource_not_found_equivalent(
        &app,
        get_as(&granted_detail, RESTRICTED_ROUTE_TOKEN),
        get_as(
            format!(
                "/api/v1/organizations/{organization}/routes/{}",
                RouteId::new()
            ),
            RESTRICTED_ROUTE_TOKEN,
        ),
    )
    .await?;
    let revoked_mcp = app
        .call(route_mcp_get(4, granted_route.id, RESTRICTED_ROUTE_TOKEN))
        .await?;
    assert_eq!(
        response_json(&revoked_mcp)?["result"]["structuredContent"]["code"],
        404
    );
    Ok(())
}

fn route_mcp_get(id: u64, route_id: RouteId, token: &str) -> BootRequest {
    let body = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": "a3s_cloud_routes_get",
            "arguments": {"routeId": route_id},
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": a3s_cloud_contracts::MCP_PROTOCOL_VERSION,
                "io.modelcontextprotocol/clientInfo": {
                    "name": "a3s-cloud-route-test",
                    "version": "1.0.0"
                },
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }
    });
    BootRequest::new(HttpMethod::Post, "/api/v1/mcp")
        .with_header("content-type", "application/json")
        .with_header("accept", "application/json, text/event-stream")
        .with_header("authorization", format!("Bearer {token}"))
        .with_header(
            "mcp-protocol-version",
            a3s_cloud_contracts::MCP_PROTOCOL_VERSION,
        )
        .with_header("mcp-method", "tools/call")
        .with_header("mcp-name", "a3s_cloud_routes_get")
        .with_body(body.to_string().into_bytes())
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

fn route_fixture(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    hostname: &str,
    port: u16,
) -> Result<Route> {
    let created_at = Utc::now();
    let workload_id = WorkloadId::new();
    let workload_revision_id = WorkloadRevisionId::new();
    let target = RouteTarget::new(
        workload_id,
        workload_revision_id,
        format!("workload:{workload_id}:revision:{workload_revision_id}"),
        1,
        RoutePortName::parse("http").map_err(BootError::Internal)?,
        UpstreamEndpoint::parse(format!("http://127.0.0.1:{port}")).map_err(BootError::Internal)?,
        created_at,
    )
    .map_err(BootError::Internal)?;
    Route::create(
        RouteId::new(),
        organization_id,
        project_id,
        environment_id,
        GatewayScopeId::new(),
        NodeId::new(),
        RouteHostname::parse(hostname).map_err(BootError::Internal)?,
        RoutePath::parse("/").map_err(BootError::Internal)?,
        DomainClaimId::new(),
        DomainNamePattern::parse(hostname).map_err(BootError::Internal)?,
        GatewayCertificateId::new(),
        workload_id,
        target,
        created_at,
    )
    .map_err(BootError::Internal)
}
