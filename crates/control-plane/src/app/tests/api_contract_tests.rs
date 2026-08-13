use super::*;
use crate::presentation::{
    generate_openapi_contract, API_CONTRACT_VERSION_HEADER, API_MAJOR_VERSION,
    MINIMUM_DEPRECATION_DAYS, OPENAPI_CONTRACT_VERSION, OPENAPI_PUBLIC_PATH,
};
use a3s_use_extension::{
    plugin_catalog_host_input_schema, plugin_catalog_inspection_input_schema,
    plugin_catalog_search_input_schema,
};
use std::collections::BTreeSet;

const OPENAPI_SOURCE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../openapi/v1.json");

#[tokio::test]
async fn openapi_contract_is_public_raw_and_versioned() -> Result<()> {
    let app = contract_test_application()?;
    let response = app
        .call(BootRequest::new(HttpMethod::Get, OPENAPI_PUBLIC_PATH))
        .await?;

    assert_eq!(response.status(), 200);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(
        response.header(API_CONTRACT_VERSION_HEADER),
        Some(OPENAPI_CONTRACT_VERSION)
    );
    assert!(response
        .header("x-request-id")
        .is_some_and(|value| Uuid::parse_str(value).is_ok()));
    assert_eq!(
        response.header("cache-control"),
        Some("public, max-age=300")
    );

    let document = response_json(&response)?;
    assert_eq!(document["openapi"], "3.0.3");
    assert_eq!(document["info"]["version"], OPENAPI_CONTRACT_VERSION);
    assert_eq!(document["x-a3s-api-major-version"], API_MAJOR_VERSION);
    assert_eq!(
        document["x-a3s-minimum-deprecation-days"],
        MINIMUM_DEPRECATION_DAYS
    );
    assert!(document.get("data").is_none());
    assert!(document["paths"]
        .as_object()
        .is_some_and(|paths| !paths.is_empty()));
    Ok(())
}

#[test]
fn committed_openapi_snapshot_matches_the_resolved_route_contract() -> Result<()> {
    let app = contract_test_application()?;
    let generated = generate_openapi_contract(&app)?;
    let rendered = format!(
        "{}\n",
        serde_json::to_string_pretty(&generated)
            .map_err(|error| BootError::Internal(error.to_string()))?
    );

    if std::env::var_os("A3S_CLOUD_UPDATE_OPENAPI").is_some() {
        std::fs::write(OPENAPI_SOURCE_PATH, rendered)
            .map_err(|error| BootError::Internal(error.to_string()))?;
        return Ok(());
    }

    let committed: Value = serde_json::from_str(include_str!("../../../../../openapi/v1.json"))
        .map_err(|error| BootError::Internal(error.to_string()))?;
    assert_eq!(generated, committed);
    Ok(())
}

#[test]
fn generated_openapi_operations_have_stable_ids_security_and_envelopes() -> Result<()> {
    let app = contract_test_application()?;
    let document = generate_openapi_contract(&app)?;
    let paths = document["paths"]
        .as_object()
        .ok_or_else(|| BootError::Internal("OpenAPI paths are missing".into()))?;
    let mut operation_ids = BTreeSet::new();
    let mut operation_count = 0;

    for (path, path_item) in paths {
        let path_item = path_item
            .as_object()
            .ok_or_else(|| BootError::Internal(format!("OpenAPI path `{path}` is invalid")))?;
        for method in ["delete", "get", "patch", "post", "put"] {
            let Some(operation) = path_item.get(method) else {
                continue;
            };
            operation_count += 1;
            let operation_id = operation["operationId"].as_str().ok_or_else(|| {
                BootError::Internal(format!("OpenAPI operation `{method} {path}` has no ID"))
            })?;
            assert!(operation_ids.insert(operation_id.to_owned()));
            assert!(operation["tags"]
                .as_array()
                .is_some_and(|tags| !tags.is_empty()));
            assert!(operation["security"].is_array());
            assert!(operation["responses"].get("500").is_some());
        }
    }

    assert!(
        operation_count >= 60,
        "unexpectedly small public REST surface"
    );
    assert_eq!(document["paths"]["/platform"]["get"]["security"], json!([]));
    assert_eq!(
        document["paths"]["/organizations"]["get"]["security"],
        json!([{ "bearerAuth": [] }])
    );
    assert!(document["paths"]["/organizations"]["post"]["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "idempotency-key"
                && parameter["in"] == "header"
                && parameter["required"] == true
        })));
    let memberships = &document["paths"]["/organizations/{organization_id}/memberships"];
    assert_eq!(memberships["get"]["tags"], json!(["Identity"]));
    assert!(memberships["get"]["responses"]["200"].is_object());
    assert_eq!(memberships["post"]["tags"], json!(["Identity"]));
    assert!(memberships["post"]["requestBody"]["content"]["application/json"].is_object());
    assert!(memberships["post"]["responses"]["200"].is_object());
    assert!(memberships["post"]["responses"]["201"].is_object());
    assert!(memberships["post"]["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "idempotency-key"
                && parameter["in"] == "header"
                && parameter["required"] == true
        })));
    let oidc_login = &document["paths"]["/identity/oidc/{provider_key}/login"]["get"];
    assert_eq!(oidc_login["tags"], json!(["Identity"]));
    assert_eq!(oidc_login["security"], json!([]));
    assert!(oidc_login["responses"]["303"].is_object());
    assert_eq!(oidc_login["x-a3s-oauth-cookie-bound"], true);
    assert!(oidc_login["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "organization_id"
                && parameter["in"] == "query"
                && parameter["required"] == true
                && parameter["schema"]["format"] == "uuid"
        })));
    let oidc_link = &document["paths"]
        ["/organizations/{organization_id}/identity/oidc/{provider_key}/link"]["post"];
    assert_eq!(oidc_link["tags"], json!(["Identity"]));
    assert!(oidc_link["responses"]["200"].is_object());
    assert!(oidc_link.get("requestBody").is_none());
    assert!(oidc_link["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters
            .iter()
            .all(|parameter| parameter["name"] != "idempotency-key")));
    let oidc_callback = &document["paths"]["/identity/oidc/{provider_key}/callback"]["get"];
    assert_eq!(oidc_callback["security"], json!([]));
    assert!(oidc_callback["responses"]["200"].is_object());
    assert_eq!(oidc_callback["x-a3s-oauth-cookie-bound"], true);
    let membership =
        &document["paths"]["/organizations/{organization_id}/memberships/{membership_id}"];
    assert_eq!(membership["get"]["tags"], json!(["Identity"]));
    assert!(membership["get"]["responses"]["200"].is_object());
    for path in [
        "/organizations/{organization_id}/memberships/{membership_id}/role",
        "/organizations/{organization_id}/memberships/{membership_id}/revocation",
    ] {
        let operation = &document["paths"][path]["post"];
        assert_eq!(operation["tags"], json!(["Identity"]));
        assert!(operation["requestBody"]["content"]["application/json"].is_object());
        assert!(operation["responses"]["200"].is_object());
        assert!(operation["parameters"]
            .as_array()
            .is_some_and(|parameters| parameters.iter().any(|parameter| {
                parameter["name"] == "idempotency-key"
                    && parameter["in"] == "header"
                    && parameter["required"] == true
            })));
    }
    let membership_invitations =
        &document["paths"]["/organizations/{organization_id}/membership-invitations"];
    assert_eq!(membership_invitations["get"]["tags"], json!(["Identity"]));
    assert!(membership_invitations["get"]["responses"]["200"].is_object());
    let invitation_create = &membership_invitations["post"];
    assert_eq!(invitation_create["tags"], json!(["Identity"]));
    let invitation_create_schema =
        &invitation_create["requestBody"]["content"]["application/json"]["schema"];
    assert_eq!(invitation_create_schema["additionalProperties"], false);
    assert_eq!(
        invitation_create_schema["properties"]["principalId"]["format"],
        "uuid"
    );
    assert_eq!(
        invitation_create_schema["properties"]["expiresAt"]["format"],
        "date-time"
    );
    assert!(invitation_create["responses"]["200"].is_object());
    assert!(invitation_create["responses"]["201"].is_object());
    assert!(invitation_create["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "idempotency-key"
                && parameter["in"] == "header"
                && parameter["required"] == true
        })));
    let invitation = &document["paths"]
        ["/organizations/{organization_id}/membership-invitations/{invitation_id}"]["get"];
    assert_eq!(invitation["tags"], json!(["Identity"]));
    assert!(invitation["responses"]["200"].is_object());
    let my_invitations = &document["paths"]["/membership-invitations"]["get"];
    assert_eq!(my_invitations["tags"], json!(["Identity"]));
    assert!(my_invitations["responses"]["200"].is_object());
    for path in [
        "/membership-invitations/{invitation_id}/acceptance",
        "/organizations/{organization_id}/membership-invitations/{invitation_id}/revocation",
    ] {
        let operation = &document["paths"][path]["post"];
        assert_eq!(operation["tags"], json!(["Identity"]));
        assert_eq!(
            operation["requestBody"]["content"]["application/json"]["schema"]
                ["additionalProperties"],
            false
        );
        assert_eq!(
            operation["requestBody"]["content"]["application/json"]["schema"]["properties"]
                ["expectedVersion"]["minimum"],
            1
        );
        assert!(operation["responses"]["200"].is_object());
        assert!(operation["parameters"]
            .as_array()
            .is_some_and(|parameters| parameters.iter().any(|parameter| {
                parameter["name"] == "idempotency-key"
                    && parameter["in"] == "header"
                    && parameter["required"] == true
            })));
    }
    assert!(
        document["paths"]["/membership-invitations/{invitation_id}/acceptance"]["post"]
            ["responses"]["201"]
            .is_object()
    );
    let audit_records = &document["paths"]["/organizations/{organization_id}/audit-records"]["get"];
    assert_eq!(audit_records["tags"], json!(["Audit"]));
    assert!(audit_records["responses"]["200"].is_object());
    assert!(audit_records["responses"]["403"].is_object());
    let audit_parameters = audit_records["parameters"]
        .as_array()
        .expect("audit query parameters");
    for (name, format) in [
        ("actorPrincipalId", Some("uuid")),
        ("aggregateId", Some("uuid")),
        ("requestId", Some("uuid")),
        ("action", None),
        ("from", Some("date-time")),
        ("to", Some("date-time")),
        ("cursor", None),
        ("limit", None),
    ] {
        let parameter = audit_parameters
            .iter()
            .find(|parameter| parameter["name"] == name)
            .unwrap_or_else(|| panic!("missing audit query parameter `{name}`"));
        assert_eq!(parameter["in"], "query");
        assert_eq!(parameter["required"], false);
        if let Some(format) = format {
            assert_eq!(parameter["schema"]["format"], format);
        }
    }
    let audit_limit = audit_parameters
        .iter()
        .find(|parameter| parameter["name"] == "limit")
        .expect("audit limit parameter");
    assert_eq!(audit_limit["schema"]["minimum"], 1);
    assert_eq!(audit_limit["schema"]["maximum"], 200);
    assert_eq!(audit_limit["schema"]["default"], 50);
    let audit_cursor = audit_parameters
        .iter()
        .find(|parameter| parameter["name"] == "cursor")
        .expect("audit cursor parameter");
    assert_eq!(audit_cursor["schema"]["maxLength"], 128);
    let resource_grants = &document["paths"]
        ["/organizations/{organization_id}/memberships/{membership_id}/resource-grants"];
    assert_eq!(resource_grants["get"]["tags"], json!(["Identity"]));
    assert!(resource_grants["get"]["responses"]["200"].is_object());
    assert_eq!(resource_grants["post"]["tags"], json!(["Identity"]));
    assert_eq!(
        resource_grants["post"]["requestBody"]["content"]["application/json"]["schema"]
            ["properties"]["scope"]["discriminator"]["propertyName"],
        "kind"
    );
    assert!(resource_grants["post"]["responses"]["201"].is_object());
    let resource_grant = &document["paths"]
        ["/organizations/{organization_id}/resource-grants/{resource_grant_id}"]["get"];
    assert_eq!(resource_grant["tags"], json!(["Identity"]));
    let resource_grant_revocation = &document["paths"]
        ["/organizations/{organization_id}/resource-grants/{resource_grant_id}/revocation"]["post"];
    assert_eq!(resource_grant_revocation["tags"], json!(["Identity"]));
    assert_eq!(
        resource_grant_revocation["requestBody"]["content"]["application/json"]["schema"]
            ["properties"]["expectedVersion"]["minimum"],
        1
    );
    let plugin_registries =
        &document["paths"]["/organizations/{organization_id}/plugin-registries"];
    assert_eq!(plugin_registries["get"]["tags"], json!(["Plugins"]));
    assert!(plugin_registries["get"]["responses"]["200"].is_object());
    let plugin_registry = &document["paths"]
        ["/organizations/{organization_id}/plugin-registries/{registry_id}"]["get"];
    assert_eq!(plugin_registry["tags"], json!(["Plugins"]));
    let node_pools = &document["paths"]["/organizations/{organization_id}/node-pools"];
    assert_eq!(node_pools["get"]["tags"], json!(["Fleet"]));
    assert_eq!(node_pools["post"]["tags"], json!(["Fleet"]));
    assert_eq!(
        node_pools["post"]["requestBody"]["content"]["application/json"]["schema"]["properties"]
            ["memberNodeIds"]["maxItems"],
        10_000
    );
    assert!(node_pools["post"]["responses"]["201"].is_object());
    let member_removal = &document["paths"]
        ["/organizations/{organization_id}/node-pools/{node_pool_id}/members/removal"]["post"];
    assert_eq!(member_removal["tags"], json!(["Fleet"]));
    assert_eq!(
        member_removal["requestBody"]["content"]["application/json"]["schema"]["properties"]
            ["memberNodeIds"]["maxItems"],
        10_000
    );
    let maintenance = &document["paths"]
        ["/organizations/{organization_id}/node-pools/{node_pool_id}/maintenance"]["post"];
    assert_eq!(maintenance["tags"], json!(["Fleet"]));
    assert_eq!(
        maintenance["requestBody"]["content"]["application/json"]["schema"]["properties"]["reason"]
            ["maxLength"],
        1_024
    );
    let plugin_search_schema = &document["paths"]
        ["/organizations/{organization_id}/plugin-registries/{registry_id}/catalog/search"]["post"]
        ["requestBody"]["content"]["application/json"]["schema"];
    assert_eq!(
        plugin_search_schema["properties"]["host"],
        plugin_catalog_host_input_schema()
    );
    assert_eq!(
        plugin_search_schema["properties"]["search"],
        plugin_catalog_search_input_schema()
    );
    let plugin_inspection_schema = &document["paths"]
        ["/organizations/{organization_id}/plugin-registries/{registry_id}/catalog/inspect"]
        ["post"]["requestBody"]["content"]["application/json"]["schema"];
    let canonical_inspection = plugin_catalog_inspection_input_schema();
    assert_eq!(
        plugin_inspection_schema["properties"]["host"],
        plugin_catalog_host_input_schema()
    );
    for property in ["packageId", "version", "channel"] {
        assert_eq!(
            plugin_inspection_schema["properties"][property],
            canonical_inspection["properties"][property],
            "{property}"
        );
    }
    for path in [
        "/organizations/{organization_id}/plugin-registries/{registry_id}/catalog/search",
        "/organizations/{organization_id}/plugin-registries/{registry_id}/catalog/cache/search",
        "/organizations/{organization_id}/plugin-registries/{registry_id}/catalog/inspect",
        "/organizations/{organization_id}/plugin-registries/{registry_id}/catalog/cache/inspect",
    ] {
        let operation = &document["paths"][path]["post"];
        assert_eq!(operation["tags"], json!(["Plugins"]));
        assert!(operation["requestBody"]["content"]["application/json"].is_object());
        assert!(operation["responses"]["200"].is_object());
        assert!(!operation["parameters"]
            .as_array()
            .is_some_and(|parameters| parameters.iter().any(|parameter| {
                parameter["name"] == "idempotency-key" && parameter["in"] == "header"
            })));
    }
    let log_stream = &document["paths"]
        ["/organizations/{organization_id}/build-runs/{build_run_id}/logs/stream"]["get"];
    assert_eq!(
        log_stream["responses"]["200"]["$ref"],
        "#/components/responses/SseSuccess200"
    );
    assert!(log_stream["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "last-event-id" && parameter["in"] == "header"
        })));
    assert!(document["paths"]
        .get("/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/workloads")
        .and_then(|path| path.get("post"))
        .and_then(|operation| operation.get("requestBody"))
        .and_then(|body| body.get("content"))
        .and_then(|content| content.get("application/vnd.a3s.acl"))
        .is_some());
    let mcp_profile = &document["paths"]
        ["/organizations/{organization_id}/assets/{asset_id}/releases/{asset_release_id}/mcp-service-profile"];
    assert!(mcp_profile["get"]["responses"]["200"].is_object());
    assert!(mcp_profile["get"]["responses"].get("413").is_none());
    assert!(mcp_profile["get"]["responses"].get("415").is_none());
    assert!(mcp_profile["post"]["requestBody"]["content"]["application/vnd.a3s.acl"].is_object());
    assert_eq!(
        mcp_profile["post"]["requestBody"]["content"]["application/vnd.a3s.acl"]["schema"]
            ["maxLength"],
        65_536
    );
    assert!(mcp_profile["post"]["requestBody"]["content"]
        .get("application/json")
        .is_none());
    assert!(mcp_profile["post"]["responses"]["200"].is_object());
    assert!(mcp_profile["post"]["responses"]["201"].is_object());
    assert!(mcp_profile["post"]["responses"]["413"].is_object());
    assert!(mcp_profile["post"]["responses"]["415"].is_object());
    let ontology_collection =
        &document["paths"]["/organizations/{organization_id}/projects/{project_id}/ontologies"];
    assert_eq!(ontology_collection["get"]["tags"], json!(["Workflow"]));
    assert_eq!(ontology_collection["post"]["tags"], json!(["Workflow"]));
    assert_eq!(
        ontology_collection["post"]["requestBody"]["content"]["application/vnd.a3s.acl"]["schema"]
            ["maxLength"],
        1_048_576
    );
    assert!(ontology_collection["post"]["requestBody"]["content"]
        .get("application/json")
        .is_none());
    assert!(ontology_collection["post"]["responses"]["201"].is_object());
    assert!(ontology_collection["post"]["responses"]["413"].is_object());
    assert!(ontology_collection["post"]["responses"]["415"].is_object());
    let ontology_revision = &document["paths"]
        ["/organizations/{organization_id}/ontologies/{ontology_id}/revisions"]["post"];
    assert_eq!(ontology_revision["tags"], json!(["Workflow"]));
    assert!(ontology_revision["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "x-a3s-expected-version"
                && parameter["in"] == "header"
                && parameter["required"] == true
        })));
    assert!(ontology_revision["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "x-a3s-migration-rule"
                && parameter["in"] == "header"
                && parameter["required"] == false
        })));
    let workflow_definition_collection = &document["paths"]
        ["/organizations/{organization_id}/projects/{project_id}/workflow-definitions"];
    assert_eq!(
        workflow_definition_collection["get"]["tags"],
        json!(["Workflow"])
    );
    assert_eq!(
        workflow_definition_collection["post"]["tags"],
        json!(["Workflow"])
    );
    let workflow_publication = &workflow_definition_collection["post"]["requestBody"]["content"]
        ["application/json"]["schema"];
    assert_eq!(workflow_publication["additionalProperties"], false);
    assert_eq!(
        workflow_publication["properties"]["definitionAcl"]["maxLength"],
        1_048_576
    );
    assert_eq!(
        workflow_publication["properties"]["payloads"]["maxItems"],
        2_048
    );
    let semantic_contracts = &workflow_publication["properties"]["semanticContracts"];
    assert_eq!(semantic_contracts["additionalProperties"], false);
    assert_eq!(
        semantic_contracts["required"],
        json!([
            "descriptorBindingsAcl",
            "descriptorRegistryAcl",
            "variableContractAcl"
        ])
    );
    assert_eq!(
        semantic_contracts["properties"]["descriptorBindingsAcl"]["maxLength"],
        524_288
    );
    assert_eq!(
        semantic_contracts["properties"]["descriptorRegistryAcl"]["maxLength"],
        4_194_304
    );
    assert_eq!(
        semantic_contracts["properties"]["variableContractAcl"]["maxLength"],
        2_097_152
    );
    assert!(
        workflow_definition_collection["post"]["requestBody"]["content"]
            .get("application/vnd.a3s.acl")
            .is_none()
    );
    assert!(workflow_definition_collection["post"]["responses"]["201"].is_object());
    assert!(workflow_definition_collection["post"]["responses"]["413"].is_object());
    assert!(workflow_definition_collection["post"]["responses"]["415"].is_object());
    let workflow_revision = &document["paths"]
        ["/organizations/{organization_id}/workflow-definitions/{workflow_definition_id}/revisions"]
        ["post"];
    assert!(workflow_revision["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "x-a3s-expected-version"
                && parameter["in"] == "header"
                && parameter["required"] == true
        })));
    let workflow_goal_collection =
        &document["paths"]["/organizations/{organization_id}/projects/{project_id}/workflow-goals"];
    assert_eq!(
        workflow_goal_collection["post"]["tags"],
        json!(["Workflow"])
    );
    assert_eq!(
        workflow_goal_collection["post"]["requestBody"]["content"]["application/vnd.a3s.acl"]
            ["schema"]["maxLength"],
        262_144
    );
    assert!(workflow_goal_collection["post"]["requestBody"]["content"]
        .get("application/json")
        .is_none());
    let workflow_plan = &document["paths"]
        ["/organizations/{organization_id}/workflow-goals/{workflow_goal_id}/plan-revisions/{plan_revision_id}"]
        ["get"];
    assert_eq!(workflow_plan["tags"], json!(["Workflow"]));
    let human_task_collection = &document["paths"]
        ["/organizations/{organization_id}/projects/{project_id}/human-tasks"]["get"];
    assert_eq!(human_task_collection["tags"], json!(["Workflow"]));
    assert!(human_task_collection["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "limit"
                && parameter["in"] == "query"
                && parameter["schema"]["minimum"] == 1
                && parameter["schema"]["maximum"] == 200
        }) && parameters.iter().any(|parameter| {
            parameter["name"] == "status"
                && parameter["in"] == "query"
                && parameter["schema"]["enum"]
                    == json!([
                        "pending_activation",
                        "ready",
                        "claimed",
                        "completed",
                        "expired",
                        "cancelled"
                    ])
        })));
    assert!(human_task_collection["responses"]["200"].is_object());
    let human_task =
        &document["paths"]["/organizations/{organization_id}/human-tasks/{human_task_id}"]["get"];
    assert_eq!(human_task["tags"], json!(["Workflow"]));
    assert!(human_task["responses"]["200"].is_object());
    for action in ["claim", "release"] {
        let path =
            format!("/organizations/{{organization_id}}/human-tasks/{{human_task_id}}/{action}");
        let mutation = &document["paths"][&path]["post"];
        assert_eq!(mutation["tags"], json!(["Workflow"]), "{action}");
        assert!(mutation["requestBody"].is_null(), "{action}");
        assert!(mutation["responses"]["200"].is_object(), "{action}");
        assert!(mutation["responses"].get("202").is_none(), "{action}");
        assert!(
            mutation["parameters"]
                .as_array()
                .is_some_and(|parameters| parameters.iter().any(|parameter| {
                    parameter["name"] == "idempotency-key"
                        && parameter["in"] == "header"
                        && parameter["required"] == true
                }) && parameters.iter().any(|parameter| {
                    parameter["name"] == "x-a3s-expected-version"
                        && parameter["in"] == "header"
                        && parameter["required"] == true
                        && parameter["schema"]["minimum"] == 1
                })),
            "{action}"
        );
    }
    let submission = &document["paths"]
        ["/organizations/{organization_id}/human-tasks/{human_task_id}/submission"]["post"];
    assert_eq!(submission["tags"], json!(["Workflow"]));
    assert!(submission["responses"]["200"].is_object());
    assert!(submission["responses"]["413"].is_object());
    assert!(submission["responses"]["415"].is_object());
    assert!(submission["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().all(|parameter| {
            parameter["name"] != "idempotency-key" && parameter["name"] != "x-a3s-expected-version"
        })));
    let submission_schema = &submission["requestBody"]["content"]["application/json"]["schema"];
    assert_eq!(submission_schema["additionalProperties"], false);
    assert_eq!(
        submission_schema["properties"]["apiVersion"]["enum"],
        json!(["a3s.dev/form-interaction-submission/v1"])
    );
    assert_eq!(
        submission_schema["properties"]["identity"]["additionalProperties"],
        false
    );
    assert_eq!(
        submission_schema["properties"]["form"]["properties"]["mode"]["enum"],
        json!(["interaction"])
    );
    assert_eq!(
        submission_schema["properties"]["idempotencyKey"]["maxLength"],
        255
    );
    let form_collection =
        &document["paths"]["/organizations/{organization_id}/projects/{project_id}/forms"];
    assert_eq!(form_collection["get"]["tags"], json!(["Forms"]));
    assert_eq!(form_collection["post"]["tags"], json!(["Forms"]));
    let form_draft_schema =
        &form_collection["post"]["requestBody"]["content"]["application/json"]["schema"];
    assert_eq!(form_draft_schema["additionalProperties"], false);
    assert_eq!(form_draft_schema["required"], json!(["name", "document"]));
    assert_eq!(form_draft_schema["properties"]["name"]["maxLength"], 120);
    assert_eq!(
        form_draft_schema["properties"]["description"]["maxLength"],
        4_096
    );
    assert_eq!(
        form_draft_schema["properties"]["document"]["x-a3s-max-canonical-bytes"],
        crate::modules::forms::CLOUD_FORM_DOCUMENT_MAX_BYTES
    );
    assert!(form_collection["post"]["responses"]["200"].is_object());
    assert!(form_collection["post"]["responses"]["201"].is_object());
    assert!(form_collection["post"]["responses"]["413"].is_object());
    assert!(form_collection["post"]["responses"]["415"].is_object());
    assert!(form_collection["post"]["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "idempotency-key"
                && parameter["in"] == "header"
                && parameter["required"] == true
        })));
    let form = &document["paths"]["/organizations/{organization_id}/forms/{form_id}"]["get"];
    assert_eq!(form["tags"], json!(["Forms"]));
    let form_revision = &document["paths"]
        ["/organizations/{organization_id}/forms/{form_id}/draft-revisions"]["post"];
    assert_eq!(form_revision["tags"], json!(["Forms"]));
    assert_eq!(
        &form_revision["requestBody"]["content"]["application/json"]["schema"],
        form_draft_schema
    );
    assert!(form_revision["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "x-a3s-expected-version"
                && parameter["in"] == "header"
                && parameter["required"] == true
                && parameter["schema"]["minimum"] == 1
        })));
    let form_releases =
        &document["paths"]["/organizations/{organization_id}/forms/{form_id}/releases"];
    assert_eq!(form_releases["get"]["tags"], json!(["Forms"]));
    assert_eq!(form_releases["post"]["tags"], json!(["Forms"]));
    assert!(form_releases["post"].get("requestBody").is_none());
    assert!(form_releases["post"]["responses"]["200"].is_object());
    assert!(form_releases["post"]["responses"]["201"].is_object());
    assert!(form_releases["post"]["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "x-a3s-expected-version"
                && parameter["in"] == "header"
                && parameter["required"] == true
                && parameter["schema"]["minimum"] == 1
        })));
    let form_release = &document["paths"]
        ["/organizations/{organization_id}/forms/{form_id}/releases/{release_id}"]["get"];
    assert_eq!(form_release["tags"], json!(["Forms"]));
    let mcp_route_collection = &document["paths"]
        ["/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-route-policies"];
    assert_eq!(mcp_route_collection["get"]["tags"], json!(["Edge"]));
    assert!(mcp_route_collection["get"]["responses"]["200"].is_object());
    assert!(mcp_route_collection["get"]["responses"]
        .get("413")
        .is_none());
    assert!(mcp_route_collection["get"]["responses"]
        .get("415")
        .is_none());
    assert_eq!(
        mcp_route_collection["post"]["requestBody"]["content"]["application/vnd.a3s.acl"]["schema"]
            ["maxLength"],
        524_288
    );
    assert!(mcp_route_collection["post"]["requestBody"]["content"]
        .get("application/json")
        .is_none());
    for path in [
        "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-route-policies",
        "/organizations/{organization_id}/mcp-route-policies/{route_id}/revisions",
    ] {
        let operation = &document["paths"][path]["post"];
        assert_eq!(operation["tags"], json!(["Edge"]));
        assert!(operation["responses"]["200"].is_object());
        assert!(operation["responses"]["201"].is_object());
        assert!(operation["responses"]["413"].is_object());
        assert!(operation["responses"]["415"].is_object());
    }
    let mcp_route =
        &document["paths"]["/organizations/{organization_id}/mcp-route-policies/{route_id}"]["get"];
    assert_eq!(mcp_route["tags"], json!(["Edge"]));
    assert!(mcp_route["responses"]["200"].is_object());
    assert!(mcp_route["responses"].get("413").is_none());
    assert!(mcp_route["responses"].get("415").is_none());
    for path in [
        "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/assets/{asset_id}/releases/{asset_release_id}/workloads",
        "/organizations/{organization_id}/workloads/{workload_id}/assets/{asset_id}/releases/{asset_release_id}/deployments",
    ] {
        let operation = &document["paths"][path]["post"];
        assert_eq!(operation["tags"], json!(["Workloads"]));
        assert!(operation["requestBody"]["content"]["application/json"].is_object());
        assert!(operation["requestBody"]["content"]["application/vnd.a3s.acl"].is_object());
        assert!(operation["parameters"]
            .as_array()
            .is_some_and(|parameters| parameters.iter().any(|parameter| {
                parameter["name"] == "idempotency-key"
                    && parameter["in"] == "header"
                    && parameter["required"] == true
            })));
        assert!(operation["responses"]["200"].is_object());
        assert!(operation["responses"]["202"].is_object());
    }
    for (path, method) in [
        (
            "/organizations/{organization_id}/workloads/{workload_id}/skills/{skill_asset_id}/releases/{skill_asset_release_id}/bindings",
            "post",
        ),
        (
            "/organizations/{organization_id}/workloads/{workload_id}/skills/{skill_asset_id}/bindings",
            "delete",
        ),
    ] {
        let operation = &document["paths"][path][method];
        assert_eq!(operation["tags"], json!(["Workloads"]));
        assert!(operation.get("requestBody").is_none());
        assert!(operation["parameters"]
            .as_array()
            .is_some_and(|parameters| parameters.iter().any(|parameter| {
                parameter["name"] == "idempotency-key"
                    && parameter["in"] == "header"
                    && parameter["required"] == true
            })));
        assert!(operation["responses"]["200"].is_object());
        assert!(operation["responses"]["202"].is_object());
    }
    for path in [
        "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-credentials",
        "/organizations/{organization_id}/mcp-credentials/{credential_id}/rotate",
    ] {
        let operation = &document["paths"][path]["post"];
        assert_eq!(operation["tags"], json!(["Edge"]));
        assert!(operation["parameters"]
            .as_array()
            .is_some_and(|parameters| parameters.iter().any(|parameter| {
                parameter["name"] == "idempotency-key"
                    && parameter["in"] == "header"
                    && parameter["required"] == true
            })));
        assert!(operation["responses"]["200"].is_object());
        assert!(operation["responses"]["201"].is_object());
    }
    let executions = &document["paths"]
        ["/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/executions"];
    assert!(executions["get"].is_object());
    assert!(executions["post"]["requestBody"]["content"]["application/json"].is_object());
    assert!(executions["post"]["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "idempotency-key"
                && parameter["in"] == "header"
                && parameter["required"] == true
        })));
    let execution =
        &document["paths"]["/organizations/{organization_id}/executions/{execution_id}"];
    assert!(execution["get"].is_object());
    assert!(execution["delete"].is_object());
    let execution_templates = &document["paths"]
        ["/organizations/{organization_id}/projects/{project_id}/execution-templates"];
    assert!(execution_templates["get"].is_object());
    assert!(execution_templates["post"]["requestBody"]["content"]["application/json"].is_object());
    assert!(execution_templates["post"]["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "idempotency-key"
                && parameter["in"] == "header"
                && parameter["required"] == true
        })));
    let execution_template_revision = &document["paths"]
        ["/organizations/{organization_id}/projects/{project_id}/execution-templates/{template_id}/revisions/{revision_id}"]
        ["get"];
    assert!(execution_template_revision.is_object());
    for parameter_name in ["template_id", "revision_id"] {
        assert!(execution_template_revision["parameters"]
            .as_array()
            .is_some_and(|parameters| parameters.iter().any(|parameter| {
                parameter["name"] == parameter_name
                    && parameter["in"] == "path"
                    && parameter["schema"]["format"] == "uuid"
            })));
    }

    let conversations = &document["paths"]
        ["/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/agent-conversations"];
    assert_eq!(conversations["get"]["tags"], json!(["Agents"]));
    assert_eq!(conversations["post"]["tags"], json!(["Agents"]));
    assert!(conversations["post"].get("requestBody").is_none());
    assert!(conversations["post"]["responses"]["201"].is_object());
    assert!(conversations["post"]["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "idempotency-key"
                && parameter["in"] == "header"
                && parameter["required"] == true
        })));
    let agent_executions = &document["paths"]
        ["/organizations/{organization_id}/agent-conversations/{conversation_id}/executions"];
    assert!(agent_executions["get"]["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "limit" && parameter["schema"]["maximum"] == 200
        })));
    assert!(agent_executions["post"]["requestBody"]["content"]["application/json"].is_object());
    assert!(agent_executions["post"]["responses"]["202"].is_object());
    assert!(
        document["paths"]["/organizations/{organization_id}/agent-executions/{execution_id}"]
            ["get"]
            .is_object()
    );
    let agent_cancellation = &document["paths"]
        ["/organizations/{organization_id}/agent-executions/{execution_id}/cancel"]["post"];
    assert_eq!(agent_cancellation["tags"], json!(["Agents"]));
    assert!(agent_cancellation.get("requestBody").is_none());
    assert!(agent_cancellation["responses"]["202"].is_object());
    assert!(agent_cancellation["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "idempotency-key"
                && parameter["in"] == "header"
                && parameter["required"] == true
        })));
    let agent_events = &document["paths"]
        ["/organizations/{organization_id}/agent-conversations/{conversation_id}/events"]["get"];
    assert!(agent_events["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters
            .iter()
            .any(|parameter| { parameter["name"] == "cursor" && parameter["in"] == "query" })
            && parameters.iter().any(|parameter| {
                parameter["name"] == "limit" && parameter["schema"]["maximum"] == 200
            })));
    let agent_event_stream = &document["paths"]
        ["/organizations/{organization_id}/agent-conversations/{conversation_id}/events/stream"]
        ["get"];
    assert_eq!(
        agent_event_stream["responses"]["200"]["$ref"],
        "#/components/responses/SseSuccess200"
    );
    assert!(agent_event_stream["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "limit" && parameter["schema"]["maximum"] == 16
        }) && parameters.iter().any(|parameter| {
            parameter["name"] == "last-event-id" && parameter["in"] == "header"
        })));

    let asset_git = &document["paths"]
        ["/organizations/{organization_id}/assets/{asset_id}/git/info/refs"]["get"];
    assert_eq!(asset_git["tags"], json!(["Assets"]));
    assert!(asset_git["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "service"
                && parameter["in"] == "query"
                && parameter["required"] == true
                && parameter["schema"]["enum"] == json!(["git-upload-pack", "git-receive-pack"])
        })));
    assert_eq!(
        asset_git["responses"]["200"]["$ref"],
        "#/components/responses/AssetGitAdvertisementSuccess200"
    );

    let receive_pack = &document["paths"]
        ["/organizations/{organization_id}/assets/{asset_id}/git/git-receive-pack"]["post"];
    assert!(
        receive_pack["requestBody"]["content"]["application/x-git-receive-pack-request"]
            .is_object()
    );
    assert!(receive_pack["requestBody"]["content"]
        .get("application/json")
        .is_none());
    assert!(receive_pack["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters
            .iter()
            .all(|parameter| parameter["name"] != "idempotency-key")));
    assert_eq!(
        receive_pack["responses"]["200"]["$ref"],
        "#/components/responses/AssetGitReceivePackSuccess200"
    );
    assert_eq!(
        receive_pack["responses"]["413"]["$ref"],
        "#/components/responses/Error413"
    );
    assert_eq!(
        receive_pack["responses"]["415"]["$ref"],
        "#/components/responses/Error415"
    );

    let assets = &document["paths"]["/organizations/{organization_id}/assets"];
    assert_eq!(assets["get"]["tags"], json!(["Assets"]));
    assert_eq!(assets["post"]["tags"], json!(["Assets"]));
    assert!(assets["post"]["responses"]["201"].is_object());
    assert!(assets["post"]["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "idempotency-key" && parameter["in"] == "header"
        })));
    let archive =
        &document["paths"]["/organizations/{organization_id}/assets/{asset_id}/archive"]["post"];
    assert!(archive.get("requestBody").is_none());
    let releases =
        &document["paths"]["/organizations/{organization_id}/assets/{asset_id}/releases"];
    assert!(releases["get"].is_object());
    assert!(releases["post"]["responses"]["201"].is_object());
    let selection = &document["paths"]
        ["/organizations/{organization_id}/assets/{asset_id}/release-selection"]["get"];
    assert!(selection["parameters"]
        .as_array()
        .is_some_and(|parameters| parameters.iter().any(|parameter| {
            parameter["name"] == "version"
                && parameter["in"] == "query"
                && parameter["required"] == false
                && parameter["schema"]["type"] == "string"
        })));
    let yank = &document["paths"]
        ["/organizations/{organization_id}/assets/{asset_id}/releases/{asset_release_id}/yank"]
        ["post"];
    assert!(yank.get("requestBody").is_none());
    Ok(())
}

#[tokio::test]
async fn normal_api_responses_advertise_the_contract_version() -> Result<()> {
    let app = contract_test_application()?;
    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/api/v1/platform"))
        .await?;

    assert_eq!(response.status(), 200);
    assert_eq!(
        response.header(API_CONTRACT_VERSION_HEADER),
        Some(OPENAPI_CONTRACT_VERSION)
    );
    Ok(())
}

fn contract_test_application() -> Result<BootApplication> {
    build_test_application(
        Arc::new(InMemoryIdentityRepository::new()),
        Arc::new(InMemoryProjectsRepository::new()),
    )
}
