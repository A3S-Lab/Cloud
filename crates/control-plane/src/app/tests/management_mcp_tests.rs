use super::*;
use crate::modules::search::{SearchResourceKind, SearchResult};
use crate::modules::shared_kernel::domain::{
    OntologyId, OntologyRevisionId, OrganizationId, Sha256Digest, WorkflowDefinitionId,
    WorkflowRevisionId,
};
use crate::modules::workflow::{WorkflowGoalContract, WorkflowGoalSpec};
use a3s_use_extension::{
    plugin_catalog_host_input_schema, plugin_catalog_inspection_input_schema,
    plugin_catalog_search_input_schema,
};

const MCP_PATH: &str = "/api/v1/mcp";
const MCP_PROTOCOL_VERSION: &str = a3s_cloud_contracts::MCP_PROTOCOL_VERSION;
const MCP_WORKLOAD_TOKEN: &str =
    "a3s_1111111111111111111111111111111111111111111111111111111111111111";
const MCP_BUILD_TOKEN: &str =
    "a3s_2222222222222222222222222222222222222222222222222222222222222222";
const MCP_FORM_TOKEN: &str = "a3s_3333333333333333333333333333333333333333333333333333333333333333";
const MCP_ROUTE_TOKEN: &str =
    "a3s_6666666666666666666666666666666666666666666666666666666666666666";
const MCP_FORM_MEMBER_TOKEN: &str =
    "a3s_4444444444444444444444444444444444444444444444444444444444444444";
const MCP_INVITEE_TOKEN: &str =
    "a3s_5555555555555555555555555555555555555555555555555555555555555555";
const MCP_ONTOLOGY_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/w0.1/ontology.acl"
));

#[tokio::test]
async fn management_mcp_discovers_modern_stateless_json_rpc() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    bootstrap_organization(&app, "mcp-bootstrap", "Acme").await?;

    let unauthenticated = app.call(mcp_request(None, discover_request(1))).await?;
    assert_eq!(unauthenticated.status(), 401);

    let discovered = app
        .call(mcp_request(Some(ADMIN_TOKEN), discover_request(2)))
        .await?;
    assert_eq!(discovered.status(), 200);
    assert_eq!(discovered.header("content-type"), Some("application/json"));
    assert_eq!(
        discovered.header("mcp-protocol-version"),
        Some(MCP_PROTOCOL_VERSION)
    );
    assert_eq!(discovered.header("cache-control"), Some("no-store"));
    assert!(discovered.header("mcp-session-id").is_none());

    let body = response_json(&discovered)?;
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["id"], 2);
    assert_eq!(body["result"]["resultType"], "complete");
    assert_eq!(
        body["result"]["supportedVersions"],
        json!([MCP_PROTOCOL_VERSION])
    );
    assert_eq!(body["result"]["capabilities"]["tools"], json!({}));
    assert_eq!(
        body["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "a3s-cloud"
    );
    assert_eq!(body["result"]["ttlMs"], 0);
    assert_eq!(body["result"]["cacheScope"], "private");
    assert!(body.get("data").is_none());

    let initialized = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "legacy-client", "version": "1.0.0"}
                }
            }),
        ))
        .await?;
    assert_eq!(initialized.status(), 404);
    assert_eq!(response_json(&initialized)?["error"]["code"], -32601);

    for method in [HttpMethod::Get, HttpMethod::Delete] {
        let rejected = app
            .call(
                BootRequest::new(method, MCP_PATH)
                    .with_header("accept", "text/event-stream")
                    .with_header("authorization", format!("Bearer {ADMIN_TOKEN}")),
            )
            .await?;
        assert_eq!(rejected.status(), 405);
        assert_eq!(rejected.header("allow"), Some("POST"));
    }
    Ok(())
}

#[tokio::test]
async fn management_mcp_rejects_batches_and_invalid_modern_metadata() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    bootstrap_organization(&app, "mcp-protocol", "Acme").await?;

    let batch = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            json!([discover_request(1), discover_request(2)]),
        ))
        .await?;
    assert_eq!(batch.status(), 400);
    assert_eq!(response_json(&batch)?["error"]["code"], -32600);

    let tools_list = with_request_metadata(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/list"
    }));
    let missing_version = app
        .call(
            raw_mcp_request(Some(ADMIN_TOKEN), tools_list.clone())
                .with_header("mcp-method", "tools/list"),
        )
        .await?;
    assert_eq!(missing_version.status(), 400);
    assert_eq!(response_json(&missing_version)?["error"]["code"], -32020);

    let mismatched_version = app
        .call(
            raw_mcp_request(Some(ADMIN_TOKEN), tools_list)
                .with_header("mcp-protocol-version", "2025-03-26"),
        )
        .await?;
    assert_eq!(mismatched_version.status(), 400);
    assert_eq!(response_json(&mismatched_version)?["error"]["code"], -32020);

    let unsupported_version = app
        .call(mcp_request_with_version(
            Some(ADMIN_TOKEN),
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/list"}),
            "1900-01-01",
        ))
        .await?;
    assert_eq!(unsupported_version.status(), 400);
    let unsupported = response_json(&unsupported_version)?;
    assert_eq!(unsupported["error"]["code"], -32022);
    assert_eq!(
        unsupported["error"]["data"]["supported"],
        json!([MCP_PROTOCOL_VERSION])
    );
    assert_eq!(unsupported["error"]["data"]["requested"], "1900-01-01");

    let missing_method = app
        .call(
            raw_mcp_request(
                Some(ADMIN_TOKEN),
                with_request_metadata(json!({
                    "jsonrpc": "2.0",
                    "id": 5,
                    "method": "tools/list"
                })),
            )
            .with_header("mcp-protocol-version", MCP_PROTOCOL_VERSION),
        )
        .await?;
    assert_eq!(missing_method.status(), 400);
    assert_eq!(response_json(&missing_method)?["error"]["code"], -32020);

    let mismatched_name = app
        .call(
            mcp_request(
                Some(ADMIN_TOKEN),
                tool_call(6, "a3s_cloud_projects_list", json!({})),
            )
            .with_header("mcp-name", "a3s_cloud_projects_create"),
        )
        .await?;
    assert_eq!(mismatched_name.status(), 400);
    assert_eq!(response_json(&mismatched_name)?["error"]["code"], -32020);

    let optional_client_info = app
        .call(request_with_metadata(
            Some(ADMIN_TOKEN),
            json!({"jsonrpc": "2.0", "id": 7, "method": "tools/list"}),
            json!({
                "io.modelcontextprotocol/protocolVersion": MCP_PROTOCOL_VERSION,
                "io.modelcontextprotocol/clientCapabilities": {}
            }),
            MCP_PROTOCOL_VERSION,
        ))
        .await?;
    assert_eq!(optional_client_info.status(), 200);
    assert_eq!(
        response_json(&optional_client_info)?["result"]["resultType"],
        "complete"
    );

    let missing_client_capabilities = app
        .call(request_with_metadata(
            Some(ADMIN_TOKEN),
            json!({"jsonrpc": "2.0", "id": 8, "method": "tools/list"}),
            json!({
                "io.modelcontextprotocol/protocolVersion": MCP_PROTOCOL_VERSION,
                "io.modelcontextprotocol/clientInfo": {
                    "name": "a3s-cloud-test",
                    "version": "1.0.0"
                }
            }),
            MCP_PROTOCOL_VERSION,
        ))
        .await?;
    assert_eq!(missing_client_capabilities.status(), 400);
    assert_eq!(
        response_json(&missing_client_capabilities)?["error"]["code"],
        -32602
    );

    let missing_body_protocol_version = app
        .call(request_with_metadata(
            Some(ADMIN_TOKEN),
            json!({"jsonrpc": "2.0", "id": 9, "method": "tools/list"}),
            json!({
                "io.modelcontextprotocol/clientCapabilities": {}
            }),
            MCP_PROTOCOL_VERSION,
        ))
        .await?;
    assert_eq!(missing_body_protocol_version.status(), 400);
    assert_eq!(
        response_json(&missing_body_protocol_version)?["error"]["code"],
        -32602
    );

    let ignored_legacy_session = app
        .call(
            mcp_request(
                Some(ADMIN_TOKEN),
                json!({"jsonrpc": "2.0", "id": 10, "method": "tools/list"}),
            )
            .with_header("mcp-session-id", "legacy-session"),
        )
        .await?;
    assert_eq!(ignored_legacy_session.status(), 200);
    assert!(ignored_legacy_session.header("mcp-session-id").is_none());

    let encoded_name = app
        .call(
            mcp_request(
                Some(ADMIN_TOKEN),
                tool_call(11, "a3s_cloud_projects_list", json!({})),
            )
            .with_header("mcp-name", "=?base64?YTNzX2Nsb3VkX3Byb2plY3RzX2xpc3Q=?="),
        )
        .await?;
    assert_eq!(encoded_name.status(), 200);
    assert_eq!(
        response_json(&encoded_name)?["result"]["resultType"],
        "complete"
    );

    let foreign_origin = app
        .call(
            mcp_request(Some(ADMIN_TOKEN), discover_request(12))
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
    create_api_token(
        &app,
        &organization,
        "mcp-workload-writer",
        "MCP workload writer",
        MCP_WORKLOAD_TOKEN,
        &[ApiTokenScope::WORKLOAD_WRITE],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "mcp-build-writer",
        "MCP build writer",
        MCP_BUILD_TOKEN,
        &[ApiTokenScope::BUILD_WRITE],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "mcp-route-writer",
        "MCP route writer",
        MCP_ROUTE_TOKEN,
        &[ApiTokenScope::ROUTE_WRITE],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "mcp-form-writer",
        "MCP Form writer",
        MCP_FORM_TOKEN,
        &[ApiTokenScope::CLOUD_READ, ApiTokenScope::FORM_WRITE],
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
            "a3s_cloud_connector_profiles_list",
            "a3s_cloud_connector_profiles_get",
            "a3s_cloud_connector_revisions_list",
            "a3s_cloud_connector_revisions_get",
            "a3s_cloud_durable_cell_applications_list",
            "a3s_cloud_durable_cell_applications_get",
            "a3s_cloud_durable_cell_revisions_list",
            "a3s_cloud_durable_cell_revisions_get",
            "a3s_cloud_execution_templates_get",
            "a3s_cloud_execution_templates_list",
            "a3s_cloud_my_membership_invitations_list",
            "a3s_cloud_projects_list",
            "a3s_cloud_project_attribution_get",
            "a3s_cloud_forms_get",
            "a3s_cloud_forms_list",
            "a3s_cloud_form_releases_get",
            "a3s_cloud_form_releases_list",
            "a3s_cloud_ontologies_get",
            "a3s_cloud_ontologies_list",
            "a3s_cloud_ontology_revisions_get",
            "a3s_cloud_ontology_revisions_list",
            "a3s_cloud_ontology_revisions_diff",
            "a3s_cloud_workflow_node_catalog_get",
            "a3s_cloud_workflow_definitions_get",
            "a3s_cloud_workflow_definitions_list",
            "a3s_cloud_workflow_revisions_get",
            "a3s_cloud_workflow_revisions_list",
            "a3s_cloud_workflow_goals_get",
            "a3s_cloud_workflow_goals_list",
            "a3s_cloud_workflow_plan_revisions_get",
            "a3s_cloud_workflow_runs_get",
            "a3s_cloud_workflow_runs_list",
            "a3s_cloud_workflow_runs_wait",
            "a3s_cloud_workflow_run_output_get",
            "a3s_cloud_workflow_run_history_get",
            "a3s_cloud_workflow_run_variables_get",
            "a3s_cloud_human_tasks_get",
            "a3s_cloud_human_tasks_list",
            "a3s_cloud_search",
            "a3s_cloud_plugin_registries_list",
            "a3s_cloud_plugin_registries_get",
            "a3s_cloud_plugin_catalog_search",
            "a3s_cloud_plugin_catalog_search_cached",
            "a3s_cloud_plugin_catalog_inspect",
            "a3s_cloud_plugin_catalog_inspect_cached",
            "a3s_cloud_nodes_list",
            "a3s_cloud_nodes_get",
            "a3s_cloud_operations_list",
            "a3s_cloud_audit_records_list",
            "a3s_cloud_notifications_list",
            "a3s_cloud_notifications_get",
            "a3s_cloud_notification_outbound_subscriptions_list",
            "a3s_cloud_notification_outbound_subscriptions_get",
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
    assert!(tool_names(&project_writer_tools).contains(&"a3s_cloud_project_attribution_update"));
    assert!(!tool_names(&project_writer_tools).contains(&"a3s_cloud_environments_create"));

    let workload_writer_tools = list_tools(&app, MCP_WORKLOAD_TOKEN, 3).await?;
    for name in [
        "a3s_cloud_workloads_stop",
        "a3s_cloud_workloads_rollback",
        "a3s_cloud_deployments_cancel",
        "a3s_cloud_durable_cell_applications_create",
        "a3s_cloud_durable_cell_applications_revise",
        "a3s_cloud_durable_cell_applications_start",
        "a3s_cloud_durable_cell_applications_stop",
        "a3s_cloud_durable_cell_deployments_create",
    ] {
        assert!(tool_names(&workload_writer_tools).contains(&name), "{name}");
    }
    assert!(!tool_names(&workload_writer_tools).contains(&"a3s_cloud_build_runs_cancel"));
    assert!(!tool_names(&workload_writer_tools).contains(&"a3s_cloud_build_runs_retry"));

    let route_writer_tools = list_tools(&app, MCP_ROUTE_TOKEN, 4).await?;
    assert!(tool_names(&route_writer_tools).contains(&"a3s_cloud_durable_cell_routes_publish"));
    assert!(!tool_names(&route_writer_tools).contains(&"a3s_cloud_durable_cell_deployments_create"));

    let build_writer_tools = list_tools(&app, MCP_BUILD_TOKEN, 5).await?;
    assert!(tool_names(&build_writer_tools).contains(&"a3s_cloud_build_runs_cancel"));
    assert!(tool_names(&build_writer_tools).contains(&"a3s_cloud_build_runs_retry"));
    assert!(!tool_names(&build_writer_tools).contains(&"a3s_cloud_workloads_stop"));

    let form_writer_tools = list_tools(&app, MCP_FORM_TOKEN, 6).await?;
    for name in [
        "a3s_cloud_forms_create",
        "a3s_cloud_forms_revise",
        "a3s_cloud_form_releases_publish",
    ] {
        assert!(tool_names(&form_writer_tools).contains(&name), "{name}");
    }
    assert!(!tool_names(&form_writer_tools).contains(&"a3s_cloud_ontologies_create"));

    let administrator_tools = list_tools(&app, ADMIN_TOKEN, 7).await?;
    assert_eq!(
        tool_names(&administrator_tools),
        vec![
            "a3s_cloud_environments_create",
            "a3s_cloud_environments_list",
            "a3s_cloud_connector_profiles_create",
            "a3s_cloud_connector_profiles_revise",
            "a3s_cloud_connector_profiles_list",
            "a3s_cloud_connector_profiles_get",
            "a3s_cloud_connector_revisions_list",
            "a3s_cloud_connector_revisions_get",
            "a3s_cloud_durable_cell_applications_create",
            "a3s_cloud_durable_cell_applications_revise",
            "a3s_cloud_durable_cell_applications_start",
            "a3s_cloud_durable_cell_applications_stop",
            "a3s_cloud_durable_cell_applications_list",
            "a3s_cloud_durable_cell_applications_get",
            "a3s_cloud_durable_cell_revisions_list",
            "a3s_cloud_durable_cell_revisions_get",
            "a3s_cloud_durable_cell_deployments_create",
            "a3s_cloud_durable_cell_routes_publish",
            "a3s_cloud_execution_templates_create",
            "a3s_cloud_execution_templates_get",
            "a3s_cloud_execution_templates_list",
            "a3s_cloud_memberships_list",
            "a3s_cloud_memberships_get",
            "a3s_cloud_memberships_create",
            "a3s_cloud_memberships_change_role",
            "a3s_cloud_memberships_revoke",
            "a3s_cloud_membership_invitations_list",
            "a3s_cloud_membership_invitations_get",
            "a3s_cloud_membership_invitations_create",
            "a3s_cloud_membership_invitations_revoke",
            "a3s_cloud_my_membership_invitations_list",
            "a3s_cloud_membership_invitations_accept",
            "a3s_cloud_resource_grants_list",
            "a3s_cloud_resource_grants_get",
            "a3s_cloud_resource_grants_create",
            "a3s_cloud_resource_grants_revoke",
            "a3s_cloud_projects_create",
            "a3s_cloud_projects_list",
            "a3s_cloud_project_attribution_get",
            "a3s_cloud_project_attribution_update",
            "a3s_cloud_forms_create",
            "a3s_cloud_forms_get",
            "a3s_cloud_forms_list",
            "a3s_cloud_forms_revise",
            "a3s_cloud_form_releases_get",
            "a3s_cloud_form_releases_list",
            "a3s_cloud_form_releases_publish",
            "a3s_cloud_ontologies_create",
            "a3s_cloud_ontologies_get",
            "a3s_cloud_ontologies_list",
            "a3s_cloud_ontologies_revise",
            "a3s_cloud_ontology_revisions_get",
            "a3s_cloud_ontology_revisions_list",
            "a3s_cloud_ontology_revisions_diff",
            "a3s_cloud_workflow_node_catalog_get",
            "a3s_cloud_workflow_definitions_create",
            "a3s_cloud_workflow_definitions_get",
            "a3s_cloud_workflow_definitions_list",
            "a3s_cloud_workflow_definitions_revise",
            "a3s_cloud_workflow_revisions_get",
            "a3s_cloud_workflow_revisions_list",
            "a3s_cloud_workflow_goals_create",
            "a3s_cloud_workflow_goals_get",
            "a3s_cloud_workflow_goals_list",
            "a3s_cloud_workflow_plan_revisions_get",
            "a3s_cloud_workflow_runs_start",
            "a3s_cloud_workflow_runs_cancel",
            "a3s_cloud_workflow_runs_get",
            "a3s_cloud_workflow_runs_list",
            "a3s_cloud_workflow_runs_wait",
            "a3s_cloud_workflow_run_output_get",
            "a3s_cloud_workflow_run_history_get",
            "a3s_cloud_workflow_run_variables_get",
            "a3s_cloud_human_tasks_claim",
            "a3s_cloud_human_tasks_get",
            "a3s_cloud_human_tasks_list",
            "a3s_cloud_human_tasks_release",
            "a3s_cloud_human_tasks_submit",
            "a3s_cloud_search",
            "a3s_cloud_plugin_registries_list",
            "a3s_cloud_plugin_registries_get",
            "a3s_cloud_plugin_catalog_search",
            "a3s_cloud_plugin_catalog_search_cached",
            "a3s_cloud_plugin_catalog_inspect",
            "a3s_cloud_plugin_catalog_inspect_cached",
            "a3s_cloud_nodes_list",
            "a3s_cloud_nodes_get",
            "a3s_cloud_operations_list",
            "a3s_cloud_audit_records_list",
            "a3s_cloud_notifications_list",
            "a3s_cloud_notifications_get",
            "a3s_cloud_notifications_read",
            "a3s_cloud_notification_outbound_subscriptions_create",
            "a3s_cloud_notification_outbound_subscriptions_list",
            "a3s_cloud_notification_outbound_subscriptions_get",
            "a3s_cloud_notification_outbound_subscriptions_revoke",
            "a3s_cloud_workloads_list",
            "a3s_cloud_workloads_get",
            "a3s_cloud_workload_logs_get",
            "a3s_cloud_workloads_stop",
            "a3s_cloud_workloads_rollback",
            "a3s_cloud_deployments_get",
            "a3s_cloud_deployments_cancel",
            "a3s_cloud_routes_list",
            "a3s_cloud_routes_get",
            "a3s_cloud_build_runs_list",
            "a3s_cloud_build_runs_get",
            "a3s_cloud_build_run_logs_get",
            "a3s_cloud_build_evidence_get",
            "a3s_cloud_build_runs_cancel",
            "a3s_cloud_build_runs_retry",
        ]
    );
    let create_form = administrator_tools["result"]["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|tool| tool["name"] == "a3s_cloud_forms_create")
        .ok_or_else(|| BootError::Internal("Form create tool is missing".into()))?;
    assert_eq!(create_form["inputSchema"]["additionalProperties"], false);
    assert_eq!(
        create_form["inputSchema"]["properties"]["document"]["type"],
        "object"
    );
    assert_eq!(
        create_form["inputSchema"]["properties"]["document"]["x-a3s-max-canonical-bytes"],
        4 * 1024 * 1024
    );
    let create_resource_grant =
        listed_tool(&administrator_tools, "a3s_cloud_resource_grants_create")?;
    assert_eq!(
        create_resource_grant["inputSchema"]["additionalProperties"],
        false
    );
    assert_eq!(
        create_resource_grant["inputSchema"]["properties"]["scope"]["oneOf"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );
    let create_execution_template =
        listed_tool(&administrator_tools, "a3s_cloud_execution_templates_create")?;
    assert_eq!(
        create_execution_template["inputSchema"]["required"],
        json!(["projectId", "definitionAcl", "idempotencyKey"])
    );
    assert_eq!(
        create_execution_template["inputSchema"]["properties"]["definitionAcl"]["maxLength"],
        crate::modules::executions::EXECUTION_TEMPLATE_MAX_ACL_BYTES
    );
    assert_eq!(
        create_execution_template["inputSchema"]["additionalProperties"],
        false
    );
    assert_eq!(
        create_execution_template["annotations"]["readOnlyHint"],
        false
    );
    let create_connector_profile =
        listed_tool(&administrator_tools, "a3s_cloud_connector_profiles_create")?;
    assert_eq!(
        create_connector_profile["inputSchema"]["required"],
        json!([
            "projectId",
            "environmentId",
            "name",
            "definitionAcl",
            "idempotencyKey"
        ])
    );
    assert_eq!(
        create_connector_profile["inputSchema"]["properties"]["definitionAcl"]["maxLength"],
        crate::modules::connectors::CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES
    );
    assert_eq!(
        create_connector_profile["inputSchema"]["additionalProperties"],
        false
    );
    assert_eq!(
        create_connector_profile["annotations"]["readOnlyHint"],
        false
    );
    let list_connector_profiles =
        listed_tool(&administrator_tools, "a3s_cloud_connector_profiles_list")?;
    assert_eq!(
        list_connector_profiles["inputSchema"]["properties"]["limit"],
        json!({"type": "integer", "minimum": 1, "maximum": 200, "default": 50})
    );
    assert_eq!(list_connector_profiles["annotations"]["readOnlyHint"], true);
    let deploy_durable_cell = listed_tool(
        &administrator_tools,
        "a3s_cloud_durable_cell_deployments_create",
    )?;
    assert_eq!(
        deploy_durable_cell["inputSchema"]["required"],
        json!([
            "projectId",
            "environmentId",
            "applicationId",
            "revisionId",
            "serviceProfileAcl",
            "providerWorkloadAcl",
            "storageBindingAcl",
            "idempotencyKey"
        ])
    );
    assert_eq!(
        deploy_durable_cell["inputSchema"]["properties"]["serviceProfileAcl"]["maxLength"],
        crate::modules::durable_cells::domain::DURABLE_CELL_SERVICE_PROFILE_MAX_ACL_BYTES
    );
    assert_eq!(
        deploy_durable_cell["inputSchema"]["properties"]["storageProviderProfileAcl"]["maxLength"],
        crate::modules::data::OBJECT_NAMESPACE_PROVIDER_PROFILE_MAX_ACL_BYTES
    );
    assert!(!deploy_durable_cell["inputSchema"]["required"]
        .as_array()
        .expect("required Durable Cell deployment fields")
        .contains(&json!("storageProviderProfileAcl")));
    assert_eq!(
        deploy_durable_cell["inputSchema"]["properties"]["providerWorkloadAcl"]["maxLength"],
        crate::modules::workloads::presentation::WORKLOAD_MANIFEST_MAX_BYTES
    );
    assert_eq!(
        deploy_durable_cell["inputSchema"]["properties"]["storageBindingAcl"]["maxLength"],
        crate::modules::durable_cells::domain::DURABLE_CELL_DEPLOYMENT_MAX_ACL_BYTES
    );
    assert_eq!(
        deploy_durable_cell["inputSchema"]["additionalProperties"],
        false
    );
    assert_eq!(deploy_durable_cell["annotations"]["readOnlyHint"], false);
    let list_durable_cells = listed_tool(
        &administrator_tools,
        "a3s_cloud_durable_cell_applications_list",
    )?;
    assert_eq!(
        list_durable_cells["inputSchema"]["properties"]["limit"],
        json!({"type": "integer", "minimum": 1, "maximum": 200, "default": 50})
    );
    assert_eq!(list_durable_cells["annotations"]["readOnlyHint"], true);
    let publish_durable_cell_route = listed_tool(
        &administrator_tools,
        "a3s_cloud_durable_cell_routes_publish",
    )?;
    assert_eq!(
        publish_durable_cell_route["annotations"]["readOnlyHint"],
        false
    );
    assert_eq!(
        publish_durable_cell_route["inputSchema"]["additionalProperties"],
        false
    );
    let create_outbound_subscription = listed_tool(
        &administrator_tools,
        "a3s_cloud_notification_outbound_subscriptions_create",
    )?;
    assert_eq!(
        create_outbound_subscription["inputSchema"]["properties"]["definitionAcl"]["maxLength"],
        crate::modules::notifications::OUTBOUND_NOTIFICATION_SUBSCRIPTION_MAX_ACL_BYTES
    );
    assert_eq!(
        create_outbound_subscription["inputSchema"]["required"],
        json!(["definitionAcl", "idempotencyKey"])
    );
    assert_eq!(
        create_outbound_subscription["annotations"]["readOnlyHint"],
        false
    );
    let list_outbound_subscriptions = listed_tool(
        &administrator_tools,
        "a3s_cloud_notification_outbound_subscriptions_list",
    )?;
    assert_eq!(
        list_outbound_subscriptions["inputSchema"]["properties"]["limit"],
        json!({"type": "integer", "minimum": 1, "maximum": 200, "default": 50})
    );
    assert_eq!(
        list_outbound_subscriptions["annotations"]["readOnlyHint"],
        true
    );
    let create_workflow_definition = listed_tool(
        &administrator_tools,
        "a3s_cloud_workflow_definitions_create",
    )?;
    let workflow_semantic_contracts =
        &create_workflow_definition["inputSchema"]["properties"]["semanticContracts"];
    assert_eq!(workflow_semantic_contracts["additionalProperties"], false);
    assert_eq!(
        workflow_semantic_contracts["required"],
        json!([
            "descriptorBindingsAcl",
            "descriptorRegistryAcl",
            "variableContractAcl"
        ])
    );
    assert_eq!(
        workflow_semantic_contracts["properties"]["descriptorBindingsAcl"]["maxLength"],
        crate::modules::workflow::domain::WORKFLOW_STEP_DESCRIPTOR_BINDINGS_MAX_ACL_BYTES
    );
    assert_eq!(
        workflow_semantic_contracts["properties"]["variableDefaultsAcl"]["maxLength"],
        crate::modules::workflow::domain::WORKFLOW_VARIABLE_DEFAULTS_MAX_ACL_BYTES
    );
    assert_eq!(
        workflow_semantic_contracts["properties"]["compositeRegionsAcl"]["maxLength"],
        crate::modules::workflow::domain::WORKFLOW_COMPOSITE_REGIONS_MAX_ACL_BYTES
    );
    let workflow_node_catalog =
        listed_tool(&administrator_tools, "a3s_cloud_workflow_node_catalog_get")?;
    assert_eq!(workflow_node_catalog["annotations"]["readOnlyHint"], true);
    assert_eq!(
        workflow_node_catalog["inputSchema"]["required"],
        json!(["projectId"])
    );
    assert_eq!(
        workflow_node_catalog["inputSchema"]["additionalProperties"],
        false
    );
    for name in [
        "a3s_cloud_human_tasks_claim",
        "a3s_cloud_human_tasks_release",
    ] {
        let tool = listed_tool(&administrator_tools, name)?;
        assert_eq!(tool["annotations"]["readOnlyHint"], false, "{name}");
        assert_eq!(tool["annotations"]["destructiveHint"], false, "{name}");
        assert_eq!(tool["annotations"]["idempotentHint"], true, "{name}");
        assert_eq!(
            tool["inputSchema"]["required"],
            json!(["humanTaskId", "expectedVersion", "idempotencyKey"]),
            "{name}"
        );
        assert_eq!(tool["inputSchema"]["additionalProperties"], false, "{name}");
    }
    let submit_human_task = listed_tool(&administrator_tools, "a3s_cloud_human_tasks_submit")?;
    assert_eq!(submit_human_task["annotations"]["readOnlyHint"], false);
    assert_eq!(submit_human_task["annotations"]["destructiveHint"], false);
    assert_eq!(submit_human_task["annotations"]["idempotentHint"], true);
    assert_eq!(
        submit_human_task["inputSchema"]["required"],
        json!(["humanTaskId", "submission"])
    );
    assert_eq!(
        submit_human_task["inputSchema"]["properties"]["submission"]["properties"]["apiVersion"]
            ["enum"],
        json!(["a3s.dev/form-interaction-submission/v1"])
    );
    assert_eq!(
        submit_human_task["inputSchema"]["properties"]["submission"]["additionalProperties"],
        false
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
async fn management_mcp_plugin_catalog_tools_reuse_use_contracts_and_query_bus() -> Result<()> {
    let app = build_test_application(
        Arc::new(InMemoryIdentityRepository::new()),
        Arc::new(InMemoryProjectsRepository::new()),
    )?;
    let organization = bootstrap_organization(&app, "mcp-plugins", "Plugins").await?;
    let tools = list_tools(&app, ADMIN_TOKEN, 1).await?;

    let search_schema = &listed_tool(&tools, "a3s_cloud_plugin_catalog_search")?["inputSchema"];
    assert_eq!(
        search_schema["properties"]["host"],
        plugin_catalog_host_input_schema()
    );
    assert_eq!(
        search_schema["properties"]["search"],
        plugin_catalog_search_input_schema()
    );
    assert_eq!(
        search_schema["properties"]["registryId"],
        json!({"type": "string", "format": "uuid"})
    );

    let inspection_schema =
        &listed_tool(&tools, "a3s_cloud_plugin_catalog_inspect")?["inputSchema"];
    let canonical_inspection = plugin_catalog_inspection_input_schema();
    for property in ["packageId", "version", "channel"] {
        assert_eq!(
            inspection_schema["properties"][property], canonical_inspection["properties"][property],
            "{property}"
        );
    }
    assert_eq!(
        inspection_schema["properties"]["host"],
        plugin_catalog_host_input_schema()
    );
    assert_eq!(
        inspection_schema["required"],
        json!(["registryId", "host", "packageId"])
    );

    let listed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(2, "a3s_cloud_plugin_registries_list", json!({})),
        ))
        .await?;
    assert_eq!(
        response_json(&listed)?["result"]["structuredContent"]["data"],
        json!([])
    );

    let registry_id = Uuid::now_v7();
    let get = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                3,
                "a3s_cloud_plugin_registries_get",
                json!({"registryId": registry_id}),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&get)?["result"]["structuredContent"]["code"],
        404
    );

    let host = json!({
        "target": "x86_64-unknown-linux-gnu",
        "useVersion": "0.3.0"
    });
    for (id, name, arguments) in [
        (
            4,
            "a3s_cloud_plugin_catalog_search",
            json!({
                "registryId": registry_id,
                "host": host,
                "search": {"query": "a3s", "limit": 20}
            }),
        ),
        (
            5,
            "a3s_cloud_plugin_catalog_search_cached",
            json!({
                "registryId": registry_id,
                "host": host,
                "search": {"query": "a3s", "limit": 20}
            }),
        ),
        (
            6,
            "a3s_cloud_plugin_catalog_inspect",
            json!({
                "registryId": registry_id,
                "host": host,
                "packageId": "a3s/example"
            }),
        ),
        (
            7,
            "a3s_cloud_plugin_catalog_inspect_cached",
            json!({
                "registryId": registry_id,
                "host": host,
                "packageId": "a3s/example"
            }),
        ),
    ] {
        let response = app
            .call(mcp_request(
                Some(ADMIN_TOKEN),
                tool_call(id, name, arguments),
            ))
            .await?;
        let body = response_json(&response)?;
        assert_eq!(body["result"]["isError"], true, "{name}");
        assert_eq!(body["result"]["structuredContent"]["code"], 404, "{name}");
    }

    let unknown_argument = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                8,
                "a3s_cloud_plugin_catalog_search",
                json!({
                    "registryId": registry_id,
                    "host": host,
                    "search": {"query": "a3s", "limit": 20},
                    "organizationId": organization
                }),
            ),
        ))
        .await?;
    assert_eq!(response_json(&unknown_argument)?["error"]["code"], -32602);
    Ok(())
}

#[tokio::test]
async fn management_mcp_reuses_membership_commands_queries_and_idempotency() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    bootstrap_organization(&app, "mcp-memberships", "Acme").await?;

    let create_arguments = json!({
        "principalKind": "human",
        "name": "Release operator",
        "role": "member",
        "idempotencyKey": "mcp-membership-create"
    });
    let created = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(1, "a3s_cloud_memberships_create", create_arguments.clone()),
        ))
        .await?;
    let created_body = response_json(&created)?;
    assert_eq!(created_body["result"]["structuredContent"]["code"], 201);
    assert_eq!(
        created_body["result"]["structuredContent"]["data"]["principalKind"],
        "human"
    );
    assert_eq!(
        created_body["result"]["structuredContent"]["data"]["replayed"],
        false
    );
    let membership_id = created_body["result"]["structuredContent"]["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP membership response has no ID".into()))?
        .to_owned();

    let replayed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(2, "a3s_cloud_memberships_create", create_arguments),
        ))
        .await?;
    let replayed_body = response_json(&replayed)?;
    assert_eq!(replayed_body["result"]["structuredContent"]["code"], 200);
    assert_eq!(
        replayed_body["result"]["structuredContent"]["data"]["id"],
        membership_id
    );
    assert_eq!(
        replayed_body["result"]["structuredContent"]["data"]["replayed"],
        true
    );

    for (id, arguments) in [
        (
            7,
            json!({
                "name": "Missing kind",
                "role": "member",
                "idempotencyKey": "mcp-membership-missing-kind"
            }),
        ),
        (
            8,
            json!({
                "principalKind": "robot",
                "name": "Invalid kind",
                "role": "member",
                "idempotencyKey": "mcp-membership-invalid-kind"
            }),
        ),
    ] {
        let invalid = app
            .call(mcp_request(
                Some(ADMIN_TOKEN),
                tool_call(id, "a3s_cloud_memberships_create", arguments),
            ))
            .await?;
        assert_eq!(response_json(&invalid)?["error"]["code"], -32602);
    }

    let listed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(3, "a3s_cloud_memberships_list", json!({})),
        ))
        .await?;
    let listed_body = response_json(&listed)?;
    assert!(listed_body["result"]["structuredContent"]["data"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|membership| membership["id"] == membership_id));

    let fetched = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                4,
                "a3s_cloud_memberships_get",
                json!({"membershipId": membership_id}),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&fetched)?["result"]["structuredContent"]["data"]["role"],
        "member"
    );

    let changed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                5,
                "a3s_cloud_memberships_change_role",
                json!({
                    "membershipId": membership_id,
                    "role": "restricted",
                    "expectedVersion": 1,
                    "idempotencyKey": "mcp-membership-restrict"
                }),
            ),
        ))
        .await?;
    let changed_body = response_json(&changed)?;
    assert_eq!(changed_body["result"]["structuredContent"]["code"], 200);
    assert_eq!(
        changed_body["result"]["structuredContent"]["data"]["role"],
        "restricted"
    );
    assert_eq!(
        changed_body["result"]["structuredContent"]["data"]["aggregateVersion"],
        2
    );

    let revoked = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                6,
                "a3s_cloud_memberships_revoke",
                json!({
                    "membershipId": membership_id,
                    "expectedVersion": 2,
                    "idempotencyKey": "mcp-membership-revoke"
                }),
            ),
        ))
        .await?;
    let revoked_body = response_json(&revoked)?;
    assert_eq!(revoked_body["result"]["structuredContent"]["code"], 200);
    assert_eq!(
        revoked_body["result"]["structuredContent"]["data"]["aggregateVersion"],
        3
    );
    assert!(revoked_body["result"]["structuredContent"]["data"]["revokedAt"].is_string());
    Ok(())
}

#[tokio::test]
async fn management_mcp_reuses_principal_bound_membership_invitations() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let invited_organization =
        bootstrap_organization(&app, "mcp-invitation-target", "Invitation target").await?;
    let principal_organization =
        create_organization(&app, "mcp-invitation-principal", "Principal home").await?;

    let service = app
        .call(post_json(
            format!("/api/v1/organizations/{principal_organization}/memberships"),
            "mcp-invitation-service",
            json!({"name": "Invited automation", "role": "member"}),
        ))
        .await?;
    assert_eq!(service.status(), 201);
    let service = response_json(&service)?;
    let principal_id = service["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("invited service has no Principal ID".into()))?
        .to_owned();
    let credential = app
        .call(post_json(
            format!("/api/v1/organizations/{principal_organization}/api-tokens"),
            "mcp-invitation-token",
            json!({
                "name": "Invitation self service",
                "token": MCP_INVITEE_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ, ApiTokenScope::IDENTITY_WRITE],
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(credential.status(), 201);

    let create_arguments = json!({
        "principalId": principal_id,
        "role": "restricted",
        "expiresAt": Utc::now() + chrono::Duration::days(7),
        "idempotencyKey": "mcp-invitation-create"
    });
    let created = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                1,
                "a3s_cloud_membership_invitations_create",
                create_arguments.clone(),
            ),
        ))
        .await?;
    let created = response_json(&created)?;
    assert_eq!(created["result"]["structuredContent"]["code"], 201);
    assert_eq!(
        created["result"]["structuredContent"]["data"]["status"],
        "pending"
    );
    let invitation_id = created["result"]["structuredContent"]["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP invitation response has no ID".into()))?
        .to_owned();

    let replayed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                2,
                "a3s_cloud_membership_invitations_create",
                create_arguments,
            ),
        ))
        .await?;
    let replayed = response_json(&replayed)?;
    assert_eq!(replayed["result"]["structuredContent"]["code"], 200);
    assert_eq!(
        replayed["result"]["structuredContent"]["data"]["replayed"],
        true
    );

    let listed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(3, "a3s_cloud_membership_invitations_list", json!({})),
        ))
        .await?;
    assert!(
        response_json(&listed)?["result"]["structuredContent"]["data"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|invitation| invitation["id"] == invitation_id)
    );

    let fetched = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                4,
                "a3s_cloud_membership_invitations_get",
                json!({"invitationId": invitation_id}),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&fetched)?["result"]["structuredContent"]["data"]["principalId"],
        principal_id
    );

    let mine = app
        .call(mcp_request(
            Some(MCP_INVITEE_TOKEN),
            tool_call(5, "a3s_cloud_my_membership_invitations_list", json!({})),
        ))
        .await?;
    assert!(response_json(&mine)?["result"]["structuredContent"]["data"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|invitation| invitation["id"] == invitation_id));

    let invalid_version = app
        .call(mcp_request(
            Some(MCP_INVITEE_TOKEN),
            tool_call(
                6,
                "a3s_cloud_membership_invitations_accept",
                json!({
                    "invitationId": invitation_id,
                    "expectedVersion": 0,
                    "idempotencyKey": "mcp-invitation-invalid-version"
                }),
            ),
        ))
        .await?;
    let invalid_version = response_json(&invalid_version)?;
    assert_eq!(
        invalid_version["error"]["code"], -32602,
        "{invalid_version:#}"
    );

    let revoked = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                7,
                "a3s_cloud_membership_invitations_revoke",
                json!({
                    "invitationId": invitation_id,
                    "expectedVersion": 1,
                    "idempotencyKey": "mcp-invitation-revoke"
                }),
            ),
        ))
        .await?;
    let revoked = response_json(&revoked)?;
    assert_eq!(
        revoked["result"]["structuredContent"]["data"]["status"],
        "revoked"
    );
    assert_eq!(
        revoked["result"]["structuredContent"]["data"]["aggregateVersion"],
        2
    );

    let second = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                8,
                "a3s_cloud_membership_invitations_create",
                json!({
                    "principalId": principal_id,
                    "role": "member",
                    "expiresAt": Utc::now() + chrono::Duration::days(7),
                    "idempotencyKey": "mcp-invitation-create-second"
                }),
            ),
        ))
        .await?;
    let second = response_json(&second)?;
    let second_id = second["result"]["structuredContent"]["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("second MCP invitation has no ID".into()))?
        .to_owned();

    let guessed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                9,
                "a3s_cloud_membership_invitations_accept",
                json!({
                    "invitationId": second_id,
                    "expectedVersion": 1,
                    "idempotencyKey": "mcp-invitation-wrong-principal"
                }),
            ),
        ))
        .await?;
    let guessed = response_json(&guessed)?;
    assert_eq!(guessed["result"]["isError"], true);
    assert_eq!(guessed["result"]["structuredContent"]["code"], 404);

    let accept_arguments = json!({
        "invitationId": second_id,
        "expectedVersion": 1,
        "idempotencyKey": "mcp-invitation-accept"
    });
    let accepted = app
        .call(mcp_request(
            Some(MCP_INVITEE_TOKEN),
            tool_call(
                10,
                "a3s_cloud_membership_invitations_accept",
                accept_arguments.clone(),
            ),
        ))
        .await?;
    let accepted = response_json(&accepted)?;
    assert_eq!(accepted["result"]["structuredContent"]["code"], 201);
    assert_eq!(
        accepted["result"]["structuredContent"]["data"]["invitation"]["status"],
        "accepted"
    );
    assert_eq!(
        accepted["result"]["structuredContent"]["data"]["membership"]["organizationId"],
        invited_organization
    );
    assert_eq!(
        accepted["result"]["structuredContent"]["data"]["membership"]["principalId"],
        principal_id
    );

    let accepted_replay = app
        .call(mcp_request(
            Some(MCP_INVITEE_TOKEN),
            tool_call(
                11,
                "a3s_cloud_membership_invitations_accept",
                accept_arguments,
            ),
        ))
        .await?;
    let accepted_replay = response_json(&accepted_replay)?;
    assert_eq!(
        accepted_replay["result"]["structuredContent"]["data"]["replayed"],
        true
    );
    Ok(())
}

#[tokio::test]
async fn management_mcp_reuses_resource_grant_commands_queries_and_idempotency() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "mcp-resource-grants", "Acme").await?;

    let membership = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                1,
                "a3s_cloud_memberships_create",
                json!({
                    "principalKind": "service",
                    "name": "Restricted deployment observer",
                    "role": "restricted",
                    "idempotencyKey": "mcp-resource-grant-membership"
                }),
            ),
        ))
        .await?;
    let membership = response_json(&membership)?;
    let membership_id = membership["result"]["structuredContent"]["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP membership response has no ID".into()))?;
    let project = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects"),
            "mcp-resource-grant-project",
            json!({"name": "Granted project"}),
        ))
        .await?;
    assert_eq!(project.status(), 201);
    let project_id = response_json(&project)?["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP Resource Grant project has no ID".into()))?
        .parse::<Uuid>()
        .map_err(|error| BootError::Internal(format!("invalid project ID: {error}")))?;
    let create_arguments = json!({
        "membershipId": membership_id,
        "scope": {"kind": "project", "projectId": project_id},
        "idempotencyKey": "mcp-resource-grant-create"
    });

    let created = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                2,
                "a3s_cloud_resource_grants_create",
                create_arguments.clone(),
            ),
        ))
        .await?;
    let created = response_json(&created)?;
    assert_eq!(created["result"]["structuredContent"]["code"], 201);
    assert_eq!(
        created["result"]["structuredContent"]["data"]["scope"],
        json!({"kind": "project", "projectId": project_id})
    );
    assert_eq!(
        created["result"]["structuredContent"]["data"]["replayed"],
        false
    );
    let resource_grant_id = created["result"]["structuredContent"]["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP Resource Grant response has no ID".into()))?
        .to_owned();

    let replayed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(3, "a3s_cloud_resource_grants_create", create_arguments),
        ))
        .await?;
    let replayed = response_json(&replayed)?;
    assert_eq!(replayed["result"]["structuredContent"]["code"], 200);
    assert_eq!(
        replayed["result"]["structuredContent"]["data"]["id"],
        resource_grant_id
    );
    assert_eq!(
        replayed["result"]["structuredContent"]["data"]["replayed"],
        true
    );

    let listed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                4,
                "a3s_cloud_resource_grants_list",
                json!({"membershipId": membership_id}),
            ),
        ))
        .await?;
    let listed = response_json(&listed)?;
    assert_eq!(
        listed["result"]["structuredContent"]["data"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );

    let fetched = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                5,
                "a3s_cloud_resource_grants_get",
                json!({"resourceGrantId": resource_grant_id}),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&fetched)?["result"]["structuredContent"]["data"]["membershipId"],
        membership_id
    );

    let revoke_arguments = json!({
        "resourceGrantId": resource_grant_id,
        "expectedVersion": 1,
        "idempotencyKey": "mcp-resource-grant-revoke"
    });
    let revoked = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                6,
                "a3s_cloud_resource_grants_revoke",
                revoke_arguments.clone(),
            ),
        ))
        .await?;
    let revoked = response_json(&revoked)?;
    assert_eq!(revoked["result"]["structuredContent"]["code"], 200);
    assert_eq!(
        revoked["result"]["structuredContent"]["data"]["aggregateVersion"],
        2
    );
    assert!(revoked["result"]["structuredContent"]["data"]["revokedAt"].is_string());

    let replayed_revoke = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(7, "a3s_cloud_resource_grants_revoke", revoke_arguments),
        ))
        .await?;
    assert_eq!(
        response_json(&replayed_revoke)?["result"]["structuredContent"]["data"]["replayed"],
        true
    );

    let malformed_scope = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                8,
                "a3s_cloud_resource_grants_create",
                json!({
                    "membershipId": membership_id,
                    "scope": {
                        "kind": "project",
                        "projectId": project_id,
                        "nodeId": Uuid::now_v7()
                    },
                    "idempotencyKey": "mcp-resource-grant-malformed"
                }),
            ),
        ))
        .await?;
    assert_eq!(response_json(&malformed_scope)?["error"]["code"], -32602);
    Ok(())
}

#[tokio::test]
async fn management_mcp_form_tools_follow_current_membership_role() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "mcp-form-role", "Acme").await?;

    let member = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                1,
                "a3s_cloud_memberships_create",
                json!({
                    "principalKind": "human",
                    "name": "Form author",
                    "role": "member",
                    "idempotencyKey": "mcp-form-member-create"
                }),
            ),
        ))
        .await?;
    let member = response_json(&member)?;
    let membership_id = member["result"]["structuredContent"]["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP membership response has no ID".into()))?;
    let principal_id = member["result"]["structuredContent"]["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP membership response has no principal ID".into()))?;
    let token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "mcp-form-member-token",
            json!({
                "name": "Form author",
                "token": MCP_FORM_MEMBER_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ, ApiTokenScope::FORM_WRITE],
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(token.status(), 201);

    let member_tools = list_tools(&app, MCP_FORM_MEMBER_TOKEN, 2).await?;
    assert!(tool_names(&member_tools).contains(&"a3s_cloud_forms_create"));
    assert!(tool_names(&member_tools).contains(&"a3s_cloud_form_releases_publish"));
    assert!(!tool_names(&member_tools).contains(&"a3s_cloud_memberships_list"));
    assert!(!tool_names(&member_tools).contains(&"a3s_cloud_memberships_change_role"));

    let restricted = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                3,
                "a3s_cloud_memberships_change_role",
                json!({
                    "membershipId": membership_id,
                    "role": "restricted",
                    "expectedVersion": 1,
                    "idempotencyKey": "mcp-form-member-restrict"
                }),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&restricted)?["result"]["structuredContent"]["code"],
        200
    );

    let restricted_tools = list_tools(&app, MCP_FORM_MEMBER_TOKEN, 4).await?;
    assert_eq!(
        tool_names(&restricted_tools),
        vec![
            "a3s_cloud_my_membership_invitations_list",
            "a3s_cloud_notifications_list",
            "a3s_cloud_notifications_get",
            "a3s_cloud_notification_outbound_subscriptions_list",
            "a3s_cloud_notification_outbound_subscriptions_get",
        ]
    );
    let denied = app
        .call(mcp_request(
            Some(MCP_FORM_MEMBER_TOKEN),
            tool_call(
                5,
                "a3s_cloud_forms_list",
                json!({"projectId": Uuid::new_v4()}),
            ),
        ))
        .await?;
    assert_eq!(response_json(&denied)?["error"]["code"], -32602);
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

    let stopped = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                24,
                "a3s_cloud_workloads_stop",
                json!({
                    "workloadId": workload_id,
                    "idempotencyKey": "mcp-stop-workload"
                }),
            ),
        ))
        .await?;
    let stopped = response_json(&stopped)?;
    assert_eq!(stopped["result"]["isError"], false);
    assert_eq!(stopped["result"]["structuredContent"]["code"], 202);
    assert_eq!(
        stopped["result"]["structuredContent"]["data"]["workloadId"],
        workload_id
    );
    assert_eq!(
        stopped["result"]["structuredContent"]["data"]["replayed"],
        false
    );

    let stop_replay = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                25,
                "a3s_cloud_workloads_stop",
                json!({
                    "workloadId": workload_id,
                    "idempotencyKey": "mcp-stop-workload"
                }),
            ),
        ))
        .await?;
    let stop_replay = response_json(&stop_replay)?;
    assert_eq!(stop_replay["result"]["structuredContent"]["code"], 200);
    assert_eq!(
        stop_replay["result"]["structuredContent"]["data"]["replayed"],
        true
    );

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
        (
            26,
            "a3s_cloud_workloads_stop",
            json!({
                "workloadId": missing_resource_id,
                "idempotencyKey": "missing-workload-stop"
            }),
        ),
        (
            27,
            "a3s_cloud_workloads_rollback",
            json!({
                "workloadId": missing_resource_id,
                "sourceRevisionId": missing_resource_id,
                "idempotencyKey": "missing-workload-rollback"
            }),
        ),
        (
            28,
            "a3s_cloud_deployments_cancel",
            json!({
                "deploymentId": missing_resource_id,
                "idempotencyKey": "missing-deployment-cancel"
            }),
        ),
        (
            29,
            "a3s_cloud_build_runs_cancel",
            json!({
                "buildRunId": missing_resource_id,
                "idempotencyKey": "missing-build-cancel"
            }),
        ),
        (
            30,
            "a3s_cloud_build_runs_retry",
            json!({
                "buildRunId": missing_resource_id,
                "idempotencyKey": "missing-build-retry"
            }),
        ),
        (
            34,
            "a3s_cloud_workflow_runs_start",
            json!({
                "projectId": project,
                "workflowGoalId": missing_resource_id,
                "planRevisionId": missing_resource_id,
                "idempotencyKey": "missing-workflow-run-start"
            }),
        ),
        (
            35,
            "a3s_cloud_workflow_runs_cancel",
            json!({
                "workflowRunId": missing_resource_id,
                "idempotencyKey": "missing-workflow-run-cancel"
            }),
        ),
        (
            36,
            "a3s_cloud_workflow_runs_get",
            json!({"workflowRunId": missing_resource_id}),
        ),
        (
            37,
            "a3s_cloud_workflow_runs_wait",
            json!({"workflowRunId": missing_resource_id, "timeoutSeconds": 0}),
        ),
        (
            38,
            "a3s_cloud_workflow_run_output_get",
            json!({"workflowRunId": missing_resource_id}),
        ),
        (
            39,
            "a3s_cloud_workflow_run_history_get",
            json!({"workflowRunId": missing_resource_id}),
        ),
        (
            40,
            "a3s_cloud_workflow_run_variables_get",
            json!({"workflowRunId": missing_resource_id}),
        ),
        (
            44,
            "a3s_cloud_human_tasks_get",
            json!({"humanTaskId": missing_resource_id}),
        ),
        (
            47,
            "a3s_cloud_human_tasks_claim",
            json!({
                "humanTaskId": missing_resource_id,
                "expectedVersion": 2,
                "idempotencyKey": "missing-human-task-claim"
            }),
        ),
        (
            48,
            "a3s_cloud_human_tasks_release",
            json!({
                "humanTaskId": missing_resource_id,
                "expectedVersion": 3,
                "idempotencyKey": "missing-human-task-release"
            }),
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
        (
            31,
            "a3s_cloud_workloads_stop",
            json!({"workloadId": missing_resource_id}),
        ),
        (
            32,
            "a3s_cloud_workloads_rollback",
            json!({
                "workloadId": missing_resource_id,
                "idempotencyKey": "missing-source-revision"
            }),
        ),
        (
            33,
            "a3s_cloud_build_runs_cancel",
            json!({
                "buildRunId": missing_resource_id,
                "idempotencyKey": "unknown-field",
                "organizationId": organization
            }),
        ),
        (
            40,
            "a3s_cloud_workflow_runs_start",
            json!({
                "projectId": project,
                "workflowGoalId": missing_resource_id,
                "planRevisionId": missing_resource_id,
                "timeoutSeconds": 0,
                "idempotencyKey": "invalid-workflow-run-start"
            }),
        ),
        (
            41,
            "a3s_cloud_workflow_runs_list",
            json!({"projectId": project, "limit": 201}),
        ),
        (
            42,
            "a3s_cloud_workflow_runs_wait",
            json!({"workflowRunId": missing_resource_id, "timeoutSeconds": 31}),
        ),
        (
            43,
            "a3s_cloud_workflow_run_history_get",
            json!({"workflowRunId": missing_resource_id, "limit": 0}),
        ),
        (
            45,
            "a3s_cloud_human_tasks_list",
            json!({"projectId": project, "limit": 201}),
        ),
        (
            46,
            "a3s_cloud_human_tasks_list",
            json!({"projectId": project, "status": "assigned"}),
        ),
        (
            49,
            "a3s_cloud_human_tasks_claim",
            json!({
                "humanTaskId": missing_resource_id,
                "expectedVersion": 0,
                "idempotencyKey": "invalid-human-task-claim"
            }),
        ),
        (
            50,
            "a3s_cloud_human_tasks_release",
            json!({
                "humanTaskId": missing_resource_id,
                "expectedVersion": 3,
                "idempotencyKey": "invalid-human-task-release",
                "organizationId": organization
            }),
        ),
        (
            51,
            "a3s_cloud_human_tasks_submit",
            json!({"humanTaskId": missing_resource_id, "submission": {}}),
        ),
        (
            52,
            "a3s_cloud_execution_templates_list",
            json!({"projectId": project, "limit": 201}),
        ),
        (
            53,
            "a3s_cloud_execution_templates_create",
            json!({
                "projectId": project,
                "definitionAcl": "invalid",
                "idempotencyKey": "invalid-execution-template",
                "organizationId": organization
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
async fn management_mcp_reuses_the_execution_template_lifecycle() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "mcp-execution-template", "Acme").await?;
    let project = create_project(
        &app,
        &organization,
        "mcp-execution-template-project",
        "Automation",
    )
    .await?;
    let definition_acl = super::execution_tests::execution_template_acl()?;
    let create_arguments = json!({
        "projectId": project,
        "definitionAcl": definition_acl,
        "idempotencyKey": "mcp-execution-template-create"
    });

    let created = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                1,
                "a3s_cloud_execution_templates_create",
                create_arguments.clone(),
            ),
        ))
        .await?;
    let created = response_json(&created)?;
    assert_eq!(created["result"]["structuredContent"]["code"], 201);
    let revision = &created["result"]["structuredContent"]["data"]["executionTemplate"];
    let template_id = revision["templateId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP ExecutionTemplate has no template ID".into()))?;
    let revision_id = revision["revisionId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP ExecutionTemplate has no revision ID".into()))?;
    assert_eq!(revision["capability"], "execution.run");
    assert_eq!(revision["definitionAcl"], definition_acl);

    let replay = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(2, "a3s_cloud_execution_templates_create", create_arguments),
        ))
        .await?;
    let replay = response_json(&replay)?;
    assert_eq!(replay["result"]["structuredContent"]["code"], 200);
    assert_eq!(
        replay["result"]["structuredContent"]["data"]["replayed"],
        true
    );
    assert_eq!(
        replay["result"]["structuredContent"]["data"]["executionTemplate"]["revisionId"],
        revision_id
    );

    let listed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                3,
                "a3s_cloud_execution_templates_list",
                json!({"projectId": project, "limit": 1}),
            ),
        ))
        .await?;
    let listed = response_json(&listed)?;
    assert_eq!(
        listed["result"]["structuredContent"]["data"][0]["templateId"],
        template_id
    );

    let fetched = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                4,
                "a3s_cloud_execution_templates_get",
                json!({
                    "projectId": project,
                    "templateId": template_id,
                    "revisionId": revision_id
                }),
            ),
        ))
        .await?;
    let fetched = response_json(&fetched)?;
    assert_eq!(
        fetched["result"]["structuredContent"]["data"]["definitionDigest"],
        revision["definitionDigest"]
    );

    let missing = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                5,
                "a3s_cloud_execution_templates_get",
                json!({
                    "projectId": project,
                    "templateId": Uuid::now_v7(),
                    "revisionId": Uuid::now_v7()
                }),
            ),
        ))
        .await?;
    let missing = response_json(&missing)?;
    assert_eq!(missing["result"]["isError"], true);
    assert_eq!(missing["result"]["structuredContent"]["code"], 404);

    let foreign_organization =
        create_organization(&app, "mcp-execution-template-foreign", "Foreign").await?;
    let foreign_project = create_project(
        &app,
        &foreign_organization,
        "mcp-execution-template-foreign-project",
        "Foreign automation",
    )
    .await?;
    for (id, project_id) in [(6, foreign_project), (7, Uuid::now_v7().to_string())] {
        let denied = app
            .call(mcp_request(
                Some(ADMIN_TOKEN),
                tool_call(
                    id,
                    "a3s_cloud_execution_templates_list",
                    json!({"projectId": project_id}),
                ),
            ))
            .await?;
        let denied = response_json(&denied)?;
        assert_eq!(denied["result"]["isError"], true);
        assert_eq!(denied["result"]["structuredContent"]["code"], 404);
        assert_eq!(
            denied["result"]["structuredContent"]["statusCode"],
            "NOT_FOUND"
        );
    }
    Ok(())
}

#[tokio::test]
async fn management_mcp_reuses_the_connector_profile_revision_lifecycle() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "mcp-connectors", "Connectors").await?;
    let project = create_project(
        &app,
        &organization,
        "mcp-connectors-project",
        "Connector project",
    )
    .await?;
    let environment = super::connector_tests::create_connector_environment(
        &app,
        &organization,
        &project,
        "mcp-connectors-environment",
    )
    .await?;
    let initial_acl = super::connector_tests::connector_acl(1_000)?;
    let create_arguments = json!({
        "projectId": project,
        "environmentId": environment,
        "name": "Incident webhook",
        "definitionAcl": initial_acl,
        "idempotencyKey": "mcp-connector-create"
    });

    let created = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                1,
                "a3s_cloud_connector_profiles_create",
                create_arguments.clone(),
            ),
        ))
        .await?;
    let created = response_json(&created)?;
    assert_eq!(created["result"]["structuredContent"]["code"], 201);
    let record = &created["result"]["structuredContent"]["data"]["record"];
    assert_eq!(record["profile"]["aggregateVersion"], 1);
    assert_eq!(record["revision"]["revisionNumber"], 1);
    assert_eq!(record["revision"]["definitionAcl"], initial_acl);
    assert!(record["revision"].get("endpoint").is_none());
    let profile_id = super::connector_tests::required_connector_string(
        &record["profile"]["profileId"],
        "MCP profile ID",
    )?;
    let initial_revision_id = super::connector_tests::required_connector_string(
        &record["revision"]["revisionId"],
        "MCP revision ID",
    )?;

    let replay = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(2, "a3s_cloud_connector_profiles_create", create_arguments),
        ))
        .await?;
    let replay = response_json(&replay)?;
    assert_eq!(replay["result"]["structuredContent"]["code"], 200);
    assert_eq!(
        replay["result"]["structuredContent"]["data"]["replayed"],
        true
    );
    assert_eq!(
        replay["result"]["structuredContent"]["data"]["record"]["profile"]["profileId"],
        profile_id
    );

    let listed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                3,
                "a3s_cloud_connector_profiles_list",
                json!({
                    "projectId": project,
                    "environmentId": environment,
                    "limit": 1
                }),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&listed)?["result"]["structuredContent"]["data"][0]["profileId"],
        profile_id
    );

    let fetched = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                4,
                "a3s_cloud_connector_profiles_get",
                json!({
                    "projectId": project,
                    "environmentId": environment,
                    "profileId": profile_id
                }),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&fetched)?["result"]["structuredContent"]["data"]["revision"]["revisionId"],
        initial_revision_id
    );

    let revised_acl = super::connector_tests::connector_acl(2_000)?;
    let revised = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                5,
                "a3s_cloud_connector_profiles_revise",
                json!({
                    "projectId": project,
                    "environmentId": environment,
                    "profileId": profile_id,
                    "expectedVersion": 1,
                    "definitionAcl": revised_acl,
                    "idempotencyKey": "mcp-connector-revise"
                }),
            ),
        ))
        .await?;
    let revised = response_json(&revised)?;
    assert_eq!(revised["result"]["structuredContent"]["code"], 201);
    let revised_record = &revised["result"]["structuredContent"]["data"]["record"];
    assert_eq!(revised_record["profile"]["aggregateVersion"], 2);
    assert_eq!(revised_record["revision"]["revisionNumber"], 2);
    assert_eq!(
        revised_record["revision"]["parentRevisionId"],
        initial_revision_id
    );
    let revised_revision_id = super::connector_tests::required_connector_string(
        &revised_record["revision"]["revisionId"],
        "revised MCP revision ID",
    )?;

    let revisions = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                6,
                "a3s_cloud_connector_revisions_list",
                json!({
                    "projectId": project,
                    "environmentId": environment,
                    "profileId": profile_id
                }),
            ),
        ))
        .await?;
    let revisions = response_json(&revisions)?;
    assert_eq!(
        revisions["result"]["structuredContent"]["data"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        revisions["result"]["structuredContent"]["data"][0]["revisionId"],
        revised_revision_id
    );

    let initial_revision = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                7,
                "a3s_cloud_connector_revisions_get",
                json!({
                    "projectId": project,
                    "environmentId": environment,
                    "profileId": profile_id,
                    "revisionId": initial_revision_id
                }),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&initial_revision)?["result"]["structuredContent"]["data"]["revisionNumber"],
        1
    );

    for (id, name, arguments) in [
        (
            8,
            "a3s_cloud_connector_profiles_list",
            json!({
                "projectId": project,
                "environmentId": environment,
                "limit": 201
            }),
        ),
        (
            9,
            "a3s_cloud_connector_profiles_get",
            json!({
                "projectId": project,
                "environmentId": environment,
                "profileId": profile_id,
                "organizationId": organization
            }),
        ),
    ] {
        let rejected = app
            .call(mcp_request(
                Some(ADMIN_TOKEN),
                tool_call(id, name, arguments),
            ))
            .await?;
        assert_eq!(response_json(&rejected)?["error"]["code"], -32602, "{name}");
    }
    Ok(())
}

#[tokio::test]
async fn management_mcp_reuses_the_durable_cell_application_projection_lifecycle() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let secrets = Arc::new(InMemorySecretRepository::new());
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let builds = Arc::new(InMemoryBuildRunRepository::new());
    let app = build_test_application_with_external_builds(
        identity,
        projects,
        Arc::clone(&secrets),
        workloads,
        Arc::new(InMemorySourceRevisionRepository::new()),
        Arc::clone(&builds),
    )?;
    let organization = bootstrap_organization(&app, "mcp-cells", "Durable Cells").await?;
    let project = create_project(&app, &organization, "mcp-cells-project", "Cells project").await?;
    let environment = super::durable_cell_tests::create_cell_environment(
        &app,
        &organization,
        &project,
        "mcp-cells-environment",
        "Production",
    )
    .await?;

    let organization_id = OrganizationId::from_uuid(super::durable_cell_tests::parse_cell_uuid(
        &organization,
        "organization",
    )?);
    let project_id = ProjectId::from_uuid(super::durable_cell_tests::parse_cell_uuid(
        &project, "project",
    )?);
    let environment_id = EnvironmentId::from_uuid(super::durable_cell_tests::parse_cell_uuid(
        &environment,
        "environment",
    )?);
    let build_run_id = super::durable_cell_tests::seed_cell_build(
        builds.as_ref(),
        organization_id,
        project_id,
        environment_id,
        'a',
    )
    .await?;
    let revised_build_run_id = super::durable_cell_tests::seed_cell_build(
        builds.as_ref(),
        organization_id,
        project_id,
        environment_id,
        'b',
    )
    .await?;
    let access_key = super::durable_cell_tests::store_cell_secret(
        secrets.as_ref(),
        organization_id,
        project_id,
        environment_id,
        "MCP S0 access key",
    )
    .await?;
    let secret_key = super::durable_cell_tests::store_cell_secret(
        secrets.as_ref(),
        organization_id,
        project_id,
        environment_id,
        "MCP S0 secret key",
    )
    .await?;

    let profile = super::durable_cell_tests::service_profile()?;
    let initial_definition =
        super::durable_cell_tests::application_definition(build_run_id, &profile, 'a')?;
    let create_arguments = json!({
        "projectId": project,
        "environmentId": environment,
        "name": "Tenant counters",
        "definitionAcl": initial_definition.canonical_acl(),
        "idempotencyKey": "mcp-cell-create"
    });
    let created = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                1,
                "a3s_cloud_durable_cell_applications_create",
                create_arguments.clone(),
            ),
        ))
        .await?;
    let created = response_json(&created)?;
    assert_eq!(created["result"]["structuredContent"]["code"], 201);
    let record = &created["result"]["structuredContent"]["data"]["record"];
    assert_eq!(record["application"]["aggregateVersion"], 1);
    assert_eq!(
        record["revision"]["definitionAcl"],
        initial_definition.canonical_acl()
    );
    let application_id = super::durable_cell_tests::required_cell_string(
        &record["application"]["applicationId"],
        "MCP application ID",
    )?;
    let initial_revision_id = super::durable_cell_tests::required_cell_string(
        &record["revision"]["revisionId"],
        "initial MCP revision ID",
    )?;

    let replayed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                2,
                "a3s_cloud_durable_cell_applications_create",
                create_arguments,
            ),
        ))
        .await?;
    let replayed = response_json(&replayed)?;
    assert_eq!(replayed["result"]["structuredContent"]["code"], 200);
    assert_eq!(
        replayed["result"]["structuredContent"]["data"]["replayed"],
        true
    );

    let listed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                3,
                "a3s_cloud_durable_cell_applications_list",
                json!({
                    "projectId": project,
                    "environmentId": environment,
                    "limit": 1
                }),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&listed)?["result"]["structuredContent"]["data"][0]["applicationId"],
        application_id
    );
    let fetched = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                4,
                "a3s_cloud_durable_cell_applications_get",
                json!({
                    "projectId": project,
                    "environmentId": environment,
                    "applicationId": application_id
                }),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&fetched)?["result"]["structuredContent"]["data"]["revision"]["revisionId"],
        initial_revision_id
    );

    for (id, name, expected_version, idempotency_key, desired_state) in [
        (
            5,
            "a3s_cloud_durable_cell_applications_stop",
            1,
            "mcp-cell-stop",
            "stopped",
        ),
        (
            6,
            "a3s_cloud_durable_cell_applications_start",
            2,
            "mcp-cell-start",
            "running",
        ),
    ] {
        let changed = app
            .call(mcp_request(
                Some(ADMIN_TOKEN),
                tool_call(
                    id,
                    name,
                    json!({
                        "projectId": project,
                        "environmentId": environment,
                        "applicationId": application_id,
                        "expectedVersion": expected_version,
                        "idempotencyKey": idempotency_key
                    }),
                ),
            ))
            .await?;
        let changed = response_json(&changed)?;
        assert_eq!(changed["result"]["structuredContent"]["code"], 200);
        assert_eq!(
            changed["result"]["structuredContent"]["data"]["record"]["application"]["desiredState"],
            desired_state
        );
    }

    let revised_definition =
        super::durable_cell_tests::application_definition(revised_build_run_id, &profile, 'b')?;
    let revised = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                7,
                "a3s_cloud_durable_cell_applications_revise",
                json!({
                    "projectId": project,
                    "environmentId": environment,
                    "applicationId": application_id,
                    "expectedVersion": 3,
                    "definitionAcl": revised_definition.canonical_acl(),
                    "idempotencyKey": "mcp-cell-revise"
                }),
            ),
        ))
        .await?;
    let revised = response_json(&revised)?;
    assert_eq!(revised["result"]["structuredContent"]["code"], 201);
    let revised_record = &revised["result"]["structuredContent"]["data"]["record"];
    assert_eq!(
        revised_record["revision"]["parentRevisionId"],
        initial_revision_id
    );
    let revision_id = super::durable_cell_tests::required_cell_string(
        &revised_record["revision"]["revisionId"],
        "revised MCP revision ID",
    )?;

    let revisions = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                8,
                "a3s_cloud_durable_cell_revisions_list",
                json!({
                    "projectId": project,
                    "environmentId": environment,
                    "applicationId": application_id
                }),
            ),
        ))
        .await?;
    let revisions = response_json(&revisions)?;
    assert_eq!(
        revisions["result"]["structuredContent"]["data"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    let initial_revision = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                9,
                "a3s_cloud_durable_cell_revisions_get",
                json!({
                    "projectId": project,
                    "environmentId": environment,
                    "applicationId": application_id,
                    "revisionId": initial_revision_id
                }),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&initial_revision)?["result"]["structuredContent"]["data"]["revisionNumber"],
        1
    );

    let storage = super::durable_cell_tests::deployment_binding(access_key, secret_key)?;
    let provider_acl = super::durable_cell_tests::provider_workload_acl(
        &application_id,
        &profile,
        access_key,
        secret_key,
        true,
    );
    let deployment_arguments = json!({
        "projectId": project,
        "environmentId": environment,
        "applicationId": application_id,
        "revisionId": revision_id,
        "serviceProfileAcl": profile.canonical_acl(),
        "storageProviderProfileAcl": super::durable_cell_tests::storage_provider_profile_acl(),
        "providerWorkloadAcl": provider_acl,
        "storageBindingAcl": storage.canonical_acl(),
        "idempotencyKey": "mcp-cell-deploy"
    });
    let deployed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                10,
                "a3s_cloud_durable_cell_deployments_create",
                deployment_arguments.clone(),
            ),
        ))
        .await?;
    let deployed = response_json(&deployed)?;
    assert_eq!(deployed["result"]["structuredContent"]["code"], 201);
    let deployment = &deployed["result"]["structuredContent"]["data"];
    assert_eq!(deployment["replayed"], false);
    assert_eq!(
        deployment["correlation"]["applicationRevisionId"],
        revision_id
    );
    assert_eq!(
        deployment["correlation"]["serviceProfileDigest"],
        profile.digest().as_str()
    );
    let serialized = serde_json::to_string(deployment)
        .map_err(|error| BootError::Internal(error.to_string()))?;
    assert!(!serialized.contains("ciphertext-"));
    assert!(!serialized.contains("AWS_ACCESS_KEY_ID"));
    assert!(!serialized.contains("AWS_SECRET_ACCESS_KEY"));
    assert!(!serialized.contains(&access_key.secret_id.to_string()));
    assert!(!serialized.contains(&secret_key.secret_id.to_string()));

    let replayed_deployment = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                11,
                "a3s_cloud_durable_cell_deployments_create",
                deployment_arguments,
            ),
        ))
        .await?;
    let replayed_deployment = response_json(&replayed_deployment)?;
    assert_eq!(
        replayed_deployment["result"]["structuredContent"]["code"],
        200
    );
    assert_eq!(
        replayed_deployment["result"]["structuredContent"]["data"]["replayed"],
        true
    );

    let missing_route_scope = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                12,
                "a3s_cloud_durable_cell_routes_publish",
                json!({
                    "projectId": project,
                    "environmentId": environment,
                    "applicationId": application_id,
                    "revisionId": revision_id,
                    "serviceProfileAcl": profile.canonical_acl(),
                    "gatewayScopeId": Uuid::now_v7(),
                    "domainClaimId": Uuid::now_v7(),
                    "hostname": "cells.example.com",
                    "pathPrefix": "/counters",
                    "idempotencyKey": "mcp-cell-route"
                }),
            ),
        ))
        .await?;
    let missing_route_scope = response_json(&missing_route_scope)?;
    assert_eq!(missing_route_scope["result"]["isError"], true);
    assert_eq!(
        missing_route_scope["result"]["structuredContent"]["code"],
        404
    );

    for (id, name, arguments) in [
        (
            13,
            "a3s_cloud_durable_cell_applications_list",
            json!({
                "projectId": project,
                "environmentId": environment,
                "limit": 201
            }),
        ),
        (
            14,
            "a3s_cloud_durable_cell_applications_get",
            json!({
                "projectId": project,
                "environmentId": environment,
                "applicationId": application_id,
                "organizationId": organization
            }),
        ),
        (
            15,
            "a3s_cloud_durable_cell_applications_stop",
            json!({
                "projectId": project,
                "environmentId": environment,
                "applicationId": application_id,
                "expectedVersion": 0,
                "idempotencyKey": "mcp-cell-invalid-version"
            }),
        ),
    ] {
        let rejected = app
            .call(mcp_request(
                Some(ADMIN_TOKEN),
                tool_call(id, name, arguments),
            ))
            .await?;
        assert_eq!(response_json(&rejected)?["error"]["code"], -32602, "{name}");
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
            kind: SearchResourceKind::PluginRegistry,
            id: allowed_id,
            title: "Cloud plugin registry".into(),
            description: "Plugin registry at https://registry.example/plugins/".into(),
            state: Some("active".into()),
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
            kind: SearchResourceKind::PluginRegistry,
            id: denied_id,
            title: "Cloud hidden plugin registry".into(),
            description: "Plugin registry at https://hidden.example/plugins/".into(),
            state: Some("active".into()),
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
    assert_eq!(
        body["result"]["structuredContent"]["data"][0]["kind"],
        "plugin_registry"
    );
    assert_eq!(
        body["result"]["structuredContent"]["data"][0]["href"],
        format!("#/organizations/{organization}/plugin-registries/{allowed_id}")
    );
    assert!(!body.to_string().contains(&denied_id.to_string()));
    assert!(!body.to_string().contains("Cloud hidden plugin registry"));
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

#[tokio::test]
async fn management_mcp_reuses_the_versioned_ontology_lifecycle() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "mcp-ontology", "Acme").await?;
    let project = create_project(&app, &organization, "mcp-ontology-project", "Knowledge").await?;
    let create_arguments = json!({
        "projectId": project,
        "acl": MCP_ONTOLOGY_ACL,
        "idempotencyKey": "mcp-ontology-create"
    });
    let created = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(1, "a3s_cloud_ontologies_create", create_arguments.clone()),
        ))
        .await?;
    let created = response_json(&created)?;
    assert_eq!(created["result"]["structuredContent"]["code"], 201);
    let data = &created["result"]["structuredContent"]["data"];
    let ontology_id = data["ontology"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP Ontology response has no ID".into()))?;
    let first_revision_id = data["revision"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP Ontology response has no revision ID".into()))?;

    let replay = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(2, "a3s_cloud_ontologies_create", create_arguments),
        ))
        .await?;
    assert_eq!(
        response_json(&replay)?["result"]["structuredContent"]["data"]["replayed"],
        true
    );

    let listed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                3,
                "a3s_cloud_ontologies_list",
                json!({"projectId": project}),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&listed)?["result"]["structuredContent"]["data"][0]["id"],
        ontology_id
    );

    let compatible_acl = MCP_ONTOLOGY_ACL.replace(
        "Deterministic W0.1 Ontology contract fixture",
        "MCP compatible revision",
    );
    let revised = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                4,
                "a3s_cloud_ontologies_revise",
                json!({
                    "ontologyId": ontology_id,
                    "acl": compatible_acl,
                    "expectedVersion": 1,
                    "idempotencyKey": "mcp-ontology-revise"
                }),
            ),
        ))
        .await?;
    let revised = response_json(&revised)?;
    assert_eq!(revised["result"]["structuredContent"]["code"], 201);
    let second_revision_id = revised["result"]["structuredContent"]["data"]["revision"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP revised Ontology has no revision ID".into()))?;

    let revisions = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                5,
                "a3s_cloud_ontology_revisions_list",
                json!({"ontologyId": ontology_id}),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&revisions)?["result"]["structuredContent"]["data"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );

    let revision = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                6,
                "a3s_cloud_ontology_revisions_get",
                json!({"ontologyId": ontology_id, "revisionId": second_revision_id}),
            ),
        ))
        .await?;
    assert!(
        response_json(&revision)?["result"]["structuredContent"]["data"]["canonicalAcl"]
            .as_str()
            .is_some_and(|acl| acl.contains("MCP compatible revision"))
    );

    let diff = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                7,
                "a3s_cloud_ontology_revisions_diff",
                json!({
                    "ontologyId": ontology_id,
                    "fromRevisionId": first_revision_id,
                    "toRevisionId": second_revision_id
                }),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&diff)?["result"]["structuredContent"]["data"]["breaking"],
        false
    );
    Ok(())
}

#[tokio::test]
async fn management_mcp_reuses_the_workflow_definition_goal_and_plan_lifecycle() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "mcp-workflow", "Acme").await?;
    let project = create_project(&app, &organization, "mcp-workflow-project", "Automation").await?;
    let ontology = app
        .call(post_acl(
            format!("/api/v1/organizations/{organization}/projects/{project}/ontologies"),
            "mcp-workflow-ontology",
            MCP_ONTOLOGY_ACL.as_bytes().to_vec(),
        ))
        .await?;
    let ontology = response_json(&ontology)?;

    let fixture =
        super::workflow_tests::workflow_fixture("MCP Workflow").map_err(BootError::Internal)?;
    let mut create_arguments = fixture.transport;
    let create_arguments_object = create_arguments
        .as_object_mut()
        .ok_or_else(|| BootError::Internal("Workflow publication is not an object".into()))?;
    create_arguments_object.insert("projectId".into(), json!(project));
    create_arguments_object.insert(
        "idempotencyKey".into(),
        json!("mcp-workflow-definition-create"),
    );
    let created = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                1,
                "a3s_cloud_workflow_definitions_create",
                create_arguments.clone(),
            ),
        ))
        .await?;
    let created = response_json(&created)?;
    assert_eq!(created["result"]["structuredContent"]["code"], 201);
    let definition = &created["result"]["structuredContent"]["data"];
    let definition_id = definition["workflowDefinition"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP WorkflowDefinition has no ID".into()))?;
    let revision_id = definition["revision"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP Workflow revision has no ID".into()))?;
    let workflow_digest = definition["revision"]["contentDigest"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP Workflow revision has no digest".into()))?;

    let replay = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(2, "a3s_cloud_workflow_definitions_create", create_arguments),
        ))
        .await?;
    assert_eq!(
        response_json(&replay)?["result"]["structuredContent"]["data"]["replayed"],
        true
    );
    let listed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                3,
                "a3s_cloud_workflow_definitions_list",
                json!({"projectId": project}),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&listed)?["result"]["structuredContent"]["data"][0]["id"],
        definition_id
    );
    let revision = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                4,
                "a3s_cloud_workflow_revisions_get",
                json!({
                    "workflowDefinitionId": definition_id,
                    "workflowRevisionId": revision_id
                }),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&revision)?["result"]["structuredContent"]["data"]["payloadCount"],
        4
    );

    let goal_contract = WorkflowGoalContract::from_spec(WorkflowGoalSpec {
        name: "MCP exact plan".into(),
        workflow_definition_id: WorkflowDefinitionId::from_uuid(
            Uuid::parse_str(definition_id)
                .map_err(|error| BootError::Internal(error.to_string()))?,
        ),
        workflow_revision_id: WorkflowRevisionId::from_uuid(
            Uuid::parse_str(revision_id).map_err(|error| BootError::Internal(error.to_string()))?,
        ),
        workflow_digest: Sha256Digest::parse(workflow_digest).map_err(BootError::Internal)?,
        ontology_id: OntologyId::from_uuid(
            Uuid::parse_str(
                ontology["data"]["ontology"]["id"]
                    .as_str()
                    .ok_or_else(|| BootError::Internal("Ontology ID is missing".into()))?,
            )
            .map_err(|error| BootError::Internal(error.to_string()))?,
        ),
        ontology_revision_id: OntologyRevisionId::from_uuid(
            Uuid::parse_str(
                ontology["data"]["revision"]["id"]
                    .as_str()
                    .ok_or_else(|| BootError::Internal("Ontology revision ID is missing".into()))?,
            )
            .map_err(|error| BootError::Internal(error.to_string()))?,
        ),
        ontology_digest: Sha256Digest::parse(
            ontology["data"]["revision"]["contentDigest"]
                .as_str()
                .ok_or_else(|| BootError::Internal("Ontology digest is missing".into()))?,
        )
        .map_err(BootError::Internal)?,
        environment_id: None,
        input: json!({"caseId": "MCP-42"}),
    })
    .map_err(BootError::Internal)?;
    let goal = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                5,
                "a3s_cloud_workflow_goals_create",
                json!({
                    "projectId": project,
                    "acl": goal_contract.canonical_acl(),
                    "idempotencyKey": "mcp-workflow-goal-create"
                }),
            ),
        ))
        .await?;
    let goal = response_json(&goal)?;
    assert_eq!(goal["result"]["structuredContent"]["code"], 201);
    let goal_data = &goal["result"]["structuredContent"]["data"];
    let goal_id = goal_data["goal"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP WorkflowGoal has no ID".into()))?;
    let plan_revision_id = goal_data["planRevision"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP PlanRevision has no ID".into()))?;
    let plan = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                6,
                "a3s_cloud_workflow_plan_revisions_get",
                json!({
                    "workflowGoalId": goal_id,
                    "planRevisionId": plan_revision_id
                }),
            ),
        ))
        .await?;
    let plan = response_json(&plan)?;
    assert_eq!(
        plan["result"]["structuredContent"]["data"]["digest"],
        goal_data["goal"]["planDigest"]
    );
    assert_eq!(
        plan["result"]["structuredContent"]["data"]["plan"]["steps"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );

    let start_arguments = json!({
        "projectId": project,
        "workflowGoalId": goal_id,
        "planRevisionId": plan_revision_id,
        "timeoutSeconds": 60,
        "idempotencyKey": "mcp-workflow-run-start"
    });
    let started = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(7, "a3s_cloud_workflow_runs_start", start_arguments.clone()),
        ))
        .await?;
    let started = response_json(&started)?;
    assert_eq!(started["result"]["structuredContent"]["code"], 202);
    let workflow_run_id = started["result"]["structuredContent"]["data"]["workflowRun"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP WorkflowRun has no ID".into()))?;
    assert_eq!(
        started["result"]["structuredContent"]["data"]["workflowRun"]["status"],
        "pending"
    );
    let start_replay = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(8, "a3s_cloud_workflow_runs_start", start_arguments),
        ))
        .await?;
    let start_replay = response_json(&start_replay)?;
    assert_eq!(start_replay["result"]["structuredContent"]["code"], 200);
    assert_eq!(
        start_replay["result"]["structuredContent"]["data"]["replayed"],
        true
    );
    assert_eq!(
        start_replay["result"]["structuredContent"]["data"]["workflowRun"]["id"],
        workflow_run_id
    );

    let listed_runs = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                9,
                "a3s_cloud_workflow_runs_list",
                json!({"projectId": project, "limit": 1}),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&listed_runs)?["result"]["structuredContent"]["data"][0]["id"],
        workflow_run_id
    );
    for (id, name, arguments) in [
        (
            10,
            "a3s_cloud_workflow_runs_get",
            json!({"workflowRunId": workflow_run_id}),
        ),
        (
            11,
            "a3s_cloud_workflow_runs_wait",
            json!({"workflowRunId": workflow_run_id, "timeoutSeconds": 0}),
        ),
    ] {
        let response = app
            .call(mcp_request(
                Some(ADMIN_TOKEN),
                tool_call(id, name, arguments),
            ))
            .await?;
        assert_eq!(
            response_json(&response)?["result"]["structuredContent"]["data"]["id"],
            workflow_run_id,
            "{name}"
        );
    }
    let history = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                12,
                "a3s_cloud_workflow_run_history_get",
                json!({"workflowRunId": workflow_run_id, "limit": 10}),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&history)?["result"]["structuredContent"]["data"]["events"],
        json!([])
    );
    let pending_output = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                13,
                "a3s_cloud_workflow_run_output_get",
                json!({"workflowRunId": workflow_run_id}),
            ),
        ))
        .await?;
    let pending_output = response_json(&pending_output)?;
    assert_eq!(pending_output["result"]["structuredContent"]["code"], 409);
    assert_eq!(pending_output["result"]["isError"], true);

    let cancel_arguments = json!({
        "workflowRunId": workflow_run_id,
        "reason": "operator request",
        "idempotencyKey": "mcp-workflow-run-cancel"
    });
    let cancelled = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                14,
                "a3s_cloud_workflow_runs_cancel",
                cancel_arguments.clone(),
            ),
        ))
        .await?;
    let cancelled = response_json(&cancelled)?;
    assert_eq!(cancelled["result"]["structuredContent"]["code"], 202);
    assert_eq!(
        cancelled["result"]["structuredContent"]["data"]["workflowRun"]["status"],
        "cancelling"
    );
    let cancel_replay = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(15, "a3s_cloud_workflow_runs_cancel", cancel_arguments),
        ))
        .await?;
    assert_eq!(
        response_json(&cancel_replay)?["result"]["structuredContent"]["data"]["replayed"],
        true
    );
    Ok(())
}

#[tokio::test]
async fn management_mcp_reuses_the_form_draft_and_release_lifecycle() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "mcp-form", "Acme").await?;
    let project = create_project(&app, &organization, "mcp-form-project", "Forms").await?;

    let mut create_arguments =
        super::forms_tests::form_draft("Approval", "Manager approval", false);
    let create_arguments_object = create_arguments
        .as_object_mut()
        .ok_or_else(|| BootError::Internal("Form draft fixture is not an object".into()))?;
    create_arguments_object.insert("projectId".into(), json!(project));
    create_arguments_object.insert("idempotencyKey".into(), json!("mcp-form-create"));

    let created = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(1, "a3s_cloud_forms_create", create_arguments.clone()),
        ))
        .await?;
    let created = response_json(&created)?;
    assert_eq!(created["result"]["structuredContent"]["code"], 201);
    assert_eq!(
        created["result"]["structuredContent"]["data"]["form"]["aggregateVersion"],
        1
    );
    let form_id = created["result"]["structuredContent"]["data"]["form"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP Form response has no ID".into()))?
        .to_owned();

    let replayed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(2, "a3s_cloud_forms_create", create_arguments.clone()),
        ))
        .await?;
    let replayed = response_json(&replayed)?;
    assert_eq!(replayed["result"]["structuredContent"]["code"], 200);
    assert_eq!(
        replayed["result"]["structuredContent"]["data"]["replayed"],
        true
    );
    assert_eq!(
        replayed["result"]["structuredContent"]["data"]["form"]["id"],
        form_id
    );

    let mut changed_create_arguments = create_arguments;
    changed_create_arguments
        .as_object_mut()
        .expect("Form draft fixture object")
        .insert("name".into(), json!("Changed intent"));
    let changed_replay = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(3, "a3s_cloud_forms_create", changed_create_arguments),
        ))
        .await?;
    let changed_replay = response_json(&changed_replay)?;
    assert_eq!(changed_replay["result"]["structuredContent"]["code"], 409);
    assert_eq!(changed_replay["result"]["isError"], true);

    let listed = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(4, "a3s_cloud_forms_list", json!({"projectId": project})),
        ))
        .await?;
    let listed = response_json(&listed)?;
    assert_eq!(
        listed["result"]["structuredContent"]["data"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        listed["result"]["structuredContent"]["data"][0]["id"],
        form_id
    );

    let fetched = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(5, "a3s_cloud_forms_get", json!({"formId": form_id})),
        ))
        .await?;
    assert_eq!(
        response_json(&fetched)?["result"]["structuredContent"]["data"]["name"],
        "Approval"
    );

    let unknown_argument = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                6,
                "a3s_cloud_forms_get",
                json!({"formId": form_id, "organizationId": organization}),
            ),
        ))
        .await?;
    assert_eq!(response_json(&unknown_argument)?["error"]["code"], -32602);
    let invalid_document = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                6,
                "a3s_cloud_forms_create",
                json!({
                    "projectId": project,
                    "name": "Invalid",
                    "document": [],
                    "idempotencyKey": "mcp-form-invalid-document"
                }),
            ),
        ))
        .await?;
    assert_eq!(response_json(&invalid_document)?["error"]["code"], -32602);

    let mut revise_arguments = super::forms_tests::form_draft(
        "Approval request",
        "Manager approval with a required reason",
        true,
    );
    let revise_arguments_object = revise_arguments
        .as_object_mut()
        .ok_or_else(|| BootError::Internal("Form revision fixture is not an object".into()))?;
    revise_arguments_object.insert("formId".into(), json!(form_id));
    revise_arguments_object.insert("expectedVersion".into(), json!(1));
    revise_arguments_object.insert("idempotencyKey".into(), json!("mcp-form-revise"));
    let revised = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(7, "a3s_cloud_forms_revise", revise_arguments.clone()),
        ))
        .await?;
    let revised = response_json(&revised)?;
    assert_eq!(revised["result"]["structuredContent"]["code"], 201);
    assert_eq!(
        revised["result"]["structuredContent"]["data"]["form"]["aggregateVersion"],
        2
    );

    let mut stale_arguments = revise_arguments.clone();
    stale_arguments
        .as_object_mut()
        .expect("Form revision fixture object")
        .insert("idempotencyKey".into(), json!("mcp-form-revise-stale"));
    let stale = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(8, "a3s_cloud_forms_revise", stale_arguments),
        ))
        .await?;
    assert_eq!(
        response_json(&stale)?["result"]["structuredContent"]["code"],
        409
    );

    let invalid_version = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                9,
                "a3s_cloud_forms_revise",
                json!({
                    "formId": form_id,
                    "name": "Invalid version",
                    "document": {},
                    "expectedVersion": 0,
                    "idempotencyKey": "mcp-form-zero-version"
                }),
            ),
        ))
        .await?;
    assert_eq!(response_json(&invalid_version)?["error"]["code"], -32602);

    let publish_arguments = json!({
        "formId": form_id,
        "expectedVersion": 2,
        "idempotencyKey": "mcp-form-publish"
    });
    let published = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                10,
                "a3s_cloud_form_releases_publish",
                publish_arguments.clone(),
            ),
        ))
        .await?;
    let published = response_json(&published)?;
    assert_eq!(published["result"]["structuredContent"]["code"], 201);
    assert_eq!(
        published["result"]["structuredContent"]["data"]["form"]["aggregateVersion"],
        3
    );
    assert_eq!(
        published["result"]["structuredContent"]["data"]["release"]["sourceDraftVersion"],
        2
    );
    let release_id = published["result"]["structuredContent"]["data"]["release"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP Form release has no ID".into()))?
        .to_owned();

    let publish_replay = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(11, "a3s_cloud_form_releases_publish", publish_arguments),
        ))
        .await?;
    let publish_replay = response_json(&publish_replay)?;
    assert_eq!(publish_replay["result"]["structuredContent"]["code"], 200);
    assert_eq!(
        publish_replay["result"]["structuredContent"]["data"]["replayed"],
        true
    );
    assert_eq!(
        publish_replay["result"]["structuredContent"]["data"]["release"]["id"],
        release_id
    );

    let historical_revise_replay = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(12, "a3s_cloud_forms_revise", revise_arguments),
        ))
        .await?;
    let historical_revise_replay = response_json(&historical_revise_replay)?;
    assert_eq!(
        historical_revise_replay["result"]["structuredContent"]["data"]["form"]["aggregateVersion"],
        2
    );
    assert_eq!(
        historical_revise_replay["result"]["structuredContent"]["data"]["replayed"],
        true
    );

    let releases = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                13,
                "a3s_cloud_form_releases_list",
                json!({"formId": form_id}),
            ),
        ))
        .await?;
    let releases = response_json(&releases)?;
    assert_eq!(
        releases["result"]["structuredContent"]["data"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        releases["result"]["structuredContent"]["data"][0]["id"],
        release_id
    );

    let release = app
        .call(mcp_request(
            Some(ADMIN_TOKEN),
            tool_call(
                14,
                "a3s_cloud_form_releases_get",
                json!({"formId": form_id, "releaseId": release_id}),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&release)?["result"]["structuredContent"]["data"]["id"],
        release_id
    );
    Ok(())
}

#[tokio::test]
async fn management_mcp_form_tools_do_not_cross_tenant_boundaries() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let acme = bootstrap_organization(&app, "mcp-form-tenant", "Acme").await?;
    let beta = create_organization(&app, "mcp-form-tenant-beta", "Beta").await?;
    let acme_project = create_project(&app, &acme, "mcp-form-acme-project", "Acme Forms").await?;
    let beta_project = create_project(&app, &beta, "mcp-form-beta-project", "Beta Forms").await?;
    create_api_token(
        &app,
        &acme,
        "mcp-form-tenant-token",
        "Acme Form author",
        MCP_FORM_TOKEN,
        &[ApiTokenScope::CLOUD_READ, ApiTokenScope::FORM_WRITE],
        None,
    )
    .await?;

    let beta_form = app
        .call(post_json(
            format!("/api/v1/organizations/{beta}/projects/{beta_project}/forms"),
            "mcp-form-beta-create",
            super::forms_tests::form_draft("Beta approval", "Foreign Form", false),
        ))
        .await?;
    assert_eq!(beta_form.status(), 201);
    let beta_form = response_json(&beta_form)?;
    let beta_form_id = beta_form["data"]["form"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("foreign Form response has no ID".into()))?;

    let hidden_get = app
        .call(mcp_request(
            Some(MCP_FORM_TOKEN),
            tool_call(1, "a3s_cloud_forms_get", json!({"formId": beta_form_id})),
        ))
        .await?;
    let hidden_get = response_json(&hidden_get)?;
    assert_eq!(hidden_get["result"]["structuredContent"]["code"], 404);
    assert!(!hidden_get.to_string().contains("Beta approval"));

    let hidden_list = app
        .call(mcp_request(
            Some(MCP_FORM_TOKEN),
            tool_call(
                2,
                "a3s_cloud_forms_list",
                json!({"projectId": beta_project}),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&hidden_list)?["result"]["structuredContent"]["data"],
        json!([])
    );

    let mut cross_tenant_create =
        super::forms_tests::form_draft("Injected", "Must not commit", false);
    let cross_tenant_create = cross_tenant_create
        .as_object_mut()
        .ok_or_else(|| BootError::Internal("Form draft fixture is not an object".into()))?;
    cross_tenant_create.insert("projectId".into(), json!(beta_project));
    cross_tenant_create.insert(
        "idempotencyKey".into(),
        json!("mcp-form-cross-tenant-create"),
    );
    let denied_create = app
        .call(mcp_request(
            Some(MCP_FORM_TOKEN),
            tool_call(
                3,
                "a3s_cloud_forms_create",
                Value::Object(cross_tenant_create.clone()),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&denied_create)?["result"]["structuredContent"]["code"],
        404
    );

    let acme_forms = app
        .call(mcp_request(
            Some(MCP_FORM_TOKEN),
            tool_call(
                4,
                "a3s_cloud_forms_list",
                json!({"projectId": acme_project}),
            ),
        ))
        .await?;
    assert_eq!(
        response_json(&acme_forms)?["result"]["structuredContent"]["data"],
        json!([])
    );
    Ok(())
}

fn listed_tool<'a>(body: &'a Value, name: &str) -> Result<&'a Value> {
    body["result"]["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|tool| tool["name"] == name)
        .ok_or_else(|| BootError::Internal(format!("Management tool {name} is missing")))
}

fn discover_request(id: u64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "server/discover",
        "params": {}
    })
}

fn mcp_request(token: Option<&str>, body: Value) -> BootRequest {
    mcp_request_with_version(token, body, MCP_PROTOCOL_VERSION)
}

fn mcp_request_with_version(token: Option<&str>, body: Value, version: &str) -> BootRequest {
    let body = with_request_metadata_version(body, version);
    let method = body
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("server/discover")
        .to_owned();
    let name = body
        .get("params")
        .and_then(Value::as_object)
        .and_then(|params| params.get("name"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut request = raw_mcp_request(token, body)
        .with_header("mcp-protocol-version", version)
        .with_header("mcp-method", method);
    if let Some(name) = name {
        request = request.with_header("mcp-name", name);
    }
    request
}

fn request_with_metadata(
    token: Option<&str>,
    mut body: Value,
    metadata: Value,
    version: &str,
) -> BootRequest {
    let params = body
        .as_object_mut()
        .and_then(|body| {
            body.entry("params")
                .or_insert_with(|| json!({}))
                .as_object_mut()
        })
        .expect("MCP test request params must be an object");
    params.insert("_meta".into(), metadata);
    let method = body["method"].as_str().expect("MCP test method").to_owned();
    let mut request = raw_mcp_request(token, body)
        .with_header("mcp-protocol-version", version)
        .with_header("mcp-method", method);
    if let Some(name) = request_name(request.body()) {
        request = request.with_header("mcp-name", name);
    }
    request
}

fn raw_mcp_request(token: Option<&str>, body: Value) -> BootRequest {
    let request = BootRequest::new(HttpMethod::Post, MCP_PATH)
        .with_header("content-type", "application/json")
        .with_header("accept", "application/json, text/event-stream")
        .with_body(body.to_string().into_bytes());
    match token {
        Some(token) => request.with_header("authorization", format!("Bearer {token}")),
        None => request,
    }
}

fn with_request_metadata(body: Value) -> Value {
    with_request_metadata_version(body, MCP_PROTOCOL_VERSION)
}

fn with_request_metadata_version(mut body: Value, version: &str) -> Value {
    let Some(object) = body.as_object_mut() else {
        return body;
    };
    let params = object
        .entry("params")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("MCP test request params must be an object");
    params.insert(
        "_meta".into(),
        json!({
            "io.modelcontextprotocol/protocolVersion": version,
            "io.modelcontextprotocol/clientInfo": {
                "name": "a3s-cloud-test",
                "version": "1.0.0"
            },
            "io.modelcontextprotocol/clientCapabilities": {}
        }),
    );
    body
}

fn request_name(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(body)
        .ok()?
        .get("params")?
        .get("name")?
        .as_str()
        .map(str::to_owned)
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
    let body = response_json(&response)?;
    assert_eq!(body["result"]["resultType"], "complete");
    assert_eq!(
        body["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "a3s-cloud"
    );
    Ok(body)
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
