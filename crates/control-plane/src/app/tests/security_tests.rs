use super::*;
use crate::modules::edge::domain::events::{
    MCP_ROUTE_POLICY_CREATED_EVENT_KEY, MCP_ROUTE_POLICY_REVISED_EVENT_KEY,
};
use crate::modules::security::{GatewayRoutePolicyTimelineEntry, SecurityAuditCorrelation};
use crate::modules::shared_kernel::domain::{canonical_timestamp, Sha256Digest};

const SECURITY_ADMIN_TOKEN: &str =
    "a3s_3333333333333333333333333333333333333333333333333333333333333333";

#[tokio::test]
async fn tenant_administrators_query_bounded_redacted_gateway_route_policy_timeline() -> Result<()>
{
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let security = Arc::new(InMemoryGatewayRoutePolicyTimelineRepository::new());
    let app =
        build_test_application_with_security_investigations(identity, projects, security.clone())?;
    let organization = bootstrap_organization(&app, "security-timeline", "Security").await?;
    let organization_id = OrganizationId::from_uuid(parse_security_uuid(&organization)?);
    let other_organization = create_organization(&app, "security-timeline-other", "Other").await?;
    let other_organization_id =
        OrganizationId::from_uuid(parse_security_uuid(&other_organization)?);

    let administrator = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "security-timeline-administrator",
            json!({"name": "Security administrator", "role": "admin"}),
        ))
        .await?;
    assert_eq!(administrator.status(), 201);
    let administrator_principal = response_json(&administrator)?["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("administrator Principal ID is missing".into()))?
        .to_owned();
    let administrator_token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "security-timeline-administrator-token",
            json!({
                "name": "Security administrator",
                "token": SECURITY_ADMIN_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ],
                "principalId": administrator_principal,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(administrator_token.status(), 201);

    let member = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "security-timeline-member",
            json!({"name": "Security member", "role": "member"}),
        ))
        .await?;
    assert_eq!(member.status(), 201);
    let member_principal = response_json(&member)?["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("member Principal ID is missing".into()))?
        .to_owned();
    let member_token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "security-timeline-member-token",
            json!({
                "name": "Security member",
                "token": AUDIT_MEMBER_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ],
                "principalId": member_principal,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(member_token.status(), 201);

    let route_id = RouteId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let actor_id = PrincipalId::new();
    let audit_id = Uuid::now_v7();
    let now = canonical_timestamp(Utc::now());
    for revision in 1..=3 {
        security
            .register(timeline_entry(
                organization_id,
                project_id,
                environment_id,
                route_id,
                revision,
                now + chrono::Duration::seconds(revision as i64),
                (revision == 2).then_some((audit_id, actor_id)),
            ))
            .await
            .map_err(|error| BootError::Internal(error.to_string()))?;
    }
    security
        .register(timeline_entry(
            other_organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            route_id,
            1,
            now + chrono::Duration::seconds(10),
            None,
        ))
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;
    security
        .register(timeline_entry(
            organization_id,
            project_id,
            environment_id,
            RouteId::new(),
            1,
            now + chrono::Duration::seconds(11),
            None,
        ))
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;

    let path = format!(
        "/api/v1/organizations/{organization}/security-investigations/gateway-routes/{route_id}/timeline?limit=2"
    );
    let first = app.call(get_as(path, SECURITY_ADMIN_TOKEN)).await?;
    assert_eq!(first.status(), 200);
    let first = response_json(&first)?;
    let entries = first["data"]["entries"]
        .as_array()
        .ok_or_else(|| BootError::Internal("security timeline entries are missing".into()))?;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["policyRevision"], 3);
    assert_eq!(entries[0]["auditCorrelation"], "missing");
    assert_eq!(entries[1]["policyRevision"], 2);
    assert_eq!(entries[1]["auditCorrelation"], "verified");
    assert_eq!(entries[1]["auditRecordId"], audit_id.to_string());
    assert_eq!(entries[1]["actorPrincipalId"], actor_id.to_string());
    for forbidden in ["payload", "details", "canonicalAcl", "privateError"] {
        assert!(!first.to_string().contains(forbidden));
    }
    let cursor = first["data"]["nextCursor"]
        .as_str()
        .ok_or_else(|| BootError::Internal("security timeline cursor is missing".into()))?;
    let second = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/security-investigations/gateway-routes/{route_id}/timeline?limit=2&cursor={cursor}"
            ),
            SECURITY_ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(second.status(), 200);
    let second = response_json(&second)?;
    assert_eq!(second["data"]["entries"].as_array().map(Vec::len), Some(1));
    assert_eq!(second["data"]["entries"][0]["policyRevision"], 1);
    assert_eq!(second["data"]["nextCursor"], Value::Null);

    let mcp = app
        .call(mcp_tool_call_as(
            1,
            "a3s_cloud_security_gateway_route_policy_timeline_list",
            json!({"routeId": route_id, "limit": 1}),
            SECURITY_ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(mcp.status(), 200);
    let mcp = response_json(&mcp)?;
    assert_eq!(mcp["result"]["isError"], false);
    assert_eq!(
        mcp["result"]["structuredContent"]["data"]["entries"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert!(mcp["result"]["structuredContent"]["data"]["entries"][0]
        .get("details")
        .is_none());

    let member_denied = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/security-investigations/gateway-routes/{route_id}/timeline"
            ),
            AUDIT_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(member_denied.status(), 403);
    let member_mcp = app
        .call(mcp_tool_call_as(
            2,
            "a3s_cloud_security_gateway_route_policy_timeline_list",
            json!({"routeId": route_id}),
            AUDIT_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(member_mcp.status(), 200);
    assert_eq!(response_json(&member_mcp)?["error"]["code"], -32602);
    let cross_tenant = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{other_organization}/security-investigations/gateway-routes/{route_id}/timeline"
            ),
            SECURITY_ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(cross_tenant.status(), 403);

    let query_count = security.query_count();
    for suffix in ["limit=0", "limit=101", "cursor=untrusted"] {
        let response = app
            .call(get_as(
                format!(
                    "/api/v1/organizations/{organization}/security-investigations/gateway-routes/{route_id}/timeline?{suffix}"
                ),
                SECURITY_ADMIN_TOKEN,
            ))
            .await?;
        assert!(matches!(response.status(), 400 | 422));
    }
    assert_eq!(security.query_count(), query_count);
    Ok(())
}

fn timeline_entry(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    route_id: RouteId,
    policy_revision: u64,
    occurred_at: DateTime<Utc>,
    audit: Option<(Uuid, PrincipalId)>,
) -> GatewayRoutePolicyTimelineEntry {
    GatewayRoutePolicyTimelineEntry {
        event_id: Uuid::now_v7(),
        event_key: if policy_revision == 1 {
            MCP_ROUTE_POLICY_CREATED_EVENT_KEY
        } else {
            MCP_ROUTE_POLICY_REVISED_EVENT_KEY
        }
        .into(),
        schema_version: 1,
        organization_id,
        project_id,
        environment_id,
        route_id,
        policy_revision,
        policy_digest: Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest"),
        occurred_at,
        correlation_id: Uuid::now_v7(),
        audit_correlation: if audit.is_some() {
            SecurityAuditCorrelation::Verified
        } else {
            SecurityAuditCorrelation::Missing
        },
        audit_record_id: audit.map(|value| value.0),
        actor_principal_id: audit.map(|value| value.1),
    }
}

fn parse_security_uuid(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value)
        .map_err(|error| BootError::Internal(format!("invalid security test UUID: {error}")))
}
