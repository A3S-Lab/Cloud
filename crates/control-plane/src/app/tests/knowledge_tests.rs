use super::*;

const KNOWLEDGE_BASE_REVISION_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/k0.1/knowledge-base-revision.acl"
));
const FIXTURE_ORGANIZATION_ID: &str = "018f0000-0000-7000-8000-000000000201";
const FIXTURE_PROJECT_ID: &str = "018f0000-0000-7000-8000-000000000202";
const FIXTURE_KNOWLEDGE_BASE_ID: &str = "018f0000-0000-7000-8000-000000000301";
const RESTRICTED_KNOWLEDGE_TOKEN: &str =
    "a3s_c777777777777777777777777777777777777777777777777777777777777777";

#[tokio::test]
async fn knowledge_base_rest_create_get_list_and_deny_unauthorized_project() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "knowledge-rest", "Knowledge REST").await?;

    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "knowledge-rest-membership",
            json!({"name": "Restricted Knowledge operator", "role": "restricted"}),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let membership = response_json(&membership)?;
    let membership_id = membership["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted Knowledge membership has no ID".into()))?;
    let principal_id = membership["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted Knowledge principal has no ID".into()))?;
    let token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "knowledge-rest-token",
            json!({
                "name": "Restricted Knowledge operator",
                "token": RESTRICTED_KNOWLEDGE_TOKEN,
                "scopes": [ApiTokenScope::KNOWLEDGE_WRITE, ApiTokenScope::CLOUD_READ],
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(token.status(), 201);

    let granted_project =
        create_project(&app, &organization, "knowledge-granted-project", "Granted").await?;
    let denied_project =
        create_project(&app, &organization, "knowledge-denied-project", "Denied").await?;
    let resource_grant = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/memberships/{membership_id}/resource-grants"
            ),
            "knowledge-rest-grant",
            json!({"scope": {"kind": "project", "projectId": granted_project}}),
        ))
        .await?;
    assert_eq!(resource_grant.status(), 201);

    let granted_collection =
        format!("/api/v1/organizations/{organization}/projects/{granted_project}/knowledge-bases");
    let denied_collection =
        format!("/api/v1/organizations/{organization}/projects/{denied_project}/knowledge-bases");
    let revision_acl = knowledge_base_revision_acl(&organization, &granted_project);

    let created = app
        .call(post_json_as(
            &granted_collection,
            "knowledge-rest-create",
            json!({"revisionAcl": revision_acl}),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(created.status(), 201);
    assert_no_store(&created);
    let created = response_json(&created)?;
    assert_eq!(created["data"]["replayed"], false);
    assert_eq!(
        created["data"]["knowledgeBase"]["knowledgeBaseId"],
        FIXTURE_KNOWLEDGE_BASE_ID
    );
    assert_eq!(
        created["data"]["knowledgeBase"]["contractSchema"],
        "cloud.knowledge-base-revision.v1"
    );
    assert_eq!(created["data"]["knowledgeBase"]["name"], "Product FAQ");

    let fetched = app
        .call(get_as(
            format!("{granted_collection}/{FIXTURE_KNOWLEDGE_BASE_ID}"),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(fetched.status(), 200);
    let fetched = response_json(&fetched)?;
    assert_eq!(
        fetched["data"]["knowledgeBaseId"],
        FIXTURE_KNOWLEDGE_BASE_ID
    );
    assert_eq!(fetched["data"]["projectId"], granted_project);
    assert_eq!(fetched["data"]["generation"], 1);

    let listed = app
        .call(get_as(
            format!("{granted_collection}?limit=10"),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(listed.status(), 200);
    let listed = response_json(&listed)?;
    assert_eq!(listed["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        listed["data"][0]["knowledgeBaseId"],
        FIXTURE_KNOWLEDGE_BASE_ID
    );

    let denied_acl = knowledge_base_revision_acl(&organization, &denied_project);
    let denied_create = app
        .call(post_json_as(
            &denied_collection,
            "knowledge-rest-create-denied",
            json!({"revisionAcl": denied_acl}),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(denied_create.status(), 403);

    let denied_list = app
        .call(get_as(
            format!("{denied_collection}?limit=10"),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(denied_list.status(), 403);

    let denied_get = app
        .call(get_as(
            format!("{denied_collection}/{FIXTURE_KNOWLEDGE_BASE_ID}"),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(denied_get.status(), 403);
    Ok(())
}

fn knowledge_base_revision_acl(organization: &str, project: &str) -> String {
    KNOWLEDGE_BASE_REVISION_ACL
        .replace(FIXTURE_ORGANIZATION_ID, organization)
        .replace(FIXTURE_PROJECT_ID, project)
}
