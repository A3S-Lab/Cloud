use super::*;

const KNOWLEDGE_INDEX_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/k0.1/knowledge-index-revision.acl"
));
const KNOWLEDGE_POLICY_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/k0.1/knowledge-retrieval-policy-revision.acl"
));
const EXTERNAL_BINDING_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/k0.1/external-knowledge-binding.acl"
));
const FIXTURE_ORGANIZATION_ID: &str = "018f0000-0000-7000-8000-000000000201";
const FIXTURE_PROJECT_ID: &str = "018f0000-0000-7000-8000-000000000202";
const FIXTURE_INDEX_REVISION_ID: &str = "018f0000-0000-7000-8000-000000000305";
const FIXTURE_POLICY_REVISION_ID: &str = "018f0000-0000-7000-8000-000000000306";
const FIXTURE_BINDING_ID: &str = "018f0000-0000-7000-8000-000000000307";
const RESTRICTED_KNOWLEDGE_TOKEN: &str =
    "a3s_c888888888888888888888888888888888888888888888888888888888888888";

#[tokio::test]
async fn knowledge_index_policy_binding_rest_create_get_list_replay_and_deny_unauthorized_project(
) -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization =
        bootstrap_organization(&app, "knowledge-index-rest", "Knowledge Index REST").await?;

    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "knowledge-index-rest-membership",
            json!({"name": "Restricted Knowledge index operator", "role": "restricted"}),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let membership = response_json(&membership)?;
    let membership_id = membership["data"]["id"].as_str().ok_or_else(|| {
        BootError::Internal("restricted Knowledge index membership has no ID".into())
    })?;
    let principal_id = membership["data"]["principalId"].as_str().ok_or_else(|| {
        BootError::Internal("restricted Knowledge index principal has no ID".into())
    })?;
    let token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "knowledge-index-rest-token",
            json!({
                "name": "Restricted Knowledge index operator",
                "token": RESTRICTED_KNOWLEDGE_TOKEN,
                "scopes": [ApiTokenScope::KNOWLEDGE_WRITE, ApiTokenScope::CLOUD_READ],
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(token.status(), 201);

    let granted_project = create_project(
        &app,
        &organization,
        "knowledge-index-granted-project",
        "Granted",
    )
    .await?;
    let denied_project = create_project(
        &app,
        &organization,
        "knowledge-index-denied-project",
        "Denied",
    )
    .await?;
    let resource_grant = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/memberships/{membership_id}/resource-grants"
            ),
            "knowledge-index-rest-grant",
            json!({"scope": {"kind": "project", "projectId": granted_project}}),
        ))
        .await?;
    assert_eq!(resource_grant.status(), 201);

    let granted_indexes = format!(
        "/api/v1/organizations/{organization}/projects/{granted_project}/knowledge-index-revisions"
    );
    let denied_indexes = format!(
        "/api/v1/organizations/{organization}/projects/{denied_project}/knowledge-index-revisions"
    );
    let index_acl = rewrite_tenant_ids(KNOWLEDGE_INDEX_ACL, &organization, &granted_project);

    let created = app
        .call(post_json_as(
            &granted_indexes,
            "knowledge-index-rest-create",
            json!({"indexAcl": index_acl}),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(created.status(), 201);
    assert_no_store(&created);
    let created = response_json(&created)?;
    assert_eq!(created["data"]["replayed"], false);
    assert_eq!(
        created["data"]["knowledgeIndexRevision"]["indexRevisionId"],
        FIXTURE_INDEX_REVISION_ID
    );
    assert_eq!(
        created["data"]["knowledgeIndexRevision"]["contractSchema"],
        "cloud.knowledge-index-revision.v1"
    );
    assert_eq!(
        created["data"]["knowledgeIndexRevision"]["strategy"],
        "hybrid"
    );

    let replayed = app
        .call(post_json_as(
            &granted_indexes,
            "knowledge-index-rest-create",
            json!({"indexAcl": index_acl}),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(replayed.status(), 200);
    let replayed = response_json(&replayed)?;
    assert_eq!(replayed["data"]["replayed"], true);
    assert_eq!(
        replayed["data"]["knowledgeIndexRevision"]["indexRevisionId"],
        FIXTURE_INDEX_REVISION_ID
    );

    let fetched = app
        .call(get_as(
            format!("{granted_indexes}/{FIXTURE_INDEX_REVISION_ID}"),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(fetched.status(), 200);
    let fetched = response_json(&fetched)?;
    assert_eq!(
        fetched["data"]["indexRevisionId"],
        FIXTURE_INDEX_REVISION_ID
    );
    assert_eq!(fetched["data"]["projectId"], granted_project);
    assert_eq!(fetched["data"]["strategy"], "hybrid");

    let granted_policies = format!(
        "/api/v1/organizations/{organization}/projects/{granted_project}/knowledge-retrieval-policy-revisions"
    );
    let policy_acl = rewrite_tenant_ids(KNOWLEDGE_POLICY_ACL, &organization, &granted_project);
    let created_policy = app
        .call(post_json_as(
            &granted_policies,
            "knowledge-policy-rest-create",
            json!({"policyAcl": policy_acl}),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(created_policy.status(), 201);
    let created_policy = response_json(&created_policy)?;
    assert_eq!(created_policy["data"]["replayed"], false);
    assert_eq!(
        created_policy["data"]["knowledgeRetrievalPolicyRevision"]["policyRevisionId"],
        FIXTURE_POLICY_REVISION_ID
    );
    assert_eq!(
        created_policy["data"]["knowledgeRetrievalPolicyRevision"]["searchMode"],
        "hybrid"
    );
    assert_eq!(
        created_policy["data"]["knowledgeRetrievalPolicyRevision"]["topK"],
        8
    );

    let replayed_policy = app
        .call(post_json_as(
            &granted_policies,
            "knowledge-policy-rest-create",
            json!({"policyAcl": policy_acl}),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(replayed_policy.status(), 200);
    let replayed_policy = response_json(&replayed_policy)?;
    assert_eq!(replayed_policy["data"]["replayed"], true);

    let fetched_policy = app
        .call(get_as(
            format!("{granted_policies}/{FIXTURE_POLICY_REVISION_ID}"),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(fetched_policy.status(), 200);
    let fetched_policy = response_json(&fetched_policy)?;
    assert_eq!(
        fetched_policy["data"]["policyRevisionId"],
        FIXTURE_POLICY_REVISION_ID
    );
    assert_eq!(fetched_policy["data"]["projectId"], granted_project);

    let granted_bindings = format!(
        "/api/v1/organizations/{organization}/projects/{granted_project}/external-knowledge-bindings"
    );
    let binding_acl = rewrite_tenant_ids(EXTERNAL_BINDING_ACL, &organization, &granted_project);
    let created_binding = app
        .call(post_json_as(
            &granted_bindings,
            "knowledge-binding-rest-create",
            json!({"bindingAcl": binding_acl}),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(created_binding.status(), 201);
    let created_binding = response_json(&created_binding)?;
    assert_eq!(created_binding["data"]["replayed"], false);
    assert_eq!(
        created_binding["data"]["externalKnowledgeBinding"]["bindingId"],
        FIXTURE_BINDING_ID
    );
    assert_eq!(
        created_binding["data"]["externalKnowledgeBinding"]["displayName"],
        "Partner corpus"
    );

    let replayed_binding = app
        .call(post_json_as(
            &granted_bindings,
            "knowledge-binding-rest-create",
            json!({"bindingAcl": binding_acl}),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(replayed_binding.status(), 200);
    let replayed_binding = response_json(&replayed_binding)?;
    assert_eq!(replayed_binding["data"]["replayed"], true);

    let fetched_binding = app
        .call(get_as(
            format!("{granted_bindings}/{FIXTURE_BINDING_ID}"),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(fetched_binding.status(), 200);
    let fetched_binding = response_json(&fetched_binding)?;
    assert_eq!(fetched_binding["data"]["bindingId"], FIXTURE_BINDING_ID);
    assert_eq!(fetched_binding["data"]["projectId"], granted_project);

    let knowledge_base_revision_id = created["data"]["knowledgeIndexRevision"]
        ["knowledgeBaseRevisionId"]
        .as_str()
        .ok_or_else(|| {
            BootError::Internal("KnowledgeIndexRevision has no knowledgeBaseRevisionId".into())
        })?;
    let listed_indexes = app
        .call(get_as(
            format!(
                "{granted_indexes}?knowledgeBaseRevisionId={knowledge_base_revision_id}&limit=8"
            ),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(listed_indexes.status(), 200);
    let listed_indexes = response_json(&listed_indexes)?;
    assert_eq!(
        listed_indexes["data"].as_array().map(|items| items.len()),
        Some(1)
    );

    let listed_policies = app
        .call(get_as(
            format!(
                "{granted_policies}?knowledgeBaseRevisionId={knowledge_base_revision_id}&limit=8"
            ),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(listed_policies.status(), 200);
    let listed_policies = response_json(&listed_policies)?;
    assert_eq!(
        listed_policies["data"].as_array().map(|items| items.len()),
        Some(1)
    );

    let knowledge_base_id = created_binding["data"]["externalKnowledgeBinding"]["knowledgeBaseId"]
        .as_str()
        .ok_or_else(|| {
            BootError::Internal("ExternalKnowledgeBinding has no knowledgeBaseId".into())
        })?;
    let listed_bindings = app
        .call(get_as(
            format!("{granted_bindings}?knowledgeBaseId={knowledge_base_id}&limit=8"),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(listed_bindings.status(), 200);
    let listed_bindings = response_json(&listed_bindings)?;
    assert_eq!(
        listed_bindings["data"].as_array().map(|items| items.len()),
        Some(1)
    );

    let missing_revision = app
        .call(get_as(&granted_indexes, RESTRICTED_KNOWLEDGE_TOKEN))
        .await?;
    assert_eq!(missing_revision.status(), 400);

    let denied_acl = rewrite_tenant_ids(KNOWLEDGE_INDEX_ACL, &organization, &denied_project);
    let denied_create = app
        .call(post_json_as(
            &denied_indexes,
            "knowledge-index-rest-create-denied",
            json!({"indexAcl": denied_acl}),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(denied_create.status(), 403);

    let denied_get = app
        .call(get_as(
            format!("{denied_indexes}/{FIXTURE_INDEX_REVISION_ID}"),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(denied_get.status(), 403);

    let denied_policy_get = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{denied_project}/knowledge-retrieval-policy-revisions/{FIXTURE_POLICY_REVISION_ID}"
            ),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(denied_policy_get.status(), 403);

    let denied_binding_get = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{denied_project}/external-knowledge-bindings/{FIXTURE_BINDING_ID}"
            ),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(denied_binding_get.status(), 403);
    Ok(())
}

fn rewrite_tenant_ids(fixture: &str, organization: &str, project: &str) -> String {
    fixture
        .replace(FIXTURE_ORGANIZATION_ID, organization)
        .replace(FIXTURE_PROJECT_ID, project)
}
