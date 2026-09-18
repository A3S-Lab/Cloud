use super::*;
use crate::modules::operations::{
    IOperationRepository, OperationRequest, OperationSubject, WorkflowIdentity,
};
use crate::modules::shared_kernel::domain::OperationId;

const RESTRICTED_OPERATION_TOKEN: &str =
    "a3s_f444444444444444444444444444444444444444444444444444444444444444";

#[tokio::test]
async fn restricted_operation_feed_filters_rest_mcp_and_stream_snapshots_through_subject_owners(
) -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let executions = Arc::new(InMemoryExecutionRepository::new());
    let operations = Arc::new(InMemoryOperationRepository::new());
    let app = build_test_application_with_execution_and_operation_repositories(
        identity,
        projects,
        executions,
        operations.clone(),
    )?;
    let organization = bootstrap_organization(&app, "operation-grants", "Operation grants").await?;

    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "operation-grants-membership",
            json!({"name": "Restricted operation reader", "role": "restricted"}),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let membership = response_json(&membership)?;
    let membership_id = required_operation_string(
        &membership["data"]["id"],
        "restricted operation membership ID",
    )?;
    let principal_id = required_operation_string(
        &membership["data"]["principalId"],
        "restricted operation principal ID",
    )?;
    let token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "operation-grants-token",
            json!({
                "name": "Restricted operation reader",
                "token": RESTRICTED_OPERATION_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ],
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(token.status(), 201);

    let granted_project =
        create_project(&app, &organization, "operation-granted-project", "Granted").await?;
    let denied_project =
        create_project(&app, &organization, "operation-denied-project", "Denied").await?;
    let granted_environment = create_operation_environment(
        &app,
        &organization,
        &granted_project,
        "operation-granted-environment",
        "Granted",
    )
    .await?;
    let denied_environment = create_operation_environment(
        &app,
        &organization,
        &denied_project,
        "operation-denied-environment",
        "Denied",
    )
    .await?;
    let granted_execution = create_operation_execution(
        &app,
        &organization,
        &granted_project,
        &granted_environment,
        "operation-granted-execution",
    )
    .await?;
    let denied_execution = create_operation_execution(
        &app,
        &organization,
        &denied_project,
        &denied_environment,
        "operation-denied-execution",
    )
    .await?;

    let organization_id = OrganizationId::from_uuid(parse_operation_uuid(
        &organization,
        "operation organization ID",
    )?);
    let now = Utc::now();
    for (offset, execution_id) in [granted_execution, denied_execution]
        .into_iter()
        .enumerate()
    {
        operations
            .enqueue(OperationRequest::new(
                OperationId::from_uuid(execution_id),
                organization_id,
                OperationSubject::new("execution", execution_id).map_err(BootError::Internal)?,
                WorkflowIdentity::new("cloud.execution", "1").map_err(BootError::Internal)?,
                json!({}),
                now + chrono::Duration::seconds(offset as i64),
            ))
            .await
            .map_err(|error| BootError::Internal(error.to_string()))?;
    }
    let unknown_subject = Uuid::now_v7();
    operations
        .enqueue(OperationRequest::new(
            OperationId::new(),
            organization_id,
            OperationSubject::new("future_subject", unknown_subject)
                .map_err(BootError::Internal)?,
            WorkflowIdentity::new("cloud.future", "1").map_err(BootError::Internal)?,
            json!({"projectId": granted_project, "environmentId": granted_environment}),
            now + chrono::Duration::seconds(2),
        ))
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;

    let resource_grants =
        format!("/api/v1/organizations/{organization}/memberships/{membership_id}/resource-grants");
    let grant = app
        .call(post_json(
            resource_grants,
            "operation-grants-environment",
            json!({
                "scope": {
                    "kind": "environment",
                    "projectId": granted_project,
                    "environmentId": granted_environment
                }
            }),
        ))
        .await?;
    assert_eq!(grant.status(), 201);

    let operations_path = format!("/api/v1/organizations/{organization}/operations");
    let restricted = app
        .call(get_as(&operations_path, RESTRICTED_OPERATION_TOKEN))
        .await?;
    assert_eq!(restricted.status(), 200);
    let restricted = response_json(&restricted)?;
    assert_eq!(restricted["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        restricted["data"][0]["subjectId"],
        granted_execution.to_string()
    );

    let admin = app.call(get_as(&operations_path, ADMIN_TOKEN)).await?;
    assert_eq!(admin.status(), 200);
    assert_eq!(
        response_json(&admin)?["data"].as_array().map(Vec::len),
        Some(3)
    );

    let mcp = app
        .call(operation_mcp_request(
            RESTRICTED_OPERATION_TOKEN,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "a3s_cloud_operations_list",
                    "arguments": {"limit": 50}
                }
            }),
        ))
        .await?;
    assert_eq!(mcp.status(), 200);
    let mcp = response_json(&mcp)?;
    assert_eq!(mcp["result"]["isError"], false);
    assert_eq!(
        mcp["result"]["structuredContent"]["data"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        mcp["result"]["structuredContent"]["data"][0]["subjectId"],
        granted_execution.to_string()
    );

    let stream = app
        .call(
            get_as(
                format!("{operations_path}/stream"),
                RESTRICTED_OPERATION_TOKEN,
            )
            .with_header("accept", "text/event-stream"),
        )
        .await?;
    assert_eq!(stream.status(), 200);
    assert!(stream.is_streaming());
    assert!(stream.is_event_stream());
    Ok(())
}

async fn create_operation_environment(
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

async fn create_operation_execution(
    app: &BootApplication,
    organization: &str,
    project: &str,
    environment: &str,
    idempotency_key: &str,
) -> Result<Uuid> {
    let digest = format!("sha256:{}", "a".repeat(64));
    let response = app
        .call(post_json_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/executions"
            ),
            idempotency_key,
            json!({
                "artifact": {
                    "uri": format!("oci://registry.example/functions/operation@{digest}"),
                    "digest": digest,
                    "mediaType": "application/vnd.oci.image.manifest.v1+json"
                },
                "process": {
                    "command": ["/app/operation"],
                    "args": [],
                    "workingDirectory": null,
                    "environment": {}
                },
                "input": {},
                "resources": {
                    "cpuMillis": 250,
                    "memoryBytes": 134217728,
                    "pids": 64,
                    "ephemeralStorageBytes": null,
                    "timeoutMs": 5000
                }
            }),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(response.status(), 202);
    let id = required_operation_string(
        &response_json(&response)?["data"]["execution"]["id"],
        "operation execution ID",
    )?;
    parse_operation_uuid(&id, "operation execution ID")
}

fn operation_mcp_request(token: &str, mut body: Value) -> BootRequest {
    let version = a3s_cloud_contracts::MCP_PROTOCOL_VERSION;
    body["params"]["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": version,
        "io.modelcontextprotocol/clientInfo": {
            "name": "operation-authorization-test",
            "version": "1.0.0"
        },
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    BootRequest::new(HttpMethod::Post, "/api/v1/mcp")
        .with_header("content-type", "application/json")
        .with_header("accept", "application/json, text/event-stream")
        .with_header("authorization", format!("Bearer {token}"))
        .with_header("mcp-protocol-version", version)
        .with_header("mcp-method", "tools/call")
        .with_header("mcp-name", "a3s_cloud_operations_list")
        .with_body(body.to_string().into_bytes())
}

fn required_operation_string(value: &Value, label: &str) -> Result<String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| BootError::Internal(format!("{label} is missing")))
}

fn parse_operation_uuid(value: &str, label: &str) -> Result<Uuid> {
    Uuid::parse_str(value)
        .map_err(|error| BootError::Internal(format!("{label} is invalid: {error}")))
}
