use super::*;

#[tokio::test]
async fn tenant_administrators_query_bounded_redacted_audit_history() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let audit = Arc::new(InMemoryAuditRecordRepository::new());
    let app = build_test_application_with_audit_records(identity, projects, audit.clone())?;
    let organization = bootstrap_organization(&app, "audit-query", "Audit query").await?;
    let organization_id = OrganizationId::from_uuid(parse_audit_uuid(&organization)?);
    let other_organization = create_organization(&app, "audit-query-other", "Other").await?;
    let other_organization_id = OrganizationId::from_uuid(parse_audit_uuid(&other_organization)?);

    let member = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "audit-query-member",
            json!({"name": "Audit member", "role": "member"}),
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
            "audit-query-member-token",
            json!({
                "name": "Audit member",
                "token": AUDIT_MEMBER_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ],
                "principalId": member_principal,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(member_token.status(), 201);

    let actor = PrincipalId::new();
    let request_id = Uuid::now_v7();
    let aggregate_ids = [Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7()];
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let attribution_profile_id = ProjectAttributionProfileId::new();
    let now = Utc::now();
    for (index, aggregate_id) in aggregate_ids.into_iter().enumerate() {
        audit
            .register(AuditRecord {
                id: Uuid::now_v7(),
                organization_id,
                actor_principal_id: Some(actor),
                action: if index == 1 {
                    "identity.membership.revoked".into()
                } else {
                    "identity.membership.created".into()
                },
                aggregate_id,
                occurred_at: now + chrono::Duration::seconds(index as i64),
                request_id,
                project_id: (index > 0).then_some(project_id),
                environment_id: (index == 2).then_some(environment_id),
                attribution_profile_id: (index == 2).then_some(attribution_profile_id),
                attribution_status: match index {
                    0 => AuditAttributionStatus::LegacyUnknown,
                    1 => AuditAttributionStatus::ProfileMissing,
                    _ => AuditAttributionStatus::ProfileBound,
                },
            })
            .await
            .map_err(|error| BootError::Internal(error.to_string()))?;
    }
    let hidden_id = Uuid::now_v7();
    audit
        .register(AuditRecord {
            id: hidden_id,
            organization_id: other_organization_id,
            actor_principal_id: Some(actor),
            action: "identity.membership.created".into(),
            aggregate_id: Uuid::now_v7(),
            occurred_at: now + chrono::Duration::seconds(10),
            request_id,
            project_id: None,
            environment_id: None,
            attribution_profile_id: None,
            attribution_status: AuditAttributionStatus::NotApplicable,
        })
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;

    let path = format!(
        "/api/v1/organizations/{organization}/audit-records?actorPrincipalId={actor}&requestId={request_id}&limit=2"
    );
    let first = app.call(get_as(path, ADMIN_TOKEN)).await?;
    assert_eq!(first.status(), 200);
    let first = response_json(&first)?;
    assert_eq!(first["data"]["records"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        first["data"]["records"][0]["aggregateId"],
        aggregate_ids[2].to_string()
    );
    assert_eq!(
        first["data"]["records"][1]["aggregateId"],
        aggregate_ids[1].to_string()
    );
    assert!(first["data"]["records"][0].get("details").is_none());
    assert_eq!(
        first["data"]["records"][0]["projectId"],
        project_id.to_string()
    );
    assert_eq!(
        first["data"]["records"][0]["environmentId"],
        environment_id.to_string()
    );
    assert_eq!(
        first["data"]["records"][0]["attributionProfileId"],
        attribution_profile_id.to_string()
    );
    assert_eq!(
        first["data"]["records"][0]["attributionStatus"],
        "profile_bound"
    );
    assert!(!first.to_string().contains(&hidden_id.to_string()));
    let cursor = first["data"]["nextCursor"]
        .as_str()
        .ok_or_else(|| BootError::Internal("next audit cursor is missing".into()))?;
    let second = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/audit-records?actorPrincipalId={actor}&requestId={request_id}&limit=2&cursor={cursor}"
            ),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(second.status(), 200);
    let second = response_json(&second)?;
    assert_eq!(second["data"]["records"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        second["data"]["records"][0]["aggregateId"],
        aggregate_ids[0].to_string()
    );
    assert_eq!(second["data"]["nextCursor"], Value::Null);

    let mcp = app
        .call(mcp_tool_call_as(
            1,
            "a3s_cloud_audit_records_list",
            json!({
                "actorPrincipalId": actor,
                "action": "identity.membership.created",
                "requestId": request_id,
                "projectId": project_id,
                "environmentId": environment_id,
                "attributionProfileId": attribution_profile_id,
                "attributionStatus": "profile_bound",
                "limit": 1
            }),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(mcp.status(), 200);
    let mcp = response_json(&mcp)?;
    assert_eq!(mcp["result"]["isError"], false);
    assert_eq!(
        mcp["result"]["structuredContent"]["data"]["records"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert!(mcp["result"]["structuredContent"]["data"]["records"][0]
        .get("details")
        .is_none());
    assert_eq!(
        mcp["result"]["structuredContent"]["data"]["records"][0]["attributionStatus"],
        "profile_bound"
    );

    let export_from = url::form_urlencoded::byte_serialize(
        (now - chrono::Duration::seconds(1)).to_rfc3339().as_bytes(),
    )
    .collect::<String>();
    let export_to = url::form_urlencoded::byte_serialize(
        (now + chrono::Duration::seconds(3)).to_rfc3339().as_bytes(),
    )
    .collect::<String>();
    let export = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/audit-records/export?from={export_from}&to={export_to}&limit=2"
            ),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(export.status(), 200);
    let export = response_json(&export)?;
    let payload = verified_audit_export_payload(&export["data"])?;
    assert_eq!(payload["schema"], "a3s.cloud.audit-export.v1");
    assert_eq!(payload["organizationId"], organization);
    assert_eq!(payload["filter"]["limit"], 2);
    assert_eq!(payload["records"].as_array().map(Vec::len), Some(2));
    assert!(payload["nextCursor"].is_string());
    assert!(!payload.to_string().contains("details"));
    for private in ["labels", "businessOwnerReference", "costAttributionCode"] {
        assert!(!payload.to_string().contains(private));
    }

    let export_mcp = app
        .call(mcp_tool_call_as(
            3,
            "a3s_cloud_audit_records_export",
            json!({
                "from": (now - chrono::Duration::seconds(1)).to_rfc3339(),
                "to": (now + chrono::Duration::seconds(3)).to_rfc3339(),
                "attributionStatus": "profile_bound",
                "limit": 1
            }),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(export_mcp.status(), 200);
    let export_mcp = response_json(&export_mcp)?;
    assert_eq!(export_mcp["result"]["isError"], false);
    let mcp_payload =
        verified_audit_export_payload(&export_mcp["result"]["structuredContent"]["data"])?;
    assert_eq!(mcp_payload["records"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        mcp_payload["records"][0]["attributionStatus"],
        "profile_bound"
    );

    let retention = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/audit-records/retention"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(retention.status(), 200);
    let retention = response_json(&retention)?;
    assert_eq!(retention["data"]["organizationId"], organization);
    assert_eq!(retention["data"]["retentionMs"], 7_776_000_000_u64);
    assert!(retention["data"]["policyDigest"]
        .as_str()
        .is_some_and(|value| value.starts_with("sha256:")));
    assert_eq!(retention["data"]["appliedPolicyDigest"], Value::Null);
    assert_eq!(retention["data"]["currentPolicyApplied"], false);
    assert_eq!(retention["data"]["recordsAvailableFrom"], Value::Null);
    assert_eq!(retention["data"]["recordsDeletedBefore"], Value::Null);
    assert_eq!(retention["data"]["totalDeletedRecords"], 0);
    assert_eq!(retention["data"]["version"], 0);

    let retention_mcp = app
        .call(mcp_tool_call_as(
            5,
            "a3s_cloud_audit_retention_get",
            json!({}),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(retention_mcp.status(), 200);
    let retention_mcp = response_json(&retention_mcp)?;
    assert_eq!(retention_mcp["result"]["isError"], false);
    assert_eq!(
        retention_mcp["result"]["structuredContent"]["data"],
        retention["data"]
    );

    let filtered = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/audit-records?action=identity.membership.revoked&aggregateId={}&from={}&to={} ",
                aggregate_ids[1],
                url::form_urlencoded::byte_serialize(now.to_rfc3339().as_bytes()).collect::<String>(),
                url::form_urlencoded::byte_serialize(
                    (now + chrono::Duration::seconds(3)).to_rfc3339().as_bytes()
                )
                .collect::<String>()
            )
            .trim_end()
            .to_owned(),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(filtered.status(), 200);
    assert_eq!(
        response_json(&filtered)?["data"]["records"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );

    let member_denied = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/audit-records"),
            AUDIT_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(member_denied.status(), 403);
    let member_export_denied = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/audit-records/export?from={export_from}&to={export_to}"
            ),
            AUDIT_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(member_export_denied.status(), 403);
    let member_retention_denied = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/audit-records/retention"),
            AUDIT_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(member_retention_denied.status(), 403);
    let member_mcp = app
        .call(mcp_tool_call_as(
            2,
            "a3s_cloud_audit_records_list",
            json!({}),
            AUDIT_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(member_mcp.status(), 200);
    assert_eq!(response_json(&member_mcp)?["error"]["code"], -32602);
    let member_export_mcp = app
        .call(mcp_tool_call_as(
            4,
            "a3s_cloud_audit_records_export",
            json!({
                "from": now.to_rfc3339(),
                "to": (now + chrono::Duration::seconds(3)).to_rfc3339()
            }),
            AUDIT_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(member_export_mcp.status(), 200);
    assert_eq!(response_json(&member_export_mcp)?["error"]["code"], -32602);
    let member_retention_mcp = app
        .call(mcp_tool_call_as(
            6,
            "a3s_cloud_audit_retention_get",
            json!({}),
            AUDIT_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(member_retention_mcp.status(), 200);
    assert_eq!(
        response_json(&member_retention_mcp)?["error"]["code"],
        -32602
    );
    let cross_tenant = app
        .call(get_as(
            format!("/api/v1/organizations/{other_organization}/audit-records"),
            AUDIT_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(cross_tenant.status(), 403);

    let query_count = audit.query_count();
    for suffix in [
        "limit=0",
        "limit=201",
        "cursor=untrusted",
        "action=Invalid.action.value",
        "aggregateId=00000000-0000-0000-0000-000000000000",
        "projectId=00000000-0000-0000-0000-000000000000",
        "attributionStatus=invalid",
        "from=2026-08-14T00%3A00%3A00Z&to=2026-08-13T00%3A00%3A00Z",
    ] {
        let response = app
            .call(get_as(
                format!("/api/v1/organizations/{organization}/audit-records?{suffix}"),
                ADMIN_TOKEN,
            ))
            .await?;
        assert!(
            matches!(response.status(), 400 | 422),
            "unexpected status for {suffix}: {}",
            response.status()
        );
    }
    for suffix in [
        "to=2026-08-13T00%3A00%3A00Z",
        "from=2026-07-01T00%3A00%3A00Z&to=2026-08-02T00%3A00%3A00Z",
    ] {
        let response = app
            .call(get_as(
                format!("/api/v1/organizations/{organization}/audit-records/export?{suffix}"),
                ADMIN_TOKEN,
            ))
            .await?;
        assert!(
            matches!(response.status(), 400 | 422),
            "unexpected export status for {suffix}: {}",
            response.status()
        );
    }
    assert_eq!(audit.query_count(), query_count);
    Ok(())
}

fn verified_audit_export_payload(export: &Value) -> Result<Value> {
    let payload_type = export["envelope"]["payloadType"]
        .as_str()
        .ok_or_else(|| BootError::Internal("audit export payload type is missing".into()))?;
    assert_eq!(
        payload_type,
        "application/vnd.a3s.cloud.audit-export.v1+json"
    );
    let payload = STANDARD
        .decode(
            export["envelope"]["payload"]
                .as_str()
                .ok_or_else(|| BootError::Internal("audit export payload is missing".into()))?,
        )
        .map_err(|error| BootError::Internal(format!("invalid audit export payload: {error}")))?;
    let signature = STANDARD
        .decode(
            export["envelope"]["signatures"][0]["signature"]
                .as_str()
                .ok_or_else(|| BootError::Internal("audit export signature is missing".into()))?,
        )
        .map_err(|error| BootError::Internal(format!("invalid audit export signature: {error}")))?;
    let public_key = STANDARD
        .decode(
            export["signingKey"]["publicKey"]
                .as_str()
                .ok_or_else(|| BootError::Internal("audit export public key is missing".into()))?,
        )
        .map_err(|error| {
            BootError::Internal(format!("invalid audit export public key: {error}"))
        })?;
    let key_id = format!("{:x}", Sha256::digest(&public_key));
    assert_eq!(export["signingKey"]["algorithm"], "ed25519");
    assert_eq!(export["signingKey"]["keyId"], key_id);
    assert_eq!(export["envelope"]["signatures"][0]["keyId"], key_id);
    let pae = crate::modules::shared_kernel::domain::dsse_pae_bounded(
        payload_type,
        &payload,
        crate::modules::audit::MAXIMUM_AUDIT_EXPORT_BYTES,
    )
    .map_err(BootError::Internal)?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public_key)
        .verify(&pae, &signature)
        .map_err(|_| BootError::Internal("audit export signature did not verify".into()))?;
    serde_json::from_slice(&payload)
        .map_err(|error| BootError::Internal(format!("invalid audit export document: {error}")))
}

fn parse_audit_uuid(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value)
        .map_err(|error| BootError::Internal(format!("invalid audit test UUID: {error}")))
}
