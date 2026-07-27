use super::*;
use crate::modules::search::{SearchResourceKind, SearchResult};
use crate::modules::shared_kernel::domain::OrganizationId;

const MCP_PATH: &str = "/api/v1/mcp";
const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

#[tokio::test]
async fn management_mcp_initializes_as_raw_stateless_json_rpc() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    bootstrap_organization(&app, "mcp-bootstrap", "Acme").await?;

    let unauthenticated = app.call(mcp_request(None, initialize_request(1))).await?;
    assert_eq!(unauthenticated.status(), 401);

    let initialized = app
        .call(mcp_request(Some(ADMIN_TOKEN), initialize_request(2)))
        .await?;
    assert_eq!(initialized.status(), 200);
    assert_eq!(initialized.header("content-type"), Some("application/json"));
    assert_eq!(
        initialized.header("mcp-protocol-version"),
        Some(MCP_PROTOCOL_VERSION)
    );
    assert_eq!(initialized.header("cache-control"), Some("no-store"));
    assert!(initialized.header("mcp-session-id").is_none());

    let body = response_json(&initialized)?;
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["id"], 2);
    assert_eq!(body["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
    assert_eq!(body["result"]["serverInfo"]["name"], "a3s-cloud");
    assert_eq!(
        body["result"]["capabilities"]["tools"]["listChanged"],
        false
    );
    assert!(body.get("data").is_none());

    let notification = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            }),
        ))
        .await?;
    assert_eq!(notification.status(), 202);
    assert!(notification.body().is_empty());

    let event_stream = app
        .call(
            BootRequest::new(HttpMethod::Get, MCP_PATH)
                .with_header("accept", "text/event-stream")
                .with_header("authorization", format!("Bearer {ADMIN_TOKEN}")),
        )
        .await?;
    assert_eq!(event_stream.status(), 405);
    assert_eq!(event_stream.header("allow"), Some("POST"));
    Ok(())
}

#[tokio::test]
async fn management_mcp_rejects_batches_and_unnegotiated_requests() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    bootstrap_organization(&app, "mcp-protocol", "Acme").await?;

    let batch = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            json!([initialize_request(1), initialize_request(2)]),
        ))
        .await?;
    assert_eq!(batch.status(), 400);
    assert_eq!(response_json(&batch)?["error"]["code"], -32600);

    let missing_version = app
        .call(mcp_request_without_version(
            Some(ADMIN_TOKEN),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}),
        ))
        .await?;
    assert_eq!(missing_version.status(), 400);
    assert_eq!(response_json(&missing_version)?["error"]["code"], -32600);

    let wrong_version = app
        .call(
            mcp_request(
                Some(ADMIN_TOKEN),
                json!({"jsonrpc": "2.0", "id": 4, "method": "tools/list"}),
            )
            .with_header("mcp-protocol-version", "2025-03-26"),
        )
        .await?;
    assert_eq!(wrong_version.status(), 400);
    assert_eq!(response_json(&wrong_version)?["error"]["code"], -32600);

    let foreign_origin = app
        .call(
            mcp_request(Some(ADMIN_TOKEN), initialize_request(5))
                .with_header("host", "cloud.example.test")
                .with_header("origin", "https://attacker.example.test"),
        )
        .await?;
    assert_eq!(foreign_origin.status(), 403);
    assert_eq!(response_json(&foreign_origin)?["error"]["code"], -32600);
    Ok(())
}

#[tokio::test]
async fn management_mcp_hides_and_denies_mutations_without_effective_scope() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "mcp-scopes", "Acme").await?;
    create_api_token(
        &app,
        &organization,
        "mcp-token-manager",
        "MCP token manager",
        TOKEN_MANAGER_TOKEN,
        &[ApiTokenScope::TOKEN_WRITE],
        None,
    )
    .await?;
    let read_only = app
        .call(post_json_as(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "mcp-read-only",
            json!({
                "name": "MCP read only",
                "token": EXPIRING_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ],
                "expiresAt": null
            }),
            TOKEN_MANAGER_TOKEN,
        ))
        .await?;
    assert_eq!(read_only.status(), 201);
    create_api_token(
        &app,
        &organization,
        "mcp-project-writer",
        "MCP project writer",
        PROJECT_TOKEN,
        &[ApiTokenScope::PROJECT_WRITE],
        None,
    )
    .await?;

    let read_only_tools = list_tools(&app, EXPIRING_TOKEN, 1).await?;
    assert_eq!(
        tool_names(&read_only_tools),
        vec![
            "a3s_cloud_environments_list",
            "a3s_cloud_projects_list",
            "a3s_cloud_search",
            "a3s_cloud_nodes_list",
            "a3s_cloud_nodes_get",
            "a3s_cloud_operations_list",
            "a3s_cloud_workloads_list",
            "a3s_cloud_workloads_get",
            "a3s_cloud_workload_logs_get",
            "a3s_cloud_deployments_get",
            "a3s_cloud_routes_list",
            "a3s_cloud_routes_get",
            "a3s_cloud_build_runs_list",
            "a3s_cloud_build_runs_get",
            "a3s_cloud_build_run_logs_get",
            "a3s_cloud_build_evidence_get",
        ]
    );
    assert!(read_only_tools["result"]["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .all(|tool| tool["annotations"]["readOnlyHint"] == true));

    let project_writer_tools = list_tools(&app, PROJECT_TOKEN, 2).await?;
    assert!(tool_names(&project_writer_tools).contains(&"a3s_cloud_projects_create"));
    assert!(!tool_names(&project_writer_tools).contains(&"a3s_cloud_environments_create"));

    let administrator_tools = list_tools(&app, ADMIN_TOKEN, 4).await?;
    assert_eq!(
        tool_names(&administrator_tools),
        vec![
            "a3s_cloud_environments_create",
            "a3s_cloud_environments_list",
            "a3s_cloud_projects_create",
            "a3s_cloud_projects_list",
            "a3s_cloud_search",
            "a3s_cloud_nodes_list",
            "a3s_cloud_nodes_get",
            "a3s_cloud_operations_list",
            "a3s_cloud_workloads_list",
            "a3s_cloud_workloads_get",
            "a3s_cloud_workload_logs_get",
            "a3s_cloud_deployments_get",
            "a3s_cloud_routes_list",
            "a3s_cloud_routes_get",
            "a3s_cloud_build_runs_list",
            "a3s_cloud_build_runs_get",
            "a3s_cloud_build_run_logs_get",
            "a3s_cloud_build_evidence_get",
        ]
    );

    let hidden_call = app
        .call(mcp_request(
            Some(EXPIRING_TOKEN),
            tool_call(
                3,
                "a3s_cloud_projects_create",
                json!({"name": "Hidden", "idempotencyKey": "mcp-hidden"}),
            ),
        ))
        .await?;
    assert_eq!(hidden_call.status(), 200);
    assert_eq!(response_json(&hidden_call)?["error"]["code"], -32602);
    Ok(())
}

#[tokio::test]
async fn management_mcp_reuses_project_commands_queries_and_idempotency() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "mcp-parity", "Acme").await?;

    let rest = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects"),
            "cross-surface-project",
            json!({"name": "Cloud"}),
        ))
        .await?;
    assert_eq!(rest.status(), 201);
    let rest_body = response_json(&rest)?;
    let project_id = rest_body["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("REST project response has no ID".into()))?;

    let replay = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                10,
                "a3s_cloud_projects_create",
                json!({
                    "name": "Cloud",
                    "idempotencyKey": "cross-surface-project"
                }),
            ),
        ))
        .await?;
    assert_eq!(replay.status(), 200);
    let replay_body = response_json(&replay)?;
    assert_eq!(
        replay_body["result"]["structuredContent"]["data"]["id"],
        project_id
    );
    assert_eq!(
        replay_body["result"]["structuredContent"]["data"]["replayed"],
        true
    );
    assert_eq!(replay_body["result"]["structuredContent"]["code"], 200);

    let listed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(11, "a3s_cloud_projects_list", json!({})),
        ))
        .await?;
    let listed_body = response_json(&listed)?;
    assert_eq!(
        listed_body["result"]["structuredContent"]["data"][0]["id"],
        project_id
    );
    assert_eq!(
        listed_body["result"]["structuredContent"]["data"][0]["name"],
        "Cloud"
    );

    let environment = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                12,
                "a3s_cloud_environments_create",
                json!({
                    "projectId": project_id,
                    "name": "Production",
                    "idempotencyKey": "mcp-environment"
                }),
            ),
        ))
        .await?;
    let environment_body = response_json(&environment)?;
    assert_eq!(environment_body["result"]["structuredContent"]["code"], 201);
    let environment_id = environment_body["result"]["structuredContent"]["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP environment response has no ID".into()))?;

    let environments = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                13,
                "a3s_cloud_environments_list",
                json!({"projectId": project_id}),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&environments)?["result"]["structuredContent"]["data"][0]["id"],
        environment_id
    );

    let forged_tenant = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                14,
                "a3s_cloud_projects_list",
                json!({"organizationId": Uuid::new_v4()}),
            ),
        ))
        .await?;
    assert_eq!(response_json(&forged_tenant)?["error"]["code"], -32602);

    let foreign_organization = create_organization(&app, "mcp-foreign", "Foreign").await?;
    let foreign_project = create_project(
        &app,
        &foreign_organization,
        "mcp-foreign-project",
        "Foreign",
    )
    .await?;
    let foreign = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                15,
                "a3s_cloud_environments_list",
                json!({"projectId": foreign_project}),
            ),
        ))
        .await?;
    let missing = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                16,
                "a3s_cloud_environments_list",
                json!({"projectId": Uuid::new_v4()}),
            ),
        ))
        .await?;
    let foreign_error = response_json(&foreign)?;
    let missing_error = response_json(&missing)?;
    assert_eq!(foreign_error["result"]["isError"], true);
    assert_eq!(missing_error["result"]["isError"], true);
    assert_eq!(foreign_error["result"]["structuredContent"]["code"], 404);
    assert_eq!(
        foreign_error["result"]["structuredContent"]["statusCode"],
        "NOT_FOUND"
    );
    for field in ["code", "statusCode", "message", "details"] {
        assert_eq!(
            foreign_error["result"]["structuredContent"][field],
            missing_error["result"]["structuredContent"][field]
        );
    }
    Ok(())
}

#[tokio::test]
async fn management_mcp_reuses_operational_queries_with_strict_arguments() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "mcp-operations", "Acme").await?;
    let project =
        create_project(&app, &organization, "mcp-operations-project", "Operations").await?;
    let environment = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            "mcp-operations-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment = response_id(&environment)?;

    for (id, name, arguments) in [
        (1, "a3s_cloud_nodes_list", json!({})),
        (2, "a3s_cloud_operations_list", json!({})),
        (
            3,
            "a3s_cloud_workloads_list",
            json!({"projectId": project, "environmentId": environment}),
        ),
        (
            4,
            "a3s_cloud_routes_list",
            json!({"projectId": project, "environmentId": environment}),
        ),
        (
            5,
            "a3s_cloud_build_runs_list",
            json!({"projectId": project, "environmentId": environment}),
        ),
    ] {
        let response = app
            .call(mcp_request(
                Some(ADMIN_TOKEN),
                tool_call(id, name, arguments),
            ))
            .await?;
        let body = response_json(&response)?;
        assert_eq!(body["result"]["isError"], false, "{name}");
        assert_eq!(
            body["result"]["structuredContent"]["data"],
            json!([]),
            "{name}"
        );
    }

    let created_workload = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/workloads"
            ),
            "mcp-observability-workload",
            json!({
                "name": "observability",
                "template": management_mcp_workload_template()
            }),
        ))
        .await?;
    assert_eq!(created_workload.status(), 202);
    let created_workload = response_json(&created_workload)?;
    let workload_id = created_workload["data"]["workloadId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("created workload has no workload ID".into()))?;
    let revision_id = created_workload["data"]["revisionId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("created workload has no revision ID".into()))?;
    let workload_logs = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                23,
                "a3s_cloud_workload_logs_get",
                json!({
                    "workloadId": workload_id,
                    "revisionId": revision_id,
                    "cursor": "v1:0",
                    "limit": 1,
                    "stream": "stdout"
                }),
            ),
        ))
        .await?;
    let workload_logs = response_json(&workload_logs)?;
    assert_eq!(workload_logs["result"]["isError"], false);
    let workload_logs = &workload_logs["result"]["structuredContent"]["data"];
    assert_eq!(workload_logs["workloadId"], workload_id);
    assert_eq!(workload_logs["revisionId"], revision_id);
    assert_eq!(workload_logs["records"], json!([]));
    assert!(workload_logs["nextCursor"].is_null());

    let missing_resource_id = Uuid::new_v4();
    for (id, name, arguments) in [
        (
            6,
            "a3s_cloud_nodes_get",
            json!({"nodeId": missing_resource_id}),
        ),
        (
            7,
            "a3s_cloud_workloads_get",
            json!({"workloadId": missing_resource_id}),
        ),
        (
            8,
            "a3s_cloud_deployments_get",
            json!({"deploymentId": missing_resource_id}),
        ),
        (
            9,
            "a3s_cloud_routes_get",
            json!({"routeId": missing_resource_id}),
        ),
        (
            10,
            "a3s_cloud_build_runs_get",
            json!({"buildRunId": missing_resource_id}),
        ),
        (
            11,
            "a3s_cloud_workload_logs_get",
            json!({
                "workloadId": missing_resource_id,
                "revisionId": missing_resource_id
            }),
        ),
        (
            12,
            "a3s_cloud_build_run_logs_get",
            json!({"buildRunId": missing_resource_id}),
        ),
        (
            13,
            "a3s_cloud_build_evidence_get",
            json!({"buildRunId": missing_resource_id}),
        ),
    ] {
        let response = app
            .call(mcp_request(
                Some(ADMIN_TOKEN),
                tool_call(id, name, arguments),
            ))
            .await?;
        let body = response_json(&response)?;
        let structured = &body["result"]["structuredContent"];
        assert_eq!(body["result"]["isError"], true, "{name}");
        assert_eq!(structured["code"], 404, "{name}");
        assert_eq!(structured["statusCode"], "NOT_FOUND", "{name}");
        assert!(structured["details"].is_object(), "{name}");
        assert!(structured["requestId"].is_string(), "{name}");
        assert!(structured["timestamp"].is_string(), "{name}");
    }

    for (id, name, arguments) in [
        (14, "a3s_cloud_operations_list", json!({"limit": 0})),
        (15, "a3s_cloud_operations_list", json!({"limit": 201})),
        (
            16,
            "a3s_cloud_build_runs_list",
            json!({"projectId": project, "environmentId": environment, "limit": 0}),
        ),
        (
            17,
            "a3s_cloud_build_runs_list",
            json!({"projectId": project, "environmentId": environment, "limit": 201}),
        ),
        (
            18,
            "a3s_cloud_nodes_list",
            json!({"organizationId": organization}),
        ),
        (
            19,
            "a3s_cloud_workload_logs_get",
            json!({
                "workloadId": missing_resource_id,
                "revisionId": missing_resource_id,
                "limit": 0
            }),
        ),
        (
            20,
            "a3s_cloud_build_run_logs_get",
            json!({"buildRunId": missing_resource_id, "limit": 257}),
        ),
        (
            21,
            "a3s_cloud_build_run_logs_get",
            json!({"buildRunId": missing_resource_id, "cursor": "1"}),
        ),
        (
            22,
            "a3s_cloud_workload_logs_get",
            json!({
                "workloadId": missing_resource_id,
                "revisionId": missing_resource_id,
                "stream": "combined"
            }),
        ),
    ] {
        let response = app
            .call(mcp_request(
                Some(ADMIN_TOKEN),
                tool_call(id, name, arguments),
            ))
            .await?;
        assert_eq!(response_json(&response)?["error"]["code"], -32602, "{name}");
    }
    Ok(())
}

#[tokio::test]
async fn management_mcp_search_uses_the_tenant_authorized_query() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let search = Arc::new(InMemorySearchRepository::new());
    let app = build_test_application_with_search(identity, projects, Arc::clone(&search))?;
    let organization = bootstrap_organization(&app, "mcp-search", "Acme").await?;
    let foreign_organization = create_organization(&app, "mcp-search-foreign", "Foreign").await?;
    let allowed_id = Uuid::new_v4();
    let denied_id = Uuid::new_v4();
    search
        .register(SearchResult {
            organization_id: parse_organization_id(&organization)?,
            project_id: None,
            environment_id: None,
            workload_id: None,
            kind: SearchResourceKind::Node,
            id: allowed_id,
            title: "Cloud worker".into(),
            description: "Node · ready".into(),
            state: Some("ready".into()),
            updated_at: Utc::now(),
        })
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;
    search
        .register(SearchResult {
            organization_id: parse_organization_id(&foreign_organization)?,
            project_id: None,
            environment_id: None,
            workload_id: None,
            kind: SearchResourceKind::Node,
            id: denied_id,
            title: "Cloud hidden worker".into(),
            description: "Node · ready".into(),
            state: Some("ready".into()),
            updated_at: Utc::now(),
        })
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;

    let response = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                1,
                "a3s_cloud_search",
                json!({"query": "cloud", "limit": 20}),
            ),
        ))
        .await?;
    let body = response_json(&response)?;
    assert_eq!(
        body["result"]["structuredContent"]["data"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        body["result"]["structuredContent"]["data"][0]["id"],
        allowed_id.to_string()
    );
    assert!(!body.to_string().contains(&denied_id.to_string()));
    assert!(!body.to_string().contains("Cloud hidden worker"));
    Ok(())
}

#[tokio::test]
async fn management_mcp_observes_api_token_revocation_on_the_next_request() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "mcp-revocation", "Acme").await?;
    let token_id = create_api_token(
        &app,
        &organization,
        "mcp-revocable",
        "MCP revocable",
        EXPIRING_TOKEN,
        &[ApiTokenScope::CLOUD_READ],
        None,
    )
    .await?;

    assert_eq!(list_tools(&app, EXPIRING_TOKEN, 1).await?["jsonrpc"], "2.0");
    let revoked = app
        .call(delete_as(
            format!("/api/v1/organizations/{organization}/api-tokens/{token_id}"),
            "revoke-mcp-token",
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(revoked.status(), 200);

    let denied = app
        .call(mcp_request(
            Some(EXPIRING_TOKEN),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        ))
        .await?;
    assert_eq!(denied.status(), 401);
    assert_eq!(response_json(&denied)?["statusCode"], "UNAUTHORIZED");
    Ok(())
}

fn initialize_request(id: u64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "a3s-cloud-test", "version": "1.0.0"}
        }
    })
}

fn mcp_request(token: Option<&str>, body: Value) -> BootRequest {
    mcp_request_without_version(token, body)
        .with_header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
}

fn mcp_request_without_version(token: Option<&str>, body: Value) -> BootRequest {
    let request = BootRequest::new(HttpMethod::Post, MCP_PATH)
        .with_header("content-type", "application/json")
        .with_header("accept", "application/json, text/event-stream")
        .with_body(body.to_string().into_bytes());
    match token {
        Some(token) => request.with_header("authorization", format!("Bearer {token}")),
        None => request,
    }
}

fn tool_call(id: u64, name: &str, arguments: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": name, "arguments": arguments}
    })
}

async fn list_tools(app: &BootApplication, token: &str, id: u64) -> Result<Value> {
    let response = app
        .call(mcp_request(
            Some(token),
            json!({"jsonrpc": "2.0", "id": id, "method": "tools/list"}),
        ))
        .await?;
    assert_eq!(response.status(), 200);
    response_json(&response)
}

fn tool_names(body: &Value) -> Vec<&str> {
    body["result"]["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| tool["name"].as_str())
        .collect()
}

fn parse_organization_id(value: &str) -> Result<OrganizationId> {
    Uuid::parse_str(value)
        .map(OrganizationId::from_uuid)
        .map_err(|error| BootError::Internal(format!("invalid test organization ID: {error}")))
}

fn management_mcp_workload_template() -> Value {
    json!({
        "artifact": {
            "uri": "oci://registry.example/cloud/observability:v1",
            "expectedDigest": null
        },
        "process": {},
        "secrets": [],
        "resources": {
            "cpuMillis": 100,
            "memoryBytes": 33554432,
            "pids": 32,
            "ephemeralStorageBytes": null
        },
        "ports": [{"name": "http", "containerPort": 8080}],
        "health": {
            "portName": "http",
            "path": "/health",
            "intervalMs": 1000,
            "timeoutMs": 500,
            "healthyThreshold": 1,
            "unhealthyThreshold": 3,
            "stabilizationWindowMs": 1000
        }
    })
}
