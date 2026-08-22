//! HTTP and MCP contract tests for the personal notification inbox.

use super::*;
use crate::modules::notifications::{
    INotificationRepository, Notification, NotificationAlertPolicyDefinition,
    NotificationAlertPolicySpec, NotificationAlertSource, NotificationScope, NotificationSeverity,
    OutboundNotificationChannel, OutboundNotificationConnectorTarget,
    OutboundNotificationSubscriptionDefinition, OutboundNotificationSubscriptionSpec,
};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId,
};

const NOTIFICATION_MEMBER_TOKEN: &str =
    "a3s_3333333333333333333333333333333333333333333333333333333333333333";
const NOTIFICATION_FOREIGN_TOKEN: &str =
    "a3s_9999999999999999999999999999999999999999999999999999999999999999";

#[tokio::test]
async fn personal_inbox_is_recipient_bound_paginated_and_idempotently_read() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let notifications =
        Arc::new(crate::modules::notifications::InMemoryNotificationRepository::new());
    let app =
        build_test_application_with_notifications(identity, projects, Arc::clone(&notifications))?;
    let organization = bootstrap_organization(&app, "notification-bootstrap", "Inbox").await?;
    let organization_id = organization_id(&organization)?;
    let owner = owner_principal(&app, &organization).await?;

    create_api_token(
        &app,
        &organization,
        "notification-read-only-token",
        "Notification read only",
        PROJECT_TOKEN,
        &[ApiTokenScope::CLOUD_READ],
        None,
    )
    .await?;

    let older = projected_notification(
        organization_id,
        owner,
        NotificationScope::Organization,
        "Organization access granted",
        0,
    );
    let newer = projected_notification(
        organization_id,
        owner,
        NotificationScope::Organization,
        "Organization role changed",
        1,
    );
    let foreign = projected_notification(
        organization_id,
        PrincipalId::new(),
        NotificationScope::Organization,
        "Another Principal's notification",
        2,
    );
    for notification in [&older, &newer, &foreign] {
        assert!(notifications
            .project(notification.clone())
            .await
            .map_err(|error| BootError::Internal(error.to_string()))?);
    }

    let root = format!("/api/v1/organizations/{organization}/notifications");
    let first_page = app
        .call(get_as(format!("{root}?limit=1"), ADMIN_TOKEN))
        .await?;
    assert_eq!(first_page.status(), 200);
    let first_page = response_json(&first_page)?;
    assert_eq!(
        first_page["data"]["notifications"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        first_page["data"]["notifications"][0]["id"],
        newer.id.to_string()
    );
    assert!(first_page["data"]["notifications"][0]
        .get("recipientPrincipalId")
        .is_none());
    let cursor = first_page["data"]["nextCursor"]
        .as_str()
        .ok_or_else(|| BootError::Internal("notification cursor is missing".into()))?;
    let second_page = app
        .call(get_as(
            format!("{root}?limit=1&cursor={cursor}"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(second_page.status(), 200);
    let second_page = response_json(&second_page)?;
    assert_eq!(
        second_page["data"]["notifications"][0]["id"],
        older.id.to_string()
    );
    assert!(second_page["data"]["nextCursor"].is_null());
    assert!(!second_page.to_string().contains(&foreign.id.to_string()));

    let exact = app
        .call(get_as(format!("{root}/{}", newer.id), ADMIN_TOKEN))
        .await?;
    assert_eq!(exact.status(), 200);
    assert_eq!(
        response_json(&exact)?["data"]["scope"]["kind"],
        "organization"
    );
    assert_eq!(
        app.call(get_as(format!("{root}/{}", foreign.id), ADMIN_TOKEN))
            .await?
            .status(),
        404
    );

    let read_request = || {
        post_json(
            format!("{root}/{}/read", newer.id),
            "notification:read:newer",
            json!({"expectedVersion": 1}),
        )
    };
    let scope_denied = app
        .call(post_json_as(
            format!("{root}/{}/read", newer.id),
            "notification:read:scope-denied",
            json!({"expectedVersion": 1}),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(scope_denied.status(), 403);

    let read = app.call(read_request()).await?;
    assert_eq!(read.status(), 200);
    let read = response_json(&read)?;
    assert_eq!(read["data"]["notification"]["aggregateVersion"], 2);
    assert!(read["data"]["notification"]["readAt"].is_string());
    assert_eq!(read["data"]["replayed"], false);
    let replayed = app.call(read_request()).await?;
    assert_eq!(replayed.status(), 200);
    assert_eq!(response_json(&replayed)?["data"]["replayed"], true);
    assert_eq!(notifications.outbox_events().await.len(), 1);

    let changed_replay = app
        .call(post_json(
            format!("{root}/{}/read", newer.id),
            "notification:read:newer",
            json!({"expectedVersion": 2}),
        ))
        .await?;
    assert_eq!(changed_replay.status(), 409);
    let unread = app
        .call(get_as(
            format!("{root}?unreadOnly=true&limit=50"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(unread.status(), 200);
    let unread = response_json(&unread)?;
    assert_eq!(
        unread["data"]["notifications"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        unread["data"]["notifications"][0]["id"],
        older.id.to_string()
    );

    let mcp = app
        .call(mcp_tool_call_as(
            1,
            "a3s_cloud_notifications_get",
            json!({"notificationId": older.id}),
            ADMIN_TOKEN,
        ))
        .await?;
    let mcp = response_json(&mcp)?;
    assert_eq!(mcp["result"]["isError"], false);
    assert_eq!(
        mcp["result"]["structuredContent"]["data"]["id"],
        older.id.to_string()
    );

    assert_eq!(
        app.call(get_as(format!("{root}?cursor=untrusted"), ADMIN_TOKEN))
            .await?
            .status(),
        422
    );
    assert_eq!(
        app.call(get_as(format!("{root}?limit=0"), ADMIN_TOKEN))
            .await?
            .status(),
        400
    );
    Ok(())
}

#[tokio::test]
async fn restricted_inbox_reuses_resource_grants_for_rest_and_mcp() -> Result<()> {
    let notifications =
        Arc::new(crate::modules::notifications::InMemoryNotificationRepository::new());
    let app = build_test_application_with_notifications(
        Arc::new(InMemoryIdentityRepository::new()),
        Arc::new(InMemoryProjectsRepository::new()),
        Arc::clone(&notifications),
    )?;
    let organization =
        bootstrap_organization(&app, "notification-grants-bootstrap", "Inbox grants").await?;
    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "notification-grants-membership",
            json!({"name": "Restricted inbox reader", "role": "restricted"}),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let membership = response_json(&membership)?;
    let membership_id = membership["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("notification membership ID is missing".into()))?;
    let principal = membership["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("notification Principal ID is missing".into()))?;
    let principal_id = PrincipalId::from_uuid(
        Uuid::parse_str(principal)
            .map_err(|error| BootError::Internal(format!("invalid Principal ID: {error}")))?,
    );
    let token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "notification-grants-token",
            json!({
                "name": "Restricted inbox reader",
                "token": NOTIFICATION_MEMBER_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ, ApiTokenScope::NOTIFICATION_WRITE],
                "principalId": principal,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(token.status(), 201);

    let granted_project = create_project(
        &app,
        &organization,
        "notification-granted-project",
        "Granted",
    )
    .await?;
    let hidden_project =
        create_project(&app, &organization, "notification-hidden-project", "Hidden").await?;
    let grant = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/memberships/{membership_id}/resource-grants"
            ),
            "notification-project-grant",
            json!({"scope": {"kind": "project", "projectId": granted_project}}),
        ))
        .await?;
    assert_eq!(grant.status(), 201);

    let organization_id = organization_id(&organization)?;
    let granted_project_id = ProjectId::from_uuid(
        Uuid::parse_str(&granted_project)
            .map_err(|error| BootError::Internal(format!("invalid Project ID: {error}")))?,
    );
    let hidden_project_id = ProjectId::from_uuid(
        Uuid::parse_str(&hidden_project)
            .map_err(|error| BootError::Internal(format!("invalid Project ID: {error}")))?,
    );
    let organization_notification = projected_notification(
        organization_id,
        principal_id,
        NotificationScope::Organization,
        "Organization notice",
        0,
    );
    let granted = projected_notification(
        organization_id,
        principal_id,
        NotificationScope::Project {
            project_id: granted_project_id,
        },
        "Granted project notice",
        1,
    );
    let hidden = projected_notification(
        organization_id,
        principal_id,
        NotificationScope::Project {
            project_id: hidden_project_id,
        },
        "Hidden project notice",
        2,
    );
    for notification in [&organization_notification, &granted, &hidden] {
        notifications
            .project(notification.clone())
            .await
            .map_err(|error| BootError::Internal(error.to_string()))?;
    }

    let root = format!("/api/v1/organizations/{organization}/notifications");
    let listed = app
        .call(get_as(
            format!("{root}?limit=50"),
            NOTIFICATION_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(listed.status(), 200);
    let listed = response_json(&listed)?;
    let ids = listed["data"]["notifications"]
        .as_array()
        .ok_or_else(|| BootError::Internal("notification list is not an array".into()))?
        .iter()
        .filter_map(|value| value["id"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    assert!(ids.contains(&organization_notification.id.to_string()));
    assert!(ids.contains(&granted.id.to_string()));
    assert!(!ids.contains(&hidden.id.to_string()));
    assert_eq!(
        app.call(get_as(
            format!("{root}/{}", hidden.id),
            NOTIFICATION_MEMBER_TOKEN,
        ))
        .await?
        .status(),
        404
    );
    assert_eq!(
        app.call(post_json_as(
            format!("{root}/{}/read", hidden.id),
            "notification-hidden-read",
            json!({"expectedVersion": 1}),
            NOTIFICATION_MEMBER_TOKEN,
        ))
        .await?
        .status(),
        404
    );

    let mcp = app
        .call(mcp_tool_call_as(
            2,
            "a3s_cloud_notifications_list",
            json!({"limit": 50}),
            NOTIFICATION_MEMBER_TOKEN,
        ))
        .await?;
    let mcp = response_json(&mcp)?;
    assert_eq!(mcp["result"]["isError"], false);
    let mcp_text = mcp["result"]["structuredContent"].to_string();
    assert!(mcp_text.contains(&granted.id.to_string()));
    assert!(!mcp_text.contains(&hidden.id.to_string()));
    Ok(())
}

#[tokio::test]
async fn outbound_subscription_management_is_acl_native_recipient_bound_and_cross_surface(
) -> Result<()> {
    let notifications =
        Arc::new(crate::modules::notifications::InMemoryNotificationRepository::new());
    let app = build_test_application_with_notifications(
        Arc::new(InMemoryIdentityRepository::new()),
        Arc::new(InMemoryProjectsRepository::new()),
        Arc::clone(&notifications),
    )?;
    let organization =
        bootstrap_organization(&app, "notification-outbound-bootstrap", "Outbound").await?;
    create_api_token(
        &app,
        &organization,
        "notification-outbound-writer",
        "Outbound writer",
        NOTIFICATION_MEMBER_TOKEN,
        &[ApiTokenScope::NOTIFICATION_WRITE],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "notification-outbound-reader",
        "Outbound reader",
        PROJECT_TOKEN,
        &[ApiTokenScope::CLOUD_READ],
        None,
    )
    .await?;

    let project = create_project(
        &app,
        &organization,
        "notification-outbound-project",
        "Outbound project",
    )
    .await?;
    let environment = crate::app::tests::connector_tests::create_connector_environment(
        &app,
        &organization,
        &project,
        "notification-outbound-environment",
    )
    .await?;
    let connector_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/connector-profiles"
    );
    let connector_acl = crate::app::tests::connector_tests::connector_acl(1_000)?;
    let connector = app
        .call(post_json(
            &connector_path,
            "notification-outbound-connector",
            json!({"name": "Outbound webhook", "definitionAcl": connector_acl}),
        ))
        .await?;
    assert_eq!(connector.status(), 201);
    let connector = response_json(&connector)?;
    let profile_id = connector["data"]["record"]["profile"]["profileId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("outbound Connector profile ID is missing".into()))?;
    let revision_id = connector["data"]["record"]["revision"]["revisionId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("outbound Connector revision ID is missing".into()))?;
    let signed_acl = outbound_subscription_acl(
        &project,
        &environment,
        profile_id,
        revision_id,
        OutboundNotificationChannel::SignedWebhook,
    )?;
    let slack_acl = outbound_subscription_acl_with_budget(
        &project,
        &environment,
        profile_id,
        revision_id,
        OutboundNotificationChannel::SlackCompatible,
        3,
    )?;
    let root = format!("/api/v1/organizations/{organization}/notification-outbound-subscriptions");

    assert_eq!(
        app.call(post_acl_as(
            &root,
            "notification-outbound-read-denied",
            signed_acl.clone(),
            PROJECT_TOKEN,
        ))
        .await?
        .status(),
        403
    );
    let created = app
        .call(post_acl_as(
            &root,
            "notification-outbound-create",
            signed_acl.clone(),
            NOTIFICATION_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(created.status(), 201);
    let created = response_json(&created)?;
    assert_eq!(created["data"]["replayed"], false);
    assert_eq!(created["data"]["subscription"]["state"], "active");
    assert_eq!(created["data"]["subscription"]["definitionAcl"], signed_acl);
    assert_eq!(
        created["data"]["subscription"]["definitionSchema"],
        "cloud.notification.outbound-subscription.v1"
    );
    assert_eq!(
        created["data"]["subscription"]["maximumProviderAttempts"],
        8
    );
    assert_eq!(
        created["data"]["subscription"]["suppressBefore"],
        Value::Null
    );
    assert!(created["data"]["subscription"]
        .get("recipientPrincipalId")
        .is_none());
    assert!(!created.to_string().contains("hooks.example.test"));
    let subscription_id = created["data"]["subscription"]["subscriptionId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("outbound subscription ID is missing".into()))?
        .to_owned();

    let replay = app
        .call(post_acl_as(
            &root,
            "notification-outbound-create",
            signed_acl.clone(),
            NOTIFICATION_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(replay.status(), 200);
    assert_eq!(response_json(&replay)?["data"]["replayed"], true);
    assert_eq!(
        app.call(post_acl_as(
            &root,
            "notification-outbound-create",
            slack_acl.clone(),
            NOTIFICATION_MEMBER_TOKEN,
        ))
        .await?
        .status(),
        409
    );
    assert_eq!(
        app.call(get_as(&root, NOTIFICATION_MEMBER_TOKEN))
            .await?
            .status(),
        403
    );

    let mcp_create = app
        .call(mcp_tool_call_as(
            3,
            "a3s_cloud_notification_outbound_subscriptions_create",
            json!({
                "definitionAcl": slack_acl,
                "idempotencyKey": "notification-outbound-mcp-create"
            }),
            NOTIFICATION_MEMBER_TOKEN,
        ))
        .await?;
    let mcp_create = response_json(&mcp_create)?;
    assert_eq!(mcp_create["result"]["isError"], false);
    assert_eq!(
        mcp_create["result"]["structuredContent"]["data"]["subscription"]["definitionSchema"],
        "cloud.notification.outbound-subscription.v2"
    );
    assert_eq!(
        mcp_create["result"]["structuredContent"]["data"]["subscription"]
            ["maximumProviderAttempts"],
        3
    );
    assert_eq!(
        mcp_create["result"]["structuredContent"]["data"]["subscription"]["suppressBefore"],
        Value::Null
    );
    let second_subscription_id = mcp_create["result"]["structuredContent"]["data"]["subscription"]
        ["subscriptionId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP outbound subscription ID is missing".into()))?
        .to_owned();

    let first_page = app
        .call(get_as(format!("{root}?limit=1"), PROJECT_TOKEN))
        .await?;
    assert_eq!(first_page.status(), 200);
    let first_page = response_json(&first_page)?;
    assert_eq!(
        first_page["data"]["subscriptions"].as_array().map(Vec::len),
        Some(1)
    );
    let cursor = first_page["data"]["nextCursor"]
        .as_str()
        .ok_or_else(|| BootError::Internal("outbound subscription cursor is missing".into()))?;
    let second_page = app
        .call(get_as(
            format!("{root}?limit=1&cursor={cursor}"),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(second_page.status(), 200);
    assert_eq!(
        response_json(&second_page)?["data"]["subscriptions"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        app.call(get_as(format!("{root}?limit=201"), PROJECT_TOKEN))
            .await?
            .status(),
        400
    );
    let exact = app
        .call(get_as(format!("{root}/{subscription_id}"), PROJECT_TOKEN))
        .await?;
    assert_eq!(exact.status(), 200);
    assert_eq!(
        response_json(&exact)?["data"]["target"],
        json!({
            "kind": "connector",
            "projectId": project,
            "environmentId": environment,
            "profileId": profile_id,
            "revisionId": revision_id,
        })
    );
    assert_eq!(
        response_json(&exact)?["data"]["target"]["revisionId"],
        revision_id
    );

    let mcp_get = app
        .call(mcp_tool_call_as(
            4,
            "a3s_cloud_notification_outbound_subscriptions_get",
            json!({"subscriptionId": second_subscription_id.clone()}),
            PROJECT_TOKEN,
        ))
        .await?;
    let mcp_get = response_json(&mcp_get)?;
    assert_eq!(mcp_get["result"]["isError"], false);
    assert_eq!(
        mcp_get["result"]["structuredContent"]["data"]["definitionSchema"],
        "cloud.notification.outbound-subscription.v2"
    );
    assert_eq!(
        mcp_get["result"]["structuredContent"]["data"]["maximumProviderAttempts"],
        3
    );
    assert!(!mcp_get.to_string().contains("hooks.example.test"));

    let revoke = || {
        post_json_as(
            format!("{root}/{subscription_id}/revoke"),
            "notification-outbound-revoke",
            json!({"expectedVersion": 1}),
            NOTIFICATION_MEMBER_TOKEN,
        )
    };
    let revoked = app.call(revoke()).await?;
    assert_eq!(revoked.status(), 200);
    assert_eq!(
        response_json(&revoked)?["data"]["subscription"]["state"],
        "revoked"
    );
    assert_eq!(
        response_json(&app.call(revoke()).await?)?["data"]["replayed"],
        true
    );

    let suppress_before = crate::modules::shared_kernel::domain::canonical_timestamp(
        Utc::now() + chrono::Duration::days(1),
    );
    let suppressed_acl = outbound_subscription_acl_with_suppression(
        &project,
        &environment,
        profile_id,
        revision_id,
        OutboundNotificationChannel::SignedWebhook,
        2,
        suppress_before,
    )?;
    let suppressed = app
        .call(post_acl_as(
            &root,
            "notification-outbound-suppressed-create",
            suppressed_acl.clone(),
            NOTIFICATION_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(suppressed.status(), 201);
    let suppressed = response_json(&suppressed)?;
    assert_eq!(
        suppressed["data"]["subscription"]["definitionSchema"],
        "cloud.notification.outbound-subscription.v3"
    );
    assert_eq!(
        suppressed["data"]["subscription"]["maximumProviderAttempts"],
        2
    );
    assert_eq!(
        suppressed["data"]["subscription"]["suppressBefore"],
        serde_json::json!(suppress_before)
    );
    assert_eq!(
        suppressed["data"]["subscription"]["definitionAcl"],
        suppressed_acl
    );

    let mcp_revoke = app
        .call(mcp_tool_call_as(
            5,
            "a3s_cloud_notification_outbound_subscriptions_revoke",
            json!({
                "subscriptionId": second_subscription_id,
                "expectedVersion": 1,
                "idempotencyKey": "notification-outbound-mcp-revoke"
            }),
            NOTIFICATION_MEMBER_TOKEN,
        ))
        .await?;
    assert_eq!(response_json(&mcp_revoke)?["result"]["isError"], false);
    Ok(())
}

#[tokio::test]
async fn alert_policy_management_is_acl_native_recipient_bound_and_cross_surface() -> Result<()> {
    let notifications =
        Arc::new(crate::modules::notifications::InMemoryNotificationRepository::new());
    let app = build_test_application_with_notifications(
        Arc::new(InMemoryIdentityRepository::new()),
        Arc::new(InMemoryProjectsRepository::new()),
        Arc::clone(&notifications),
    )?;
    let organization =
        bootstrap_organization(&app, "notification-alert-bootstrap", "Alert policies").await?;
    create_api_token(
        &app,
        &organization,
        "notification-alert-writer",
        "Alert policy writer",
        NOTIFICATION_MEMBER_TOKEN,
        &[ApiTokenScope::NOTIFICATION_WRITE],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "notification-alert-reader",
        "Alert policy reader",
        PROJECT_TOKEN,
        &[ApiTokenScope::CLOUD_READ],
        None,
    )
    .await?;
    let project = create_project(
        &app,
        &organization,
        "notification-alert-project",
        "Alert project",
    )
    .await?;
    let environment = crate::app::tests::connector_tests::create_connector_environment(
        &app,
        &organization,
        &project,
        "notification-alert-environment",
    )
    .await?;
    let definition_acl = notification_alert_policy_acl(&project, &environment, true)?;
    let root = format!("/api/v1/organizations/{organization}/notification-alert-policies");

    assert_eq!(
        app.call(post_acl_as(
            &root,
            "notification-alert-read-denied",
            definition_acl.clone(),
            PROJECT_TOKEN,
        ))
        .await?
        .status(),
        403
    );
    let create = || {
        post_acl_as(
            &root,
            "notification-alert-create",
            definition_acl.clone(),
            NOTIFICATION_MEMBER_TOKEN,
        )
    };
    let created = app.call(create()).await?;
    assert_eq!(created.status(), 201);
    let created = response_json(&created)?;
    assert_eq!(created["data"]["replayed"], false);
    assert_eq!(created["data"]["policy"]["state"], "active");
    assert_eq!(
        created["data"]["policy"]["source"],
        "workload.deployment-health.v1"
    );
    assert_eq!(
        created["data"]["policy"]["definitionSchema"],
        "cloud.notification.alert-policy.v1"
    );
    assert_eq!(created["data"]["policy"]["definitionAcl"], definition_acl);
    assert!(created["data"]["policy"]
        .get("recipientPrincipalId")
        .is_none());
    let policy_id = created["data"]["policy"]["policyId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("notification alert policy ID is missing".into()))?
        .to_owned();

    let replay = app.call(create()).await?;
    assert_eq!(replay.status(), 200);
    assert_eq!(response_json(&replay)?["data"]["replayed"], true);
    let changed_acl = notification_alert_policy_acl(&project, &environment, false)?;
    assert_eq!(
        app.call(post_acl_as(
            &root,
            "notification-alert-create",
            changed_acl,
            NOTIFICATION_MEMBER_TOKEN,
        ))
        .await?
        .status(),
        409
    );
    assert_eq!(
        app.call(post_acl_as(
            &root,
            "notification-alert-duplicate-scope",
            definition_acl.clone(),
            NOTIFICATION_MEMBER_TOKEN,
        ))
        .await?
        .status(),
        409
    );

    let listed = app
        .call(get_as(format!("{root}?limit=1"), PROJECT_TOKEN))
        .await?;
    assert_eq!(listed.status(), 200);
    let listed = response_json(&listed)?;
    assert_eq!(listed["data"]["policies"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["data"]["policies"][0]["policyId"], policy_id);
    assert!(listed["data"]["nextCursor"].is_null());
    assert_eq!(
        app.call(get_as(format!("{root}?limit=201"), PROJECT_TOKEN))
            .await?
            .status(),
        400
    );
    let exact = app
        .call(get_as(format!("{root}/{policy_id}"), PROJECT_TOKEN))
        .await?;
    assert_eq!(exact.status(), 200);
    assert_eq!(response_json(&exact)?["data"]["environmentId"], environment);

    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "notification-alert-foreign-membership",
            json!({"name": "Foreign alert reader", "role": "member"}),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let membership = response_json(&membership)?;
    let foreign_principal = membership["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("foreign Principal ID is missing".into()))?;
    let foreign_token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "notification-alert-foreign-token",
            json!({
                "name": "Foreign alert reader",
                "token": NOTIFICATION_FOREIGN_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ, ApiTokenScope::NOTIFICATION_WRITE],
                "principalId": foreign_principal,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(foreign_token.status(), 201);
    assert_eq!(
        app.call(get_as(
            format!("{root}/{policy_id}"),
            NOTIFICATION_FOREIGN_TOKEN,
        ))
        .await?
        .status(),
        404
    );
    let foreign_list = app.call(get_as(&root, NOTIFICATION_FOREIGN_TOKEN)).await?;
    assert_eq!(foreign_list.status(), 200);
    assert!(response_json(&foreign_list)?["data"]["policies"]
        .as_array()
        .is_some_and(Vec::is_empty));

    let revoke = || {
        post_json_as(
            format!("{root}/{policy_id}/revoke"),
            "notification-alert-revoke",
            json!({"expectedVersion": 1}),
            NOTIFICATION_MEMBER_TOKEN,
        )
    };
    let revoked = app.call(revoke()).await?;
    assert_eq!(revoked.status(), 200);
    assert_eq!(
        response_json(&revoked)?["data"]["policy"]["state"],
        "revoked"
    );
    assert_eq!(
        response_json(&app.call(revoke()).await?)?["data"]["replayed"],
        true
    );

    let mcp_create = app
        .call(mcp_tool_call_as(
            6,
            "a3s_cloud_notification_alert_policies_create",
            json!({
                "definitionAcl": definition_acl,
                "idempotencyKey": "notification-alert-mcp-create"
            }),
            NOTIFICATION_MEMBER_TOKEN,
        ))
        .await?;
    let mcp_create = response_json(&mcp_create)?;
    assert_eq!(mcp_create["result"]["isError"], false);
    assert_eq!(mcp_create["result"]["structuredContent"]["code"], 201);
    let mcp_policy_id = mcp_create["result"]["structuredContent"]["data"]["policy"]["policyId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP alert policy ID is missing".into()))?
        .to_owned();
    assert_ne!(mcp_policy_id, policy_id);

    let mcp_list = app
        .call(mcp_tool_call_as(
            7,
            "a3s_cloud_notification_alert_policies_list",
            json!({"limit": 50}),
            PROJECT_TOKEN,
        ))
        .await?;
    let mcp_list = response_json(&mcp_list)?;
    assert_eq!(mcp_list["result"]["isError"], false);
    assert_eq!(
        mcp_list["result"]["structuredContent"]["data"]["policies"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    let mcp_get = app
        .call(mcp_tool_call_as(
            8,
            "a3s_cloud_notification_alert_policies_get",
            json!({"policyId": mcp_policy_id.clone()}),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(response_json(&mcp_get)?["result"]["isError"], false);
    let mcp_revoke = app
        .call(mcp_tool_call_as(
            9,
            "a3s_cloud_notification_alert_policies_revoke",
            json!({
                "policyId": mcp_policy_id,
                "expectedVersion": 1,
                "idempotencyKey": "notification-alert-mcp-revoke"
            }),
            NOTIFICATION_MEMBER_TOKEN,
        ))
        .await?;
    let mcp_revoke = response_json(&mcp_revoke)?;
    assert_eq!(mcp_revoke["result"]["isError"], false);
    assert_eq!(
        mcp_revoke["result"]["structuredContent"]["data"]["policy"]["state"],
        "revoked"
    );
    Ok(())
}

fn outbound_subscription_acl(
    project_id: &str,
    environment_id: &str,
    profile_id: &str,
    revision_id: &str,
    channel: OutboundNotificationChannel,
) -> Result<String> {
    outbound_subscription_definition(project_id, environment_id, profile_id, revision_id, channel)
        .and_then(OutboundNotificationSubscriptionDefinition::from_spec)
        .map(|definition| definition.canonical_acl().to_owned())
        .map_err(BootError::Internal)
}

fn notification_alert_policy_acl(
    project_id: &str,
    environment_id: &str,
    notify_on_recovery: bool,
) -> Result<String> {
    let parse = |value: &str, label: &str| {
        Uuid::parse_str(value)
            .map_err(|error| BootError::Internal(format!("invalid {label}: {error}")))
    };
    NotificationAlertPolicyDefinition::from_spec(NotificationAlertPolicySpec {
        source: NotificationAlertSource::WorkloadDeploymentHealthV1,
        project_id: ProjectId::from_uuid(parse(project_id, "project ID")?),
        environment_id: EnvironmentId::from_uuid(parse(environment_id, "environment ID")?),
        notify_on_recovery,
    })
    .map(|definition| definition.canonical_acl().to_owned())
    .map_err(BootError::Internal)
}

fn outbound_subscription_acl_with_budget(
    project_id: &str,
    environment_id: &str,
    profile_id: &str,
    revision_id: &str,
    channel: OutboundNotificationChannel,
    maximum_provider_attempts: u64,
) -> Result<String> {
    outbound_subscription_definition(project_id, environment_id, profile_id, revision_id, channel)
        .and_then(|spec| {
            OutboundNotificationSubscriptionDefinition::from_spec_with_provider_attempt_budget(
                spec,
                maximum_provider_attempts,
            )
        })
        .map(|definition| definition.canonical_acl().to_owned())
        .map_err(BootError::Internal)
}

#[allow(clippy::too_many_arguments)]
fn outbound_subscription_acl_with_suppression(
    project_id: &str,
    environment_id: &str,
    profile_id: &str,
    revision_id: &str,
    channel: OutboundNotificationChannel,
    maximum_provider_attempts: u64,
    suppress_before: chrono::DateTime<Utc>,
) -> Result<String> {
    outbound_subscription_definition(project_id, environment_id, profile_id, revision_id, channel)
        .and_then(|spec| {
            OutboundNotificationSubscriptionDefinition::from_spec_with_suppression(
                spec,
                maximum_provider_attempts,
                suppress_before,
            )
        })
        .map(|definition| definition.canonical_acl().to_owned())
        .map_err(BootError::Internal)
}

fn outbound_subscription_definition(
    project_id: &str,
    environment_id: &str,
    profile_id: &str,
    revision_id: &str,
    channel: OutboundNotificationChannel,
) -> std::result::Result<OutboundNotificationSubscriptionSpec, String> {
    let parse = |value: &str, label: &str| {
        Uuid::parse_str(value).map_err(|error| format!("invalid {label}: {error}"))
    };
    Ok(OutboundNotificationSubscriptionSpec {
        channel,
        minimum_severity: NotificationSeverity::Warning,
        target: OutboundNotificationConnectorTarget::new(
            ProjectId::from_uuid(parse(project_id, "project ID")?),
            EnvironmentId::from_uuid(parse(environment_id, "environment ID")?),
            ConnectorProfileId::from_uuid(parse(profile_id, "Connector profile ID")?),
            ConnectorRevisionId::from_uuid(parse(revision_id, "Connector revision ID")?),
        )
        .map_err(|error| error.to_string())?
        .into(),
    })
}

fn projected_notification(
    organization_id: OrganizationId,
    recipient_principal_id: PrincipalId,
    scope: NotificationScope,
    title: &str,
    offset_seconds: i64,
) -> Notification {
    let occurred_at = Utc::now() + chrono::Duration::seconds(offset_seconds);
    Notification::project(
        organization_id,
        recipient_principal_id,
        Uuid::now_v7(),
        "identity.membership.role-changed".into(),
        1,
        Uuid::now_v7(),
        1,
        Uuid::now_v7(),
        NotificationSeverity::Information,
        title.into(),
        format!("{title}."),
        scope,
        occurred_at,
        occurred_at,
    )
    .expect("notification fixture")
}

async fn owner_principal(app: &BootApplication, organization: &str) -> Result<PrincipalId> {
    let response = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/memberships"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(response.status(), 200);
    let value = response_json(&response)?["data"][0]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("owner Principal ID is missing".into()))?
        .to_owned();
    Ok(PrincipalId::from_uuid(Uuid::parse_str(&value).map_err(
        |error| BootError::Internal(format!("invalid owner Principal ID: {error}")),
    )?))
}

fn organization_id(value: &str) -> Result<OrganizationId> {
    Ok(OrganizationId::from_uuid(Uuid::parse_str(value).map_err(
        |error| BootError::Internal(format!("invalid Organization ID: {error}")),
    )?))
}
