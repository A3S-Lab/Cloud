use super::documentation_examples::{component_example, example_from_schema};
use super::documentation_tags::TAGS;
use a3s_boot::{BootError, Result};
use serde_json::{json, Map, Value};

const DOCUMENTATION_URL: &str = "https://github.com/A3S-Lab/Cloud/blob/main/docs/openapi.md";
const REPOSITORY_URL: &str = "https://github.com/A3S-Lab/Cloud";

pub(super) fn install_documentation(document: &mut Value) -> Result<()> {
    let root = document
        .as_object_mut()
        .ok_or_else(|| BootError::Internal("generated OpenAPI document is not an object".into()))?;
    let info = root
        .get_mut("info")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| BootError::Internal("generated OpenAPI info is invalid".into()))?;
    info.insert(
        "contact".into(),
        json!({ "name": "A3S Lab", "url": REPOSITORY_URL }),
    );
    info.insert(
        "license".into(),
        json!({
            "name": "MIT",
            "url": "https://github.com/A3S-Lab/Cloud/blob/main/LICENSE"
        }),
    );
    root.insert(
        "externalDocs".into(),
        json!({
            "description": "A3S Cloud OpenAPI conventions, authentication, errors, pagination, and compatibility policy.",
            "url": DOCUMENTATION_URL
        }),
    );
    root.insert(
        "tags".into(),
        Value::Array(
            TAGS.iter()
                .map(|(name, description)| json!({ "name": name, "description": description }))
                .collect(),
        ),
    );

    let bearer = root
        .get_mut("components")
        .and_then(Value::as_object_mut)
        .and_then(|components| components.get_mut("securitySchemes"))
        .and_then(Value::as_object_mut)
        .and_then(|schemes| schemes.get_mut("bearerAuth"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| BootError::Internal("bearerAuth security scheme is missing".into()))?;
    bearer.insert(
        "description".into(),
        json!("A3S API token supplied as `Authorization: Bearer <token>`. Tokens are tenant-bound and may be revoked immediately."),
    );
    install_component_documentation(root)?;
    Ok(())
}

fn install_component_documentation(root: &mut Map<String, Value>) -> Result<()> {
    let components = root
        .get_mut("components")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| BootError::Internal("OpenAPI components are missing".into()))?;
    let schemas_snapshot = components
        .get("schemas")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| BootError::Internal("OpenAPI component schemas are missing".into()))?;
    let schemas = components
        .get_mut("schemas")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| BootError::Internal("OpenAPI component schemas are invalid".into()))?;
    for (name, schema) in schemas {
        if let Some(schema_object) = schema.as_object_mut() {
            schema_object
                .entry("description")
                .or_insert_with(|| json!(component_schema_description(name)));
        }
        document_schema(schema, None, false);
        let example = component_example(
            schemas_snapshot.get(name).unwrap_or(&Value::Null),
            &schemas_snapshot,
            None,
            0,
        );
        if let Some(schema_object) = schema.as_object_mut() {
            schema_object.entry("example").or_insert(example);
        }
    }

    let responses = components
        .get_mut("responses")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| BootError::Internal("OpenAPI component responses are invalid".into()))?;
    for (name, response) in responses {
        let Some(response) = response.as_object_mut() else {
            continue;
        };
        if let Some(headers) = response.get_mut("headers").and_then(Value::as_object_mut) {
            for (name, header) in headers {
                let Some(header) = header.as_object_mut() else {
                    continue;
                };
                header.entry("description").or_insert_with(|| {
                    json!(match name.as_str() {
                        "x-request-id" => {
                            "Request correlation UUID generated or propagated by the control plane."
                        }
                        "x-a3s-api-contract-version" => {
                            "Exact A3S Cloud REST contract version used for this response."
                        }
                        _ => "Response metadata defined by the A3S Cloud REST contract.",
                    })
                });
            }
        }
        let Some(content) = response.get_mut("content").and_then(Value::as_object_mut) else {
            continue;
        };
        for (media_type, media) in content {
            let Some(media) = media.as_object_mut() else {
                continue;
            };
            let Some(schema) = media.get("schema") else {
                continue;
            };
            let mut example = if media_type == "text/event-stream" {
                json!("id: 100\nevent: updated\ndata: {}\n\n")
            } else if schema.get("format").and_then(Value::as_str) == Some("binary") {
                binary_example(media_type)
            } else {
                component_example(schema, &schemas_snapshot, None, 0)
            };
            if media_type == "application/json"
                && schema
                    .get("$ref")
                    .and_then(Value::as_str)
                    .is_some_and(|reference| {
                        reference.ends_with("SuccessResponse")
                            || reference.ends_with("ErrorResponse")
                    })
            {
                if let (Some(status), Some(example)) =
                    (response_component_status(name), example.as_object_mut())
                {
                    if example.contains_key("code") {
                        example.insert("code".into(), json!(status));
                    }
                }
            }
            media.entry("example").or_insert(example);
        }
    }
    Ok(())
}

fn response_component_status(name: &str) -> Option<u16> {
    let suffix = name.get(name.len().checked_sub(3)?..)?;
    let status = suffix.parse::<u16>().ok()?;
    (200..=599).contains(&status).then_some(status)
}

fn component_schema_description(name: &str) -> String {
    match name {
        "ApiSuccessResponse" => {
            "Standard A3S success envelope returned by JSON REST operations.".into()
        }
        "ApiErrorResponse" => {
            "Standard A3S error envelope with a stable business error code and correlation data."
                .into()
        }
        "RecipientContact" => {
            "Exact-owner recipient contact projection with a redacted mailbox hint and digest."
                .into()
        }
        "RecipientContactList" => {
            "Recipient contacts owned by the authenticated human Principal.".into()
        }
        "RecipientContactMutation" => {
            "Redacted recipient contact mutation result with idempotent replay state.".into()
        }
        "NotificationAlertPolicy" => {
            "Immutable personal alert policy over a closed notification source family.".into()
        }
        "NotificationAlertPolicyTarget" => {
            "Closed alert target union discriminated by `kind`.".into()
        }
        "NotificationAlertPolicyEnvironmentTarget" => {
            "Exact Project and Environment target used by alert-policy v1.".into()
        }
        "NotificationAlertPolicyNodeTarget" => {
            "Exact Fleet Node target used by alert-policy v2.".into()
        }
        "NotificationAlertPolicyPage" => {
            "Bounded cursor page of personal notification alert policies.".into()
        }
        "NotificationAlertPolicyMutation" => {
            "Alert-policy mutation result with explicit idempotent replay state.".into()
        }
        "OutboundNotificationSubscription" => {
            "Immutable personal outbound notification subscription and delivery policy.".into()
        }
        "OutboundNotificationTarget" => {
            "Closed delivery-authority union discriminated by `kind`.".into()
        }
        "OutboundNotificationConnectorTarget" => {
            "Exact immutable Connector profile revision target for HTTP delivery.".into()
        }
        "OutboundNotificationRecipientContactTarget" => {
            "Opaque verified recipient-contact target for SMTP delivery.".into()
        }
        "OutboundNotificationSubscriptionPage" => {
            "Bounded cursor page of personal outbound notification subscriptions.".into()
        }
        "OutboundNotificationSubscriptionMutation" => {
            "Outbound-subscription mutation result with explicit idempotent replay state.".into()
        }
        _ if name.ends_with("SuccessResponse") => {
            format!("Standard A3S success envelope carrying {name} data.")
        }
        _ => format!("Reusable schema for the A3S Cloud {name} contract."),
    }
}

pub(super) fn describe_operation_documentation(
    operation: &mut Map<String, Value>,
    method: &str,
    path: &str,
    tag: &str,
    is_public: bool,
) -> Result<()> {
    let summary = operation_summary(method, path);
    document_parameters(operation, path);
    document_request_body(operation, &summary);
    let description = operation_description(operation, method, path, &summary, tag, is_public);
    operation.insert("summary".into(), json!(summary));
    operation.insert("description".into(), json!(description));
    operation.insert(
        "x-a3s-response-data".into(),
        json!(response_data_description(method, path, &summary)),
    );
    operation.insert(
        "x-a3s-authentication".into(),
        json!(if is_public { "public" } else { "bearer-token" }),
    );
    operation.insert(
        "x-a3s-idempotent-replay".into(),
        json!(has_parameter(operation, "header", "idempotency-key")),
    );
    Ok(())
}

fn document_parameters(operation: &mut Map<String, Value>, path: &str) {
    let Some(parameters) = operation
        .get_mut("parameters")
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for parameter in parameters {
        let Some(parameter) = parameter.as_object_mut() else {
            continue;
        };
        let Some(name) = parameter
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        let location = parameter
            .get("in")
            .and_then(Value::as_str)
            .unwrap_or("parameter")
            .to_owned();
        parameter
            .entry("description")
            .or_insert_with(|| json!(parameter_description(&name, &location, path)));
        if parameter.get("example").is_none() {
            let example = parameter
                .get("schema")
                .map(|schema| example_from_schema(schema, Some(&name)))
                .unwrap_or_else(|| json!("example"));
            parameter.insert("example".into(), example);
        }
    }
}

fn document_request_body(operation: &mut Map<String, Value>, summary: &str) {
    let Some(request_body) = operation
        .get_mut("requestBody")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    request_body.entry("description").or_insert_with(|| {
        json!(format!(
            "Inputs required to {}. Unknown fields are rejected when the schema is closed.",
            lowercase_first(summary)
        ))
    });
    let Some(content) = request_body
        .get_mut("content")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    for (media_type, media) in content {
        let Some(media) = media.as_object_mut() else {
            continue;
        };
        let Some(schema) = media.get_mut("schema") else {
            continue;
        };
        let recursively_document = schema
            .get("x-a3s-contract-correction")
            .and_then(Value::as_str)
            == Some("documents-existing-runtime-validation");
        document_schema(schema, None, recursively_document);
        let example = if schema.get("format").and_then(Value::as_str) == Some("binary") {
            binary_example(media_type)
        } else if media_type == "application/vnd.a3s.acl" {
            json!("version = 1\n")
        } else {
            example_from_schema(schema, None)
        };
        media.entry("example").or_insert(example);
    }
}

fn binary_example(media_type: &str) -> Value {
    json!(match media_type {
        "application/x-git-receive-pack-advertisement" => {
            "001f# service=git-receive-pack\n0000"
        }
        "application/x-git-upload-pack-advertisement" => {
            "001e# service=git-upload-pack\n0000"
        }
        "application/x-git-receive-pack-result" => "000eunpack ok\n0000",
        "application/x-git-upload-pack-result" => "0008NAK\nPACK...",
        "application/x-git-receive-pack-request" | "application/x-git-upload-pack-request" =>
            "0000",
        _ => "binary payload",
    })
}

fn document_schema(schema: &mut Value, field_name: Option<&str>, recursive: bool) {
    if let Some(object) = schema.as_object_mut() {
        if object.contains_key("$ref") {
            return;
        }
        object.entry("description").or_insert_with(|| {
            json!(field_name.map_or_else(
                || "Validated request payload for this operation.".to_owned(),
                schema_property_description,
            ))
        });
        if !recursive {
            return;
        }
        if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
            for (name, property) in properties {
                document_schema(property, Some(name), true);
            }
        }
        if let Some(items) = object.get_mut("items") {
            document_schema(items, field_name, true);
        }
        for composition in ["allOf", "anyOf", "oneOf"] {
            if let Some(variants) = object.get_mut(composition).and_then(Value::as_array_mut) {
                for variant in variants {
                    document_schema(variant, field_name, true);
                }
            }
        }
        if let Some(additional) = object.get_mut("additionalProperties") {
            if additional.is_object() {
                document_schema(additional, field_name, true);
            }
        }
    }
}

fn schema_property_description(name: &str) -> String {
    match name {
        "token" | "enrollment_token" => {
            "Write-only caller-generated credential. The server stores only its digest.".into()
        }
        "value" => "Write-only secret value; plaintext is never returned by the API.".into(),
        "definitionAcl" | "definition_acl" => {
            "Canonical A3S ACL document validated by the owning domain.".into()
        }
        "expectedVersion" | "expectedAggregateVersion" => {
            "Current aggregate version used for optimistic concurrency control.".into()
        }
        "id"
        | "principalId"
        | "nodeId"
        | "secretId"
        | "revisionId"
        | "agentAssetId"
        | "agentAssetReleaseId"
        | "gatewayScopeId"
        | "workloadRevisionId"
        | "domainClaimId" => {
            format!(
                "Stable UUID for the {}.",
                humanize_identifier(name.trim_end_matches("Id"))
            )
        }
        _ => format!(
            "{} represented by this contract.",
            title_case(&humanize_identifier(name))
        ),
    }
}

fn operation_description(
    operation: &Map<String, Value>,
    method: &str,
    path: &str,
    summary: &str,
    tag: &str,
    is_public: bool,
) -> String {
    if let Some(description) = special_description(method, path) {
        return format!(
            "{description} Successful JSON responses use the standard A3S envelope and expose request and contract-version headers."
        );
    }

    let mut sentences = vec![format!(
        "{summary} through the authoritative {tag} REST boundary in {}.",
        scope_description(path)
    )];
    sentences.push(if is_public {
        "This operation is public and declares an empty OpenAPI security requirement.".into()
    } else {
        "A tenant-authorized A3S bearer token is required.".into()
    });
    if has_parameter(operation, "header", "idempotency-key") {
        sentences.push(
            "A stable idempotency key makes mutation retries replay-safe without duplicating side effects."
                .into(),
        );
    }
    if has_parameter(operation, "query", "cursor") {
        sentences.push(
            "Use the opaque cursor to continue bounded pagination without guessing server state."
                .into(),
        );
    }
    if method == "get" && path.ends_with("/audit-records/export") {
        sentences.push(
            "The inclusive from/to window is required and may span at most 31 days; the result is one canonical redacted page in a verifiable Ed25519 DSSE envelope."
                .into(),
        );
    }
    if method == "get" && path.ends_with("/audit-records/export/manifest") {
        sentences.push(
            "The inclusive from/to window is required and may span at most 31 days; one transaction captures at most eight complete pages, which are returned with a signed retention-bound manifest or fail without a partial result."
                .into(),
        );
    }
    if method == "get" && path.ends_with("/audit-records/retention") {
        sentences.push(
            "The configured semantic policy and durable per-organization availability/deletion watermarks make historical gaps explicit."
                .into(),
        );
    }
    if path.ends_with("/stream") {
        sentences.push(
            "The response is a resumable server-sent event stream; reconnect with the documented cursor or Last-Event-ID value."
                .into(),
        );
    } else {
        sentences.push(
            "Successful JSON responses use the standard A3S envelope and expose request and contract-version headers."
                .into(),
        );
    }
    sentences.join(" ")
}

fn special_description(method: &str, path: &str) -> Option<&'static str> {
    match (method, path) {
        ("get", "/identity/oidc/{provider_key}/login") => Some(
            "Starts a public OIDC login and redirects to the configured provider. State, nonce, and S256 PKCE bind the one-time flow; nonce and verifier are held only in Secure HttpOnly callback cookies.",
        ),
        ("post", "/organizations/{organization_id}/identity/oidc/{provider_key}/link") => Some(
            "Starts an authenticated human-principal OIDC link flow. The result carries the provider authorization URL while Secure HttpOnly callback cookies bind the one-time flow.",
        ),
        ("get", "/identity/oidc/{provider_key}/callback") => Some(
            "Completes one OIDC login or link flow using the query state and callback-only HttpOnly cookies. Login credentials are returned once in JSON and never placed in a redirect URL.",
        ),
        _ => None,
    }
}

fn operation_summary(method: &str, path: &str) -> String {
    match (method, path) {
        ("post", "/bootstrap") => return "Bootstrap the first organization".into(),
        ("get", "/health/live") => return "Check control-plane liveness".into(),
        ("get", "/health/ready") => return "Check control-plane readiness".into(),
        ("get", "/platform") => return "Get platform diagnostics".into(),
        ("get", "/organizations/{organization_id}/audit-records/export") => {
            return "Export a signed audit page".into()
        }
        ("get", "/organizations/{organization_id}/audit-records/export/manifest") => {
            return "Export a complete signed audit manifest".into()
        }
        ("get", "/organizations/{organization_id}/audit-records/retention") => {
            return "Get audit retention status".into()
        }
        ("post", "/node-control/enroll") => return "Enroll a node".into(),
        ("post", "/webhooks/github") => return "Receive a GitHub webhook".into(),
        ("get", "/organizations/{organization_id}/search") => {
            return "Search authorized organization resources".into()
        }
        ("get", "/organizations/{organization_id}/source-connections/github") => {
            return "Get the GitHub source connection".into()
        }
        ("post", "/organizations/{organization_id}/source-connections/github") => {
            return "Start GitHub source connection setup".into()
        }
        ("get", "/source-connections/github/setup") => return "Start GitHub App setup".into(),
        ("get", "/source-connections/github/callback") => {
            return "Complete GitHub App setup".into()
        }
        ("get", "/identity/oidc/{provider_key}/login") => return "Start OIDC login".into(),
        ("get", "/identity/oidc/{provider_key}/callback") => {
            return "Complete OIDC authentication".into()
        }
        ("post", "/organizations/{organization_id}/identity/oidc/{provider_key}/link") => {
            return "Start OIDC identity linking".into()
        }
        _ => {}
    }

    if path.ends_with("/git/info/refs") {
        return "Advertise Git references".into();
    }
    if path.ends_with("/git/git-upload-pack") {
        return "Fetch Git objects".into();
    }
    if path.ends_with("/git/git-receive-pack") {
        return "Push Git objects".into();
    }
    if path.ends_with("/release-selection") {
        return "Select an asset release".into();
    }
    if path.ends_with("/workflow-node-catalog") {
        return "Get the workflow node catalog".into();
    }
    if path.ends_with("/attribution-profile") {
        return "Get the current project attribution profile".into();
    }
    if path.contains("/diff/") {
        return "Compare ontology revisions".into();
    }
    if path.ends_with("/logs/stream") {
        return "Stream workload logs".into();
    }
    if path.ends_with("/events/stream") {
        return "Stream agent execution events".into();
    }
    if path.ends_with("/operations/stream") {
        return "Stream operation updates".into();
    }
    if path.ends_with("/connector-profiles/{profile_id}/revisions/{revision_id}/revocation") {
        return if method == "get" {
            "Get a Connector revision revocation".into()
        } else {
            "Revoke a Connector revision".into()
        };
    }
    if path.ends_with(
        "/connector-profiles/{profile_id}/revisions/{revision_id}/execution-attempts/{attempt_id}/resolution",
    ) {
        return if method == "get" {
            "Get a Connector execution attempt resolution".into()
        } else {
            "Resolve an indeterminate Connector execution attempt".into()
        };
    }
    if path.ends_with("/connector-profiles/{profile_id}/revisions/{revision_id}/execution-attempts")
    {
        return "List unresolved Connector execution attempts".into();
    }
    if path.ends_with(
        "/connector-profiles/{profile_id}/revisions/{revision_id}/execution-attempts/{attempt_id}",
    ) {
        return "Get a Connector execution attempt".into();
    }
    if method == "get" {
        if path.contains("/assets/") && path.contains("/releases/") && path.ends_with('}') {
            return "Get an asset release".into();
        }
        if path.contains("/forms/") && path.contains("/releases/") && path.ends_with('}') {
            return "Get a form release".into();
        }
        if path.contains("/applications/") && path.contains("/releases/") && path.ends_with('}') {
            return "Get an application release".into();
        }
        if path.contains("/connector-profiles/")
            && path.contains("/revisions/")
            && path.ends_with('}')
        {
            return "Get a Connector revision".into();
        }
        if path.contains("/durable-cell-applications/")
            && path.contains("/revisions/")
            && path.ends_with('}')
        {
            return "Get a Durable Cell application revision".into();
        }
        if path.contains("/ontologies/") && path.contains("/revisions/") && path.ends_with('}') {
            return "Get an ontology revision".into();
        }
        if path.contains("/workflow-definitions/")
            && path.contains("/revisions/")
            && path.ends_with('}')
        {
            return "Get a workflow revision".into();
        }
        if path.contains("/execution-templates/")
            && path.contains("/revisions/")
            && path.ends_with('}')
        {
            return "Get an execution template revision".into();
        }
        if path.contains("/workflow-goals/")
            && path.contains("/plan-revisions/")
            && path.ends_with('}')
        {
            return "Get a workflow plan revision".into();
        }
        for (suffix, summary) in [
            ("/wait", "Wait for workflow run completion"),
            ("/output", "Get workflow run output"),
            ("/variables", "Inspect workflow run variables"),
            ("/diagnostics", "Inspect workflow run diagnostics"),
            ("/history", "List workflow run history"),
            ("/replay", "Replay an application session"),
            ("/changes", "Get agent execution changes"),
            ("/evidence", "Get signed build evidence"),
            ("/logs", "List workload logs"),
            ("/mcp-service-profile", "Get the MCP service profile"),
        ] {
            if path.ends_with(suffix) {
                return summary.into();
            }
        }
    }
    if method == "post" {
        if path == "/organizations/{organization_id}/recipient-contacts" {
            return "Request recipient contact verification".into();
        }
        if let Some(summary) = mutation_action_summary(path) {
            return summary.into();
        }
    }
    if method == "delete" {
        if path.contains("/api-tokens/") {
            return "Revoke an API token".into();
        }
        if path.contains("/build-runs/") {
            return "Cancel a build run".into();
        }
        if path.contains("/deployments/") {
            return "Cancel a deployment".into();
        }
        if path.contains("/executions/") {
            return "Cancel an execution".into();
        }
        if path.ends_with("/bindings") {
            return "Remove a skill binding".into();
        }
    }

    let resource = resource_for_path(path);
    match method {
        "get" if path.ends_with('}') => format!("Get {}", article(resource.singular)),
        "get" => format!("List {}", resource.plural),
        "post" => format!("Create {}", article(resource.singular)),
        "delete" => format!("Delete {}", article(resource.singular)),
        "patch" => format!("Update {}", article(resource.singular)),
        "put" => format!("Replace {}", article(resource.singular)),
        _ => format!("Operate on {}", resource.plural),
    }
}

fn mutation_action_summary(path: &str) -> Option<&'static str> {
    for (suffix, summary) in [
        ("/acceptance", "Accept a membership invitation"),
        (
            "/agent-executions/{execution_id}/cancel",
            "Cancel an agent execution",
        ),
        (
            "/approval-checkpoints/{checkpoint_id}/decision",
            "Decide an agent Tool approval checkpoint",
        ),
        (
            "/sessions/{session_id}/close",
            "Close an application session",
        ),
        (
            "/invocations/{invocation_id}/cancel",
            "Cancel an application invocation",
        ),
        (
            "/workflow-runs/{workflow_run_id}/cancel",
            "Cancel a workflow run",
        ),
        ("/maintenance/cancel", "Cancel node-pool maintenance"),
        ("/archive", "Archive an asset"),
        ("/retry", "Retry a build run"),
        ("/domain-claims/{claim_id}/verify", "Verify a domain claim"),
        ("/domain-claims/{claim_id}/revoke", "Revoke a domain claim"),
        (
            "/mcp-credentials/{credential_id}/rotate",
            "Rotate an MCP credential",
        ),
        (
            "/mcp-credentials/{credential_id}/revoke",
            "Revoke an MCP credential",
        ),
        (
            "/notification-alert-policies/{policy_id}/revoke",
            "Revoke a notification alert policy",
        ),
        (
            "/notification-outbound-subscriptions/{subscription_id}/revoke",
            "Revoke an outbound notification subscription",
        ),
        (
            "/notifications/{notification_id}/read",
            "Mark a notification as read",
        ),
        ("/human-tasks/{human_task_id}/claim", "Claim a human task"),
        (
            "/human-tasks/{human_task_id}/release",
            "Release a human task claim",
        ),
        (
            "/human-tasks/{human_task_id}/submission",
            "Submit a human task interaction",
        ),
        (
            "/membership-invitations/{invitation_id}/revocation",
            "Revoke a membership invitation",
        ),
        (
            "/memberships/{membership_id}/revocation",
            "Revoke a membership",
        ),
        (
            "/resource-grants/{resource_grant_id}/revocation",
            "Revoke a resource grant",
        ),
        (
            "/recipient-contacts/{recipient_contact_id}/verification",
            "Verify a recipient contact",
        ),
        (
            "/recipient-contacts/{recipient_contact_id}/revocation",
            "Revoke a recipient contact",
        ),
        (
            "/memberships/{membership_id}/role",
            "Change a membership role",
        ),
        ("/maintenance", "Schedule node-pool maintenance"),
        ("/members/removal", "Remove node-pool members"),
        ("/members", "Add node-pool members"),
        ("/actions/drain", "Drain a node"),
        ("/actions/ready", "Mark a node ready"),
        ("/actions/revoke", "Revoke a node"),
        (
            "/catalog/cache/inspect",
            "Inspect the cached plugin catalog",
        ),
        ("/catalog/cache/search", "Search the cached plugin catalog"),
        ("/catalog/inspect", "Inspect the live plugin catalog"),
        ("/catalog/search", "Search the live plugin catalog"),
        ("/start", "Start a Durable Cell application"),
        ("/stop", "Stop a workload or Durable Cell application"),
        ("/deactivate", "Deactivate a GitHub source subscription"),
        ("/rollback", "Roll back a workload"),
        ("/yank", "Yank an asset release"),
        ("/mcp-service-profile", "Publish an MCP service profile"),
        ("/draft-revisions", "Revise a form draft"),
        ("/revisions", "Publish a new revision"),
        ("/releases", "Publish a release"),
        ("/sessions", "Open an application session"),
        ("/invocations", "Request an application invocation"),
        ("/versions", "Create a secret version"),
        ("/executions", "Start an execution"),
        ("/deployments", "Create a deployment"),
        ("/bindings", "Bind a skill release"),
    ] {
        if path.ends_with(suffix) {
            return Some(summary);
        }
    }
    if path.ends_with("/routes") {
        return Some("Publish a route");
    }
    if path.ends_with("/source-revisions") {
        return Some("Resolve a source revision");
    }
    if path.ends_with("/workloads") {
        return Some("Create or deploy a workload");
    }
    None
}

#[derive(Clone, Copy)]
struct ResourceLabel {
    singular: &'static str,
    plural: &'static str,
}

fn resource_for_path(path: &str) -> ResourceLabel {
    if path.contains("security-investigations") && path.ends_with("/timeline") {
        return ResourceLabel {
            singular: "security timeline",
            plural: "security timeline entries",
        };
    }
    path.trim_matches('/')
        .split('/')
        .rev()
        .filter(|segment| !segment.starts_with('{'))
        .find_map(resource_label)
        .unwrap_or(ResourceLabel {
            singular: "resource",
            plural: "resources",
        })
}

fn resource_label(segment: &str) -> Option<ResourceLabel> {
    let (singular, plural) = match segment {
        "organizations" => ("organization", "organizations"),
        "projects" => ("project", "projects"),
        "environments" => ("environment", "environments"),
        "agent-conversations" => ("agent conversation", "agent conversations"),
        "agent-executions" => ("agent execution", "agent executions"),
        "approval-checkpoints" => ("agent approval checkpoint", "agent approval checkpoints"),
        "executions" => ("execution", "executions"),
        "events" => ("event", "events"),
        "api-tokens" => ("API token", "API tokens"),
        "assets" => ("asset", "assets"),
        "releases" => ("release", "releases"),
        "revisions" => ("revision", "revisions"),
        "plan-revisions" => ("workflow plan revision", "workflow plan revisions"),
        "audit-records" => ("audit record", "audit records"),
        "build-runs" => ("build run", "build runs"),
        "deployments" => ("deployment", "deployments"),
        "domain-claims" => ("domain claim", "domain claims"),
        "enrollment-tokens" => ("enrollment token", "enrollment tokens"),
        "forms" => ("form", "forms"),
        "gateway-certificates" => ("gateway certificate", "gateway certificates"),
        "human-tasks" => ("human task", "human tasks"),
        "mcp-credentials" => ("MCP credential", "MCP credentials"),
        "mcp-route-policies" => ("MCP route policy", "MCP route policies"),
        "membership-invitations" => ("membership invitation", "membership invitations"),
        "memberships" => ("membership", "memberships"),
        "resource-grants" => ("resource grant", "resource grants"),
        "recipient-contacts" => ("recipient contact", "recipient contacts"),
        "node-pools" => ("node pool", "node pools"),
        "nodes" => ("node", "nodes"),
        "notification-alert-policies" => {
            ("notification alert policy", "notification alert policies")
        }
        "notification-outbound-subscriptions" => (
            "outbound notification subscription",
            "outbound notification subscriptions",
        ),
        "notifications" => ("notification", "notifications"),
        "ontologies" => ("ontology", "ontologies"),
        "operations" => ("operation", "operations"),
        "plugin-registries" => ("plugin registry", "plugin registries"),
        "applications" => ("application", "applications"),
        "sessions" => ("application session", "application sessions"),
        "invocations" => ("application invocation", "application invocations"),
        "messages" => ("application message", "application messages"),
        "attribution-profiles" => (
            "project attribution profile",
            "project attribution profiles",
        ),
        "connector-profiles" => ("Connector profile", "Connector profiles"),
        "durable-cell-applications" => ("Durable Cell application", "Durable Cell applications"),
        "gateway-scopes" => ("gateway scope", "gateway scopes"),
        "routes" => ("route", "routes"),
        "secrets" => ("secret", "secrets"),
        "source-revisions" => ("source revision", "source revisions"),
        "source-subscriptions" => ("source subscription", "source subscriptions"),
        "workloads" => ("workload", "workloads"),
        "execution-templates" => ("execution template", "execution templates"),
        "workflow-definitions" => ("workflow definition", "workflow definitions"),
        "workflow-goals" => ("workflow goal", "workflow goals"),
        "workflow-runs" => ("workflow run", "workflow runs"),
        _ => return None,
    };
    Some(ResourceLabel { singular, plural })
}

fn parameter_description(name: &str, location: &str, path: &str) -> String {
    match name {
        "idempotency-key" => "Caller-owned replay key for this mutation. Reuse the same value only for the same logical request.".into(),
        "x-a3s-bootstrap-token" => "One-time deployment bootstrap secret configured by the operator.".into(),
        "x-a3s-expected-version" => "Current aggregate version used for optimistic concurrency control.".into(),
        "x-a3s-migration-rule" => "ACL migration-rule identifier required for an explicitly breaking ontology revision.".into(),
        "x-github-event" => "GitHub event name covered by the webhook signature.".into(),
        "x-github-delivery" => "Unique GitHub delivery identifier used for replay protection.".into(),
        "x-hub-signature-256" => "HMAC-SHA256 signature of the exact webhook request body.".into(),
        "last-event-id" => "Last completely processed event identifier used to resume an SSE stream.".into(),
        "cursor" => "Opaque continuation cursor returned by the preceding page or stream response.".into(),
        "limit" => "Maximum number of records to return in this bounded response.".into(),
        "q" => "Case-insensitive search text matched against authorized resource projections.".into(),
        "afterSequence" => "Return records whose monotonic sequence is strictly greater than this value.".into(),
        "timeoutSeconds" => "Maximum number of seconds for the bounded wait operation.".into(),
        "status" => "Optional lifecycle status used to filter the result set.".into(),
        "stream" => "Optional output stream filter for workload log records.".into(),
        "unreadOnly" => "When true, return only notifications not yet marked as read.".into(),
        "service" => "Git Smart HTTP service requested by the client.".into(),
        "version" if location == "query" => "Exact semantic version to select; omit it to use the highest stable release.".into(),
        "version" => "Positive immutable version number addressed by this operation.".into(),
        "from" => "Inclusive RFC 3339 lower timestamp bound for the audit query.".into(),
        "to" => "Inclusive RFC 3339 upper timestamp bound for the audit query.".into(),
        "action" => "Dot-separated audit action key used to filter records.".into(),
        "actorPrincipalId" => "Principal UUID used to filter audit records by actor.".into(),
        "aggregateId" => "Aggregate UUID used to filter audit records by target.".into(),
        "requestId" => "Request UUID used to correlate an audit record with an API call.".into(),
        "projectId" if path.contains("/audit-records") => "Exact request-time Project UUID used to filter audit records.".into(),
        "environmentId" if path.contains("/audit-records") => "Exact request-time child Environment UUID used to filter audit records.".into(),
        "pageSize" if path.ends_with("/audit-records/export/manifest") => "Number of records in each signed page; the complete bundle contains at most eight pages.".into(),
        "attributionProfileId" => "Immutable request-time Project attribution-profile UUID used to filter audit records.".into(),
        "attributionStatus" => "Closed request-time Project attribution status used to filter audit records.".into(),
        "code" => "Short-lived authorization code returned by the identity provider.".into(),
        "state" => "Opaque one-time state value that binds the browser callback to its initiating flow.".into(),
        "error" => "Provider error code returned instead of an authorization code.".into(),
        "installation_id" => "GitHub App installation identifier selected during setup.".into(),
        "setup_action" => "GitHub setup lifecycle action associated with the callback.".into(),
        "organization_id" if location == "query" => "Organization UUID bound to the one-time setup flow.".into(),
        _ if location == "path" && name.ends_with("_id") => format!(
            "Stable UUID of the {} addressed by this operation.",
            humanize_identifier(name.trim_end_matches("_id"))
        ),
        _ if location == "path" => format!(
            "Path value identifying the {} addressed by this operation.",
            humanize_identifier(name)
        ),
        _ => format!(
            "{} parameter for `{path}`.",
            title_case(&humanize_identifier(name))
        ),
    }
}

fn response_data_description(method: &str, path: &str, summary: &str) -> String {
    if method == "get" && path.ends_with("/audit-records/export") {
        return "One canonical redacted audit page in a DSSE envelope with its Ed25519 public verification key and key identity.".into();
    }
    if method == "get" && path.ends_with("/audit-records/export/manifest") {
        return "Zero through eight canonical signed audit pages plus one signed manifest binding their digests, cursor chain, shared signing key, counts, exact filter, and captured retention state.".into();
    }
    if method == "get" && path.ends_with("/audit-records/retention") {
        return "The configured audit retention duration and semantic digest plus the durable applied digest, inclusive availability watermark, physical-deletion boundary, aggregate deleted-record count, schedule, and monotonic state version.".into();
    }
    if path.ends_with("/stream") {
        return format!(
            "A resumable stream for {}.",
            lowercase_first(summary.trim_start_matches("Stream "))
        );
    }
    if let Some(subject) = summary.strip_prefix("List ") {
        return format!("A bounded collection or page of {subject}.");
    }
    if let Some(subject) = summary.strip_prefix("Search ") {
        return format!("A bounded set of authorized matches for {subject}.");
    }
    if let Some(subject) = summary.strip_prefix("Get ") {
        return format!("The authoritative {subject} projection.");
    }
    if let Some(subject) = summary.strip_prefix("Check ") {
        return format!("The current {subject} report.");
    }
    if method == "get" {
        return format!("The documented result of {}.", lowercase_first(summary));
    }
    "The authoritative mutation result, including replay metadata when the operation is idempotent."
        .into()
}

fn has_parameter(operation: &Map<String, Value>, location: &str, name: &str) -> bool {
    operation
        .get("parameters")
        .and_then(Value::as_array)
        .is_some_and(|parameters| {
            parameters.iter().any(|parameter| {
                parameter.get("in").and_then(Value::as_str) == Some(location)
                    && parameter.get("name").and_then(Value::as_str) == Some(name)
            })
        })
}

fn scope_description(path: &str) -> &'static str {
    if path.contains("/environments/{environment_id}") {
        "the addressed organization, project, and environment scope"
    } else if path.contains("/projects/{project_id}") {
        "the addressed organization and project scope"
    } else if path.starts_with("/organizations/{organization_id}") {
        "the addressed organization scope"
    } else {
        "the public control-plane scope"
    }
}

fn humanize_identifier(value: &str) -> String {
    let mut result = String::new();
    for character in value.chars() {
        if character == '_' || character == '-' {
            result.push(' ');
        } else if character.is_ascii_uppercase() {
            result.push(' ');
            result.push(character.to_ascii_lowercase());
        } else {
            result.push(character);
        }
    }
    result.trim().to_owned()
}

fn title_case(value: &str) -> String {
    let mut characters = value.chars();
    characters
        .next()
        .map(|first| first.to_ascii_uppercase().to_string() + characters.as_str())
        .unwrap_or_default()
}

fn lowercase_first(value: &str) -> String {
    let mut characters = value.chars();
    characters
        .next()
        .map(|first| first.to_ascii_lowercase().to_string() + characters.as_str())
        .unwrap_or_default()
}

fn article(noun: &str) -> String {
    let article = if noun.starts_with("MCP")
        || noun.chars().next().is_some_and(|character| {
            matches!(character.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u')
        }) {
        "an"
    } else {
        "a"
    };
    format!("{article} {noun}")
}
