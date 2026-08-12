use super::*;
use a3s_cloud_control_plane::ControlPlane;
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use std::collections::BTreeSet;

const POSTGRES_URL_ENV: &str = "A3S_CLOUD_RG3_POSTGRES_URL";
const BOOTSTRAP_TOKEN_ENV: &str = "A3S_CLOUD_RG3_BOOTSTRAP_TOKEN";
const BOOTSTRAP_TOKEN_VALUE: &str = "rg3-bootstrap-credential-0123456789abcdef";

struct MembershipFixture {
    id: String,
    token: String,
}

pub async fn exercise_resource_grant_matrix(
    postgres_url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = connect_and_migrate(&postgres_url, 8).await?;
    let database = Database::new(PostgresDialect, executor);
    let _postgres_url = EnvironmentOverride::set(POSTGRES_URL_ENV, &postgres_url);
    let _bootstrap_token = EnvironmentOverride::set(BOOTSTRAP_TOKEN_ENV, BOOTSTRAP_TOKEN_VALUE);
    let state = tempfile::tempdir()?;
    let mut application_config = config();
    application_config.postgres.url_env = POSTGRES_URL_ENV.into();
    application_config.auth.bootstrap_token_env = BOOTSTRAP_TOKEN_ENV.into();
    configure_ephemeral_application_state(&mut application_config, state.path());
    let app = build_application(application_config).await?;

    let organization = bootstrap(&app).await?;
    let foreign_organization =
        create_organization(&app, "rg3:foreign-organization", "RG3 Foreign").await?;
    let admin = create_membership(&app, &organization, "admin", '1').await?;
    let member = create_membership(&app, &organization, "member", '2').await?;
    let restricted = create_membership(&app, &organization, "restricted", '3').await?;

    let granted_project =
        create_project(&app, &organization, "rg3:project-granted", "RG3 Granted").await?;
    let denied_project =
        create_project(&app, &organization, "rg3:project-denied", "RG3 Denied").await?;
    let granted_environment = create_environment(
        &app,
        &organization,
        &granted_project,
        "rg3:environment-granted",
        "RG3 Granted",
        ADMIN_TOKEN,
    )
    .await?;
    let denied_environment = create_environment(
        &app,
        &organization,
        &granted_project,
        "rg3:environment-denied",
        "RG3 Denied",
        ADMIN_TOKEN,
    )
    .await?;

    for (role, token) in [
        ("owner", ADMIN_TOKEN),
        ("admin", admin.token.as_str()),
        ("member", member.token.as_str()),
    ] {
        let response = create_environment(
            &app,
            &organization,
            &denied_project,
            &format!("rg3:role-command-{role}"),
            &format!("RG3 {role} command"),
            token,
        )
        .await?;
        assert!(
            !response.is_empty(),
            "{role} command must return an environment ID"
        );
    }

    let granted_execution = create_execution(
        &app,
        &organization,
        &granted_project,
        &granted_environment,
        "rg3:execution-granted",
    )
    .await?;
    let denied_execution = create_execution(
        &app,
        &organization,
        &granted_project,
        &denied_environment,
        "rg3:execution-denied",
    )
    .await?;
    seed_execution_operation(&database, &organization, &granted_execution).await?;
    seed_execution_operation(&database, &organization, &denied_execution).await?;
    let granted_node = enroll_node(&app, &organization, "granted", 'd').await?;
    let denied_node = enroll_node(&app, &organization, "denied", 'e').await?;

    let project_collection = format!("/api/v1/organizations/{organization}/projects");
    let node_collection = format!("/api/v1/organizations/{organization}/nodes");
    let operation_collection = format!("/api/v1/organizations/{organization}/operations");
    let operation_stream = format!("{operation_collection}/stream");
    let granted_execution_path =
        format!("/api/v1/organizations/{organization}/executions/{granted_execution}");
    let denied_execution_path =
        format!("/api/v1/organizations/{organization}/executions/{denied_execution}");

    for (role, token) in [
        ("owner", ADMIN_TOKEN),
        ("admin", admin.token.as_str()),
        ("member", member.token.as_str()),
    ] {
        assert_rest_ids(
            &app,
            get_as(&project_collection, token),
            "id",
            [&granted_project, &denied_project],
            role,
        )
        .await?;
        assert_rest_ids(
            &app,
            get_as(&node_collection, token),
            "id",
            [&granted_node, &denied_node],
            role,
        )
        .await?;
        assert_eq!(
            app.call(get_as(&granted_execution_path, token))
                .await?
                .status(),
            200,
            "{role} must retain organization-wide indirect access"
        );
    }

    for path in [&project_collection, &node_collection, &operation_collection] {
        assert_eq!(
            app.call(get_as(path, &restricted.token)).await?.status(),
            403,
            "a restricted membership without grants must fail closed"
        );
    }
    assert_eq!(
        app.call(get_as(
            format!("/api/v1/organizations/{foreign_organization}/projects"),
            &restricted.token,
        ))
        .await?
        .status(),
        403,
        "a Resource Grant cannot cross the authenticated tenant boundary"
    );

    let grants = format!(
        "/api/v1/organizations/{organization}/memberships/{}/resource-grants",
        restricted.id
    );
    let project_scope = json!({
        "kind": "project",
        "projectId": granted_project,
    });
    let project_grant = create_grant(&app, &grants, "rg3:grant-project", &project_scope).await?;
    let project_replay = app
        .call(management_mcp_request(
            ADMIN_TOKEN,
            1,
            "a3s_cloud_resource_grants_create",
            json!({
                "membershipId": restricted.id,
                "scope": project_scope,
                "idempotencyKey": "rg3:grant-project"
            }),
        ))
        .await?;
    assert_mcp_write(&project_replay, 200, true, Some(&project_grant))?;

    assert_rest_ids(
        &app,
        get_as(&project_collection, &restricted.token),
        "id",
        [&granted_project],
        "restricted project grant",
    )
    .await?;
    let granted_environments =
        format!("/api/v1/organizations/{organization}/projects/{granted_project}/environments");
    assert_rest_ids(
        &app,
        get_as(&granted_environments, &restricted.token),
        "id",
        [&granted_environment, &denied_environment],
        "project ancestry",
    )
    .await?;
    for execution in [&granted_execution_path, &denied_execution_path] {
        assert_eq!(
            app.call(get_as(execution, &restricted.token))
                .await?
                .status(),
            200,
            "a project grant must cover descendant executions"
        );
    }
    assert_eq!(
        app.call(get_as(
            format!("/api/v1/organizations/{organization}/projects/{denied_project}/environments"),
            &restricted.token,
        ))
        .await?
        .status(),
        403
    );
    let mcp_projects = app
        .call(management_mcp_request(
            &restricted.token,
            2,
            "a3s_cloud_projects_list",
            json!({}),
        ))
        .await?;
    assert_mcp_ids(
        &mcp_projects,
        "id",
        [&granted_project],
        "project collection",
    )?;
    let mcp_environments = app
        .call(management_mcp_request(
            &restricted.token,
            3,
            "a3s_cloud_environments_list",
            json!({"projectId": granted_project}),
        ))
        .await?;
    assert_mcp_ids(
        &mcp_environments,
        "id",
        [&granted_environment, &denied_environment],
        "project ancestry",
    )?;

    let project_revocation = app
        .call(management_mcp_request(
            ADMIN_TOKEN,
            4,
            "a3s_cloud_resource_grants_revoke",
            json!({
                "resourceGrantId": project_grant,
                "expectedVersion": 1,
                "idempotencyKey": "rg3:revoke-project"
            }),
        ))
        .await?;
    assert_mcp_write(&project_revocation, 200, false, Some(&project_grant))?;
    let project_revocation_replay = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/resource-grants/{project_grant}/revocation"
            ),
            "rg3:revoke-project",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(project_revocation_replay.status(), 200);
    assert_eq!(
        response_json(&project_revocation_replay)?["data"]["replayed"],
        true
    );
    assert_eq!(
        app.call(get_as(&project_collection, &restricted.token))
            .await?
            .status(),
        403
    );

    let node_scope = json!({"kind": "node", "nodeId": granted_node});
    let node_grant = create_grant(&app, &grants, "rg3:grant-node", &node_scope).await?;
    assert_rest_ids(
        &app,
        get_as(&node_collection, &restricted.token),
        "id",
        [&granted_node],
        "exact node grant",
    )
    .await?;
    let granted_node_path = format!("{node_collection}/{granted_node}");
    let denied_node_path = format!("{node_collection}/{denied_node}");
    assert_eq!(
        app.call(get_as(&granted_node_path, &restricted.token))
            .await?
            .status(),
        200
    );
    assert_error_equivalent(
        &app,
        get_as(&denied_node_path, &restricted.token),
        get_as(
            format!("{node_collection}/{}", Uuid::now_v7()),
            &restricted.token,
        ),
        403,
    )
    .await?;
    let mcp_nodes = app
        .call(management_mcp_request(
            &restricted.token,
            5,
            "a3s_cloud_nodes_list",
            json!({}),
        ))
        .await?;
    assert_mcp_ids(&mcp_nodes, "id", [&granted_node], "exact node grant")?;

    let drain = || {
        post_json_as(
            format!("{granted_node_path}/actions/drain"),
            "rg3:drain-node",
            json!({"expectedVersion": 1}),
            &restricted.token,
        )
    };
    assert_eq!(app.call(drain()).await?.status(), 200);
    let drain_replay = app.call(drain()).await?;
    assert_eq!(drain_replay.status(), 200);
    assert_eq!(response_json(&drain_replay)?["data"]["replayed"], true);

    let node_revocation_path =
        format!("/api/v1/organizations/{organization}/resource-grants/{node_grant}/revocation");
    let node_revocation = app
        .call(post_json(
            &node_revocation_path,
            "rg3:revoke-node",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(node_revocation.status(), 200);
    let node_revocation_replay = app
        .call(post_json(
            &node_revocation_path,
            "rg3:revoke-node",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(node_revocation_replay.status(), 200);
    assert_eq!(
        response_json(&node_revocation_replay)?["data"]["replayed"],
        true
    );
    assert_eq!(
        app.call(drain()).await?.status(),
        403,
        "node grant revocation must run before an idempotent command replay"
    );

    let environment_scope = json!({
        "kind": "environment",
        "projectId": granted_project,
        "environmentId": granted_environment,
    });
    let environment_grant =
        create_grant(&app, &grants, "rg3:grant-environment", &environment_scope).await?;
    let environment_grant_replay = app
        .call(post_json(
            &grants,
            "rg3:grant-environment",
            json!({"scope": environment_scope}),
        ))
        .await?;
    assert_eq!(environment_grant_replay.status(), 200);
    assert_eq!(
        response_json(&environment_grant_replay)?["data"]["replayed"],
        true
    );

    assert_rest_ids(
        &app,
        get_as(&project_collection, &restricted.token),
        "id",
        [&granted_project],
        "environment parent projection",
    )
    .await?;
    assert_rest_ids(
        &app,
        get_as(&granted_environments, &restricted.token),
        "id",
        [&granted_environment],
        "exact environment grant",
    )
    .await?;
    assert_eq!(
        app.call(get_as(&granted_execution_path, &restricted.token))
            .await?
            .status(),
        200
    );
    assert_error_equivalent(
        &app,
        get_as(&denied_execution_path, &restricted.token),
        get_as(
            format!(
                "/api/v1/organizations/{organization}/executions/{}",
                Uuid::now_v7()
            ),
            &restricted.token,
        ),
        404,
    )
    .await?;
    assert_rest_ids(
        &app,
        get_as(&operation_collection, &restricted.token),
        "subjectId",
        [&granted_execution],
        "REST Operation subject filtering",
    )
    .await?;
    let mcp_operations = app
        .call(management_mcp_request(
            &restricted.token,
            6,
            "a3s_cloud_operations_list",
            json!({"limit": 50}),
        ))
        .await?;
    assert_mcp_ids(
        &mcp_operations,
        "subjectId",
        [&granted_execution],
        "MCP Operation subject filtering",
    )?;
    let stream_before_revocation = app
        .call(
            get_as(&operation_stream, &restricted.token).with_header("accept", "text/event-stream"),
        )
        .await?;
    assert_eq!(stream_before_revocation.status(), 200);
    assert!(stream_before_revocation.is_streaming());
    assert!(stream_before_revocation.is_event_stream());

    let cancel = || {
        delete_as(
            &granted_execution_path,
            "rg3:cancel-execution",
            &restricted.token,
        )
    };
    assert_eq!(app.call(cancel()).await?.status(), 202);
    let cancel_replay = app.call(cancel()).await?;
    assert_eq!(cancel_replay.status(), 200);
    assert_eq!(response_json(&cancel_replay)?["data"]["replayed"], true);

    let fallback_scope = json!({
        "kind": "environment",
        "projectId": granted_project,
        "environmentId": denied_environment,
    });
    let fallback_grant = create_grant(
        &app,
        &grants,
        "rg3:grant-environment-fallback",
        &fallback_scope,
    )
    .await?;
    revoke_grant(
        &app,
        &organization,
        &environment_grant,
        "rg3:revoke-environment",
    )
    .await?;
    assert_error_equivalent(
        &app,
        cancel(),
        delete_as(
            format!(
                "/api/v1/organizations/{organization}/executions/{}",
                Uuid::now_v7()
            ),
            "rg3:cancel-execution",
            &restricted.token,
        ),
        404,
    )
    .await?;
    assert_rest_ids(
        &app,
        get_as(&operation_collection, &restricted.token),
        "subjectId",
        [&denied_execution],
        "revoked Operation subject filtering",
    )
    .await?;
    let mcp_operations_after_revocation = app
        .call(management_mcp_request(
            &restricted.token,
            7,
            "a3s_cloud_operations_list",
            json!({"limit": 50}),
        ))
        .await?;
    assert_mcp_ids(
        &mcp_operations_after_revocation,
        "subjectId",
        [&denied_execution],
        "revoked MCP Operation subject filtering",
    )?;
    let stream_after_revocation = app
        .call(
            get_as(&operation_stream, &restricted.token).with_header("accept", "text/event-stream"),
        )
        .await?;
    assert_eq!(stream_after_revocation.status(), 200);
    assert!(stream_after_revocation.is_event_stream());

    revoke_grant(
        &app,
        &organization,
        &fallback_grant,
        "rg3:revoke-environment-fallback",
    )
    .await?;
    assert_eq!(
        app.call(
            get_as(&operation_stream, &restricted.token).with_header("accept", "text/event-stream"),
        )
        .await?
        .status(),
        403,
        "a stream reconnect must load the current active Resource Grants"
    );

    drop(stream_before_revocation);
    drop(stream_after_revocation);
    let organization_id = Uuid::parse_str(&organization)?;
    let restricted_membership_id = Uuid::parse_str(&restricted.id)?;
    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64)>(
                "select (select count(*) from resource_grants where organization_id = ",
            )
            .bind(organization_id)
            .append(" and membership_id = ")
            .bind(restricted_membership_id)
            .append("), (select count(*) from resource_grants where organization_id = ")
            .bind(organization_id)
            .append(" and membership_id = ")
            .bind(restricted_membership_id)
            .append(" and aggregate_version = 2 and revoked_at is not null), (select count(*) from audit_records where aggregate_id in (select id from resource_grants where organization_id = ")
            .bind(organization_id)
            .append(" and membership_id = ")
            .bind(restricted_membership_id)
            .append(") and action like 'identity.resource-grant.%'), (select count(*) from outbox_events where aggregate_id in (select id from resource_grants where organization_id = ")
            .bind(organization_id)
            .append(" and membership_id = ")
            .bind(restricted_membership_id)
            .append(") and event_key like 'identity.resource-grant.%'), (select count(*) from idempotency_records where idempotency_key like 'rg3:grant-%' or idempotency_key like 'rg3:revoke-%')"),
        )
        .await?;
    assert_eq!(
        evidence,
        (4, 4, 8, 8, 8),
        "Grant state, audit, Outbox, and idempotency must commit exactly once"
    );
    Ok(())
}

async fn bootstrap(app: &ControlPlane) -> Result<String, Box<dyn std::error::Error>> {
    let response = app
        .call(
            post_json(
                "/api/v1/bootstrap",
                "rg3:bootstrap",
                json!({
                    "organizationName": "RG3 Matrix",
                    "tokenName": "rg3-owner",
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

async fn create_organization(
    app: &ControlPlane,
    idempotency_key: &str,
    name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let response = app
        .call(post_json(
            "/api/v1/organizations",
            idempotency_key,
            json!({"name": name}),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    Ok(response_id(&response)?)
}

async fn create_membership(
    app: &ControlPlane,
    organization: &str,
    role: &str,
    token_character: char,
) -> Result<MembershipFixture, Box<dyn std::error::Error>> {
    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            &format!("rg3:membership-{role}"),
            json!({"name": format!("RG3 {role}"), "role": role}),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let membership_body = response_json(&membership)?;
    let membership_id = required_string(&membership_body["data"]["id"], "membership ID")?;
    let principal_id = required_string(
        &membership_body["data"]["principalId"],
        "membership principal ID",
    )?;
    let token = format!("a3s_{}", token_character.to_string().repeat(64));
    let token_response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            &format!("rg3:token-{role}"),
            json!({
                "name": format!("RG3 {role}"),
                "token": token,
                "scopes": [
                    "cloud:read",
                    "environment:write",
                    "node:write",
                    "execution:write"
                ],
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(token_response.status(), 201);
    Ok(MembershipFixture {
        id: membership_id,
        token,
    })
}

async fn create_project(
    app: &ControlPlane,
    organization: &str,
    idempotency_key: &str,
    name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects"),
            idempotency_key,
            json!({"name": name}),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    Ok(response_id(&response)?)
}

async fn create_environment(
    app: &ControlPlane,
    organization: &str,
    project: &str,
    idempotency_key: &str,
    name: &str,
    token: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let response = app
        .call(post_json_as(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            idempotency_key,
            json!({"name": name}),
            token,
        ))
        .await?;
    assert_eq!(
        response.status(),
        201,
        "environment creation failed for {name}"
    );
    Ok(response_id(&response)?)
}

async fn create_execution(
    app: &ControlPlane,
    organization: &str,
    project: &str,
    environment: &str,
    idempotency_key: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let digest = format!("sha256:{}", "a".repeat(64));
    let response = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/executions"
            ),
            idempotency_key,
            json!({
                "artifact": {
                    "uri": format!("oci://registry.example/functions/rg3@{digest}"),
                    "digest": digest,
                    "mediaType": "application/vnd.oci.image.manifest.v1+json"
                },
                "process": {
                    "command": ["/app/rg3"],
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
        ))
        .await?;
    assert_eq!(response.status(), 202);
    required_string(
        &response_json(&response)?["data"]["execution"]["id"],
        "execution ID",
    )
}

async fn seed_execution_operation(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization: &str,
    execution: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let organization = Uuid::parse_str(organization)?;
    let execution = Uuid::parse_str(execution)?;
    database
        .execute(
            sql_query::<()>(concat!(
                "insert into operation_requests (operation_id, organization_id, subject_kind, ",
                "subject_id, workflow_name, workflow_version, input, requested_at) ",
                "select operation_id, organization_id, 'execution', id, 'cloud.execution', '1', ",
                "jsonb_build_object('organizationId', organization_id, 'executionId', id), ",
                "requested_at from executions where organization_id = "
            ))
            .bind(organization)
            .append(" and id = ")
            .bind(execution)
            .append(" on conflict (operation_id) do nothing"),
        )
        .await?;
    Ok(())
}

async fn enroll_node(
    app: &ControlPlane,
    organization: &str,
    label: &str,
    secret_character: char,
) -> Result<String, Box<dyn std::error::Error>> {
    let secret = format!("a3sn_{}", secret_character.to_string().repeat(64));
    let issued = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/enrollment-tokens"),
            &format!("rg3:enrollment-token-{label}"),
            json!({
                "name": format!("RG3 {label}"),
                "token": secret,
                "expiresAt": Utc::now() + chrono::Duration::minutes(10)
            }),
        ))
        .await?;
    assert_eq!(issued.status(), 201);
    let enrolled = app
        .call(
            BootRequest::new(HttpMethod::Post, "/api/v1/node-control/enroll")
                .with_header("content-type", "application/json")
                .with_body(
                    json!({
                        "schema": "a3s.cloud.node-enrollment-request.v1",
                        "enrollment_token": secret,
                        "node_name": format!("rg3-{label}"),
                        "agent_instance_id": Uuid::now_v7(),
                        "agent_version": "0.1.0",
                        "csr_pem": certificate_request()?,
                        "runtime_capabilities": runtime_capabilities()
                    })
                    .to_string()
                    .into_bytes(),
                ),
        )
        .await?;
    assert_eq!(enrolled.status(), 201);
    required_string(&response_json(&enrolled)?["node_id"], "enrolled node ID")
}

async fn create_grant(
    app: &ControlPlane,
    collection: &str,
    idempotency_key: &str,
    scope: &Value,
) -> Result<String, Box<dyn std::error::Error>> {
    let response = app
        .call(post_json(
            collection,
            idempotency_key,
            json!({"scope": scope}),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    Ok(response_id(&response)?)
}

async fn revoke_grant(
    app: &ControlPlane,
    organization: &str,
    grant: &str,
    idempotency_key: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = format!("/api/v1/organizations/{organization}/resource-grants/{grant}/revocation");
    let response = app
        .call(post_json(
            &path,
            idempotency_key,
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(response.status(), 200);
    let replay = app
        .call(post_json(
            path,
            idempotency_key,
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(replay.status(), 200);
    assert_eq!(response_json(&replay)?["data"]["replayed"], true);
    Ok(())
}

async fn assert_rest_ids<const N: usize>(
    app: &ControlPlane,
    request: BootRequest,
    field: &str,
    expected: [&String; N],
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = app.call(request).await?;
    assert_eq!(response.status(), 200, "{label} REST collection status");
    let body = response_json(&response)?;
    assert_eq!(
        collection_values(&body["data"], field)?,
        expected.into_iter().cloned().collect(),
        "{label} REST collection"
    );
    Ok(())
}

fn assert_mcp_ids<const N: usize>(
    response: &BootResponse,
    field: &str,
    expected: [&String; N],
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(response.status(), 200, "{label} MCP status");
    let body = response_json(response)?;
    assert_eq!(body["result"]["isError"], false, "{label} MCP result");
    assert_eq!(
        collection_values(&body["result"]["structuredContent"]["data"], field)?,
        expected.into_iter().cloned().collect(),
        "{label} MCP collection"
    );
    Ok(())
}

fn collection_values(
    data: &Value,
    field: &str,
) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    data.as_array()
        .ok_or_else(|| std::io::Error::other("collection response data is not an array"))?
        .iter()
        .map(|item| required_string(&item[field], field))
        .collect()
}

async fn assert_error_equivalent(
    app: &ControlPlane,
    denied: BootRequest,
    missing: BootRequest,
    status: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let denied = app.call(denied).await?;
    let missing = app.call(missing).await?;
    assert_eq!(denied.status(), status);
    assert_eq!(missing.status(), status);
    let denied = response_json(&denied)?;
    let missing = response_json(&missing)?;
    for field in ["code", "statusCode", "message", "details"] {
        assert_eq!(denied[field], missing[field], "error field {field}");
    }
    Ok(())
}

fn assert_mcp_write(
    response: &BootResponse,
    code: u16,
    replayed: bool,
    expected_id: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(response.status(), 200);
    let body = response_json(response)?;
    let result = &body["result"]["structuredContent"];
    assert_eq!(result["code"], code);
    assert_eq!(result["data"]["replayed"], replayed);
    if let Some(expected_id) = expected_id {
        assert_eq!(result["data"]["id"], expected_id);
    }
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
                    "name": "a3s-cloud-rg3-postgres",
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

fn certificate_request() -> Result<String, Box<dyn std::error::Error>> {
    let key = KeyPair::generate()?;
    let mut params = CertificateParams::default();
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "rg3-postgres-node");
    params.distinguished_name = name;
    Ok(params.serialize_request(&key)?.pem()?)
}

fn runtime_capabilities() -> Value {
    json!({
        "schema": "a3s.runtime.capabilities.v4",
        "provider_id": "a3s-box",
        "provider_build": "a3s-box-rg3-postgres",
        "unit_classes": ["task", "service"],
        "artifact_media_types": ["application/vnd.oci.image.manifest.v1+json"],
        "isolation_levels": ["sandbox"],
        "network_modes": ["none", "service"],
        "mount_kinds": [],
        "health_check_kinds": [],
        "resource_controls": ["cpu", "memory", "pids", "ephemeral_storage"],
        "features": ["durable_identity", "stop", "remove", "service_tcp"]
    })
}
