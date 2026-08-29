use super::*;
use a3s_cloud_control_plane::ControlPlane;

const POSTGRES_URL_ENV: &str = "A3S_CLOUD_PMCS_POSTGRES_URL";
const BOOTSTRAP_TOKEN_ENV: &str = "A3S_CLOUD_PMCS_BOOTSTRAP_TOKEN";
const BOOTSTRAP_TOKEN_VALUE: &str = "pmcs-bootstrap-credential-0123456789abcdef";
const OPERATOR_TOKEN: &str = "a3s_dededededededededededededededededededededededededededededededede";
const BINDING_IDEMPOTENCY_KEY: &str = "pmcs:platform-binding:create";
const REVOCATION_IDEMPOTENCY_KEY: &str = "pmcs:platform-binding:revoke";

pub async fn exercise_privileged_management_cross_surface(
    postgres_url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&postgres_url, 12).await?;
    let database = Database::new(PostgresDialect, executor);
    let _postgres_url = EnvironmentOverride::set(POSTGRES_URL_ENV, &postgres_url);
    let _bootstrap_token = EnvironmentOverride::set(BOOTSTRAP_TOKEN_ENV, BOOTSTRAP_TOKEN_VALUE);

    let first_state = tempfile::tempdir()?;
    let second_state = tempfile::tempdir()?;
    let first = build_instance(first_state.path()).await?;
    let second = build_instance(second_state.path()).await?;
    let organization_id = bootstrap(&first).await?;

    let rest_policy = first
        .call(get_as("/api/v1/platform/role-policy", ADMIN_TOKEN))
        .await?;
    assert_eq!(rest_policy.status(), 200);
    let rest_policy = response_json(&rest_policy)?["data"].clone();
    let policy_revision_id = required_string(&rest_policy["id"], "policy revision ID")?;

    let mcp_policy = second
        .call(management_mcp_request(
            ADMIN_TOKEN,
            1,
            "a3s_cloud_platform_role_policy_current_get",
            json!({}),
        ))
        .await?;
    assert_mcp_code(&mcp_policy, 200, false)?;
    assert_eq!(
        response_json(&mcp_policy)?["result"]["structuredContent"]["data"],
        rest_policy,
        "REST and MCP must expose one current platform-role policy projection"
    );

    let operator_principal_id = create_human_principal(&first, &organization_id).await?;
    create_operator_token(&first, &organization_id, &operator_principal_id).await?;
    assert_policy_denied_on_both_surfaces(&first, &second).await?;

    let binding_arguments = json!({
        "principalId": operator_principal_id,
        "role": "platform_operator",
        "expectedPolicyRevisionId": policy_revision_id,
        "idempotencyKey": BINDING_IDEMPOTENCY_KEY
    });
    let (rest_created, mcp_created) = tokio::join!(
        first.call(post_json(
            "/api/v1/platform/role-bindings",
            BINDING_IDEMPOTENCY_KEY,
            json!({
                "principalId": operator_principal_id,
                "role": "platform_operator",
                "expectedPolicyRevisionId": policy_revision_id
            }),
        )),
        second.call(management_mcp_request(
            ADMIN_TOKEN,
            2,
            "a3s_cloud_platform_role_bindings_create",
            binding_arguments,
        ))
    );
    let rest_created = rest_created?;
    let mcp_created = mcp_created?;
    let rest_created_body = response_json(&rest_created)?;
    let mcp_created_body = response_json(&mcp_created)?;
    let mcp_created_result = &mcp_created_body["result"]["structuredContent"];
    let mut creation_codes = [
        rest_created.status(),
        required_u16(&mcp_created_result["code"])?,
    ];
    creation_codes.sort_unstable();
    assert_eq!(creation_codes, [200, 201]);
    assert_replay_pair(
        &rest_created_body["data"],
        &mcp_created_result["data"],
        "cross-surface binding creation",
    )?;
    let binding_id = required_string(&rest_created_body["data"]["id"], "binding ID")?;
    assert_eq!(mcp_created_result["data"]["id"], binding_id);
    assert_eq!(rest_created_body["data"]["aggregateVersion"], 1);
    assert_eq!(mcp_created_result["data"]["aggregateVersion"], 1);
    assert_create_evidence(&database, &binding_id).await?;

    let rest_after_binding = first
        .call(get_as("/api/v1/platform/role-policy", OPERATOR_TOKEN))
        .await?;
    assert_eq!(rest_after_binding.status(), 200);
    let mcp_after_binding = second
        .call(management_mcp_request(
            OPERATOR_TOKEN,
            3,
            "a3s_cloud_platform_role_policy_current_get",
            json!({}),
        ))
        .await?;
    assert_mcp_code(&mcp_after_binding, 200, false)?;
    assert_eq!(
        response_json(&rest_after_binding)?["data"],
        response_json(&mcp_after_binding)?["result"]["structuredContent"]["data"]
    );

    let (rest_revoked, mcp_revoked) = tokio::join!(
        first.call(post_json(
            format!("/api/v1/platform/role-bindings/{binding_id}/revocation"),
            REVOCATION_IDEMPOTENCY_KEY,
            json!({"expectedVersion": 1}),
        )),
        second.call(management_mcp_request(
            ADMIN_TOKEN,
            4,
            "a3s_cloud_platform_role_bindings_revoke",
            json!({
                "bindingId": binding_id,
                "expectedVersion": 1,
                "idempotencyKey": REVOCATION_IDEMPOTENCY_KEY
            }),
        ))
    );
    let rest_revoked = rest_revoked?;
    let mcp_revoked = mcp_revoked?;
    assert_eq!(rest_revoked.status(), 200);
    let rest_revoked_body = response_json(&rest_revoked)?;
    let mcp_revoked_body = response_json(&mcp_revoked)?;
    let mcp_revoked_result = &mcp_revoked_body["result"]["structuredContent"];
    assert_eq!(required_u16(&mcp_revoked_result["code"])?, 200);
    assert_replay_pair(
        &rest_revoked_body["data"],
        &mcp_revoked_result["data"],
        "cross-surface binding revocation",
    )?;
    assert_eq!(rest_revoked_body["data"]["id"], binding_id);
    assert_eq!(mcp_revoked_result["data"]["id"], binding_id);
    assert_eq!(rest_revoked_body["data"]["aggregateVersion"], 2);
    assert_eq!(mcp_revoked_result["data"]["aggregateVersion"], 2);
    assert!(rest_revoked_body["data"]["revokedAt"].is_string());
    assert_eq!(
        rest_revoked_body["data"]["revokedAt"],
        mcp_revoked_result["data"]["revokedAt"]
    );
    assert_revocation_evidence(&database, &binding_id).await?;
    assert_policy_denied_on_both_surfaces(&first, &second).await?;

    let rest_binding = first
        .call(get_as(
            format!("/api/v1/platform/role-bindings/{binding_id}"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(rest_binding.status(), 200);
    let mcp_binding = second
        .call(management_mcp_request(
            ADMIN_TOKEN,
            5,
            "a3s_cloud_platform_role_bindings_get",
            json!({"bindingId": binding_id}),
        ))
        .await?;
    assert_mcp_code(&mcp_binding, 200, false)?;
    assert_eq!(
        response_json(&rest_binding)?["data"],
        response_json(&mcp_binding)?["result"]["structuredContent"]["data"],
        "REST and MCP must expose one revoked binding projection"
    );
    Ok(())
}

async fn build_instance(
    root: &std::path::Path,
) -> Result<ControlPlane, Box<dyn std::error::Error>> {
    let mut application_config = config();
    application_config.postgres.serving_url_env = POSTGRES_URL_ENV.into();
    application_config.auth.bootstrap_token_env = BOOTSTRAP_TOKEN_ENV.into();
    configure_ephemeral_application_state(&mut application_config, root);
    Ok(build_application(application_config).await?)
}

async fn bootstrap(app: &ControlPlane) -> Result<String, Box<dyn std::error::Error>> {
    let response = app
        .call(
            post_json(
                "/api/v1/bootstrap",
                "pmcs:bootstrap",
                json!({
                    "organizationName": "Privileged Management Cross Surface",
                    "tokenName": "pmcs-owner",
                    "token": ADMIN_TOKEN,
                    "expiresAt": null
                }),
            )
            .with_header("x-a3s-bootstrap-token", BOOTSTRAP_TOKEN_VALUE),
        )
        .await?;
    assert_eq!(response.status(), 201);
    required_string(
        &response_json(&response)?["data"]["organization"]["id"],
        "bootstrap organization ID",
    )
}

async fn create_human_principal(
    app: &ControlPlane,
    organization_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization_id}/memberships"),
            "pmcs:operator-membership",
            json!({
                "principalKind": "human",
                "name": "PMCS operator",
                "role": "member"
            }),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    required_string(
        &response_json(&response)?["data"]["principalId"],
        "operator Principal ID",
    )
}

async fn create_operator_token(
    app: &ControlPlane,
    organization_id: &str,
    principal_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization_id}/api-tokens"),
            "pmcs:operator-token",
            json!({
                "principalId": principal_id,
                "name": "PMCS operator",
                "token": OPERATOR_TOKEN,
                "scopes": ["cloud:read"],
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    assert!(!String::from_utf8_lossy(response.body()).contains(OPERATOR_TOKEN));
    Ok(())
}

async fn assert_policy_denied_on_both_surfaces(
    rest: &ControlPlane,
    mcp: &ControlPlane,
) -> Result<(), Box<dyn std::error::Error>> {
    let rest_denied = rest
        .call(get_as("/api/v1/platform/role-policy", OPERATOR_TOKEN))
        .await?;
    assert_eq!(rest_denied.status(), 403);
    let rest_error = response_json(&rest_denied)?;
    let mcp_denied = mcp
        .call(management_mcp_request(
            OPERATOR_TOKEN,
            20,
            "a3s_cloud_platform_role_policy_current_get",
            json!({}),
        ))
        .await?;
    assert_mcp_code(&mcp_denied, 403, true)?;
    let mcp_error = response_json(&mcp_denied)?;
    assert_eq!(
        rest_error["message"], mcp_error["result"]["structuredContent"]["message"],
        "REST and MCP authorization denial must have one Application error"
    );
    Ok(())
}

async fn assert_create_evidence(
    database: &Database<PostgresDialect, PostgresExecutor>,
    binding_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let binding_id = Uuid::parse_str(binding_id)?;
    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64)>(
                "select
                    (select count(*) from platform_role_bindings where id = ",
            )
            .bind(binding_id)
            .append(" and aggregate_version = 1 and revoked_at is null),
                    (select count(*) from audit_records where action = 'identity.platform-role-binding.created' and aggregate_id = ")
            .bind(binding_id)
            .append("),
                    (select count(*) from outbox_events where event_key = 'identity.platform-role-binding.created' and aggregate_id = ")
            .bind(binding_id)
            .append("),
                    (select count(*) from audit_records where action = 'identity.privileged-access.authorize' and details ->> 'action' = 'identity.platform-role-binding.created' and details ->> 'resourceId' = ")
            .bind(binding_id.to_string())
            .append("),
                    (select count(*) from idempotency_records where scope_key = 'installation/platform-role-bindings' and idempotency_key = ")
            .bind(BINDING_IDEMPOTENCY_KEY)
            .append(")"),
        )
        .await?;
    assert_eq!(
        evidence,
        (1, 1, 1, 2, 1),
        "two surface requests must share one binding/business fact/idempotency result while retaining one authorization decision per request"
    );
    Ok(())
}

async fn assert_revocation_evidence(
    database: &Database<PostgresDialect, PostgresExecutor>,
    binding_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let binding_id = Uuid::parse_str(binding_id)?;
    let revocation_scope = format!("installation/platform-role-bindings/{binding_id}/revoke");
    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64)>(
                "select
                    (select count(*) from platform_role_bindings where id = ",
            )
            .bind(binding_id)
            .append(" and aggregate_version = 2 and revoked_at is not null),
                    (select count(*) from audit_records where action = 'identity.platform-role-binding.revoked' and aggregate_id = ")
            .bind(binding_id)
            .append("),
                    (select count(*) from outbox_events where event_key = 'identity.platform-role-binding.revoked' and aggregate_id = ")
            .bind(binding_id)
            .append("),
                    (select count(*) from audit_records where action = 'identity.privileged-access.authorize' and details ->> 'action' = 'identity.platform-role-binding.revoked' and details ->> 'resourceId' = ")
            .bind(binding_id.to_string())
            .append("),
                    (select count(*) from idempotency_records where scope_key = ")
            .bind(revocation_scope)
            .append(" and idempotency_key = ")
            .bind(REVOCATION_IDEMPOTENCY_KEY)
            .append(")"),
        )
        .await?;
    assert_eq!(
        evidence,
        (1, 1, 1, 2, 1),
        "two revocation requests must share one terminal state/business fact/idempotency result while retaining one authorization decision per request"
    );
    Ok(())
}

fn assert_replay_pair(
    rest: &Value,
    mcp: &Value,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let rest_replayed = rest["replayed"]
        .as_bool()
        .ok_or_else(|| std::io::Error::other(format!("{label} REST replay flag is missing")))?;
    let mcp_replayed = mcp["replayed"]
        .as_bool()
        .ok_or_else(|| std::io::Error::other(format!("{label} MCP replay flag is missing")))?;
    assert_ne!(
        rest_replayed, mcp_replayed,
        "{label} must commit exactly once"
    );
    Ok(())
}

fn assert_mcp_code(
    response: &BootResponse,
    code: u16,
    is_error: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(response.status(), 200);
    let body = response_json(response)?;
    assert_eq!(body["result"]["isError"], is_error);
    assert_eq!(
        required_u16(&body["result"]["structuredContent"]["code"])?,
        code
    );
    Ok(())
}

fn management_mcp_request(token: &str, id: u64, name: &str, arguments: Value) -> BootRequest {
    let version = a3s_cloud_contracts::MCP_PROTOCOL_VERSION;
    let body = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": name,
            "arguments": arguments,
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": version,
                "io.modelcontextprotocol/clientInfo": {
                    "name": "a3s-cloud-privileged-management-postgres",
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
        .with_header("mcp-protocol-version", version)
        .with_header("mcp-method", "tools/call")
        .with_header("mcp-name", name)
        .with_body(body.to_string().into_bytes())
}

fn required_string(value: &Value, label: &str) -> Result<String, Box<dyn std::error::Error>> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| std::io::Error::other(format!("{label} is missing")).into())
}

fn required_u16(value: &Value) -> Result<u16, Box<dyn std::error::Error>> {
    value
        .as_u64()
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| std::io::Error::other("MCP structured status code is missing").into())
}
