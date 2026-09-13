use super::*;

const KNOWLEDGE_DOCUMENT_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/k0.1/knowledge-document.acl"
));
const KNOWLEDGE_CHUNK_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/k0.1/knowledge-chunk.acl"
));
const FIXTURE_ORGANIZATION_ID: &str = "018f0000-0000-7000-8000-000000000201";
const FIXTURE_PROJECT_ID: &str = "018f0000-0000-7000-8000-000000000202";
const FIXTURE_DOCUMENT_ID: &str = "018f0000-0000-7000-8000-000000000303";
const FIXTURE_CHUNK_ID: &str = "018f0000-0000-7000-8000-000000000304";
const RESTRICTED_KNOWLEDGE_TOKEN: &str =
    "a3s_c888888888888888888888888888888888888888888888888888888888888888";

#[tokio::test]
async fn knowledge_document_and_chunk_rest_create_get_replay_and_deny_unauthorized_project(
) -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization =
        bootstrap_organization(&app, "knowledge-document-rest", "Knowledge Document REST").await?;

    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "knowledge-document-rest-membership",
            json!({"name": "Restricted Knowledge document operator", "role": "restricted"}),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let membership = response_json(&membership)?;
    let membership_id = membership["data"]["id"].as_str().ok_or_else(|| {
        BootError::Internal("restricted Knowledge document membership has no ID".into())
    })?;
    let principal_id = membership["data"]["principalId"].as_str().ok_or_else(|| {
        BootError::Internal("restricted Knowledge document principal has no ID".into())
    })?;
    let token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "knowledge-document-rest-token",
            json!({
                "name": "Restricted Knowledge document operator",
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
        "knowledge-document-granted-project",
        "Granted",
    )
    .await?;
    let denied_project = create_project(
        &app,
        &organization,
        "knowledge-document-denied-project",
        "Denied",
    )
    .await?;
    let resource_grant = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/memberships/{membership_id}/resource-grants"
            ),
            "knowledge-document-rest-grant",
            json!({"scope": {"kind": "project", "projectId": granted_project}}),
        ))
        .await?;
    assert_eq!(resource_grant.status(), 201);

    let granted_documents = format!(
        "/api/v1/organizations/{organization}/projects/{granted_project}/knowledge-documents"
    );
    let denied_documents = format!(
        "/api/v1/organizations/{organization}/projects/{denied_project}/knowledge-documents"
    );
    let document_acl = knowledge_document_acl(&organization, &granted_project);

    let created = app
        .call(post_json_as(
            &granted_documents,
            "knowledge-document-rest-create",
            json!({"documentAcl": document_acl}),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(created.status(), 201);
    assert_no_store(&created);
    let created = response_json(&created)?;
    assert_eq!(created["data"]["replayed"], false);
    assert_eq!(
        created["data"]["knowledgeDocument"]["documentId"],
        FIXTURE_DOCUMENT_ID
    );
    assert_eq!(
        created["data"]["knowledgeDocument"]["contractSchema"],
        "cloud.knowledge-document.v1"
    );
    assert_eq!(
        created["data"]["knowledgeDocument"]["title"],
        "Return policy"
    );

    let replayed = app
        .call(post_json_as(
            &granted_documents,
            "knowledge-document-rest-create",
            json!({"documentAcl": document_acl}),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(replayed.status(), 200);
    let replayed = response_json(&replayed)?;
    assert_eq!(replayed["data"]["replayed"], true);
    assert_eq!(
        replayed["data"]["knowledgeDocument"]["documentId"],
        FIXTURE_DOCUMENT_ID
    );

    let fetched = app
        .call(get_as(
            format!("{granted_documents}/{FIXTURE_DOCUMENT_ID}"),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(fetched.status(), 200);
    let fetched = response_json(&fetched)?;
    assert_eq!(fetched["data"]["documentId"], FIXTURE_DOCUMENT_ID);
    assert_eq!(fetched["data"]["projectId"], granted_project);
    assert_eq!(fetched["data"]["title"], "Return policy");

    let granted_chunks = format!("{granted_documents}/{FIXTURE_DOCUMENT_ID}/chunks");
    let chunk_acl = knowledge_chunk_acl(&organization, &granted_project);
    let created_chunk = app
        .call(post_json_as(
            &granted_chunks,
            "knowledge-chunk-rest-create",
            json!({"chunkAcl": chunk_acl}),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(created_chunk.status(), 201);
    let created_chunk = response_json(&created_chunk)?;
    assert_eq!(created_chunk["data"]["replayed"], false);
    assert_eq!(
        created_chunk["data"]["knowledgeChunk"]["chunkId"],
        FIXTURE_CHUNK_ID
    );
    assert_eq!(
        created_chunk["data"]["knowledgeChunk"]["contractSchema"],
        "cloud.knowledge-chunk.v1"
    );
    assert_eq!(created_chunk["data"]["knowledgeChunk"]["ordinal"], 0);

    let replayed_chunk = app
        .call(post_json_as(
            &granted_chunks,
            "knowledge-chunk-rest-create",
            json!({"chunkAcl": chunk_acl}),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(replayed_chunk.status(), 200);
    let replayed_chunk = response_json(&replayed_chunk)?;
    assert_eq!(replayed_chunk["data"]["replayed"], true);
    assert_eq!(
        replayed_chunk["data"]["knowledgeChunk"]["chunkId"],
        FIXTURE_CHUNK_ID
    );

    let fetched_chunk = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{granted_project}/knowledge-chunks/{FIXTURE_CHUNK_ID}"
            ),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(fetched_chunk.status(), 200);
    let fetched_chunk = response_json(&fetched_chunk)?;
    assert_eq!(fetched_chunk["data"]["chunkId"], FIXTURE_CHUNK_ID);
    assert_eq!(fetched_chunk["data"]["documentId"], FIXTURE_DOCUMENT_ID);
    assert_eq!(fetched_chunk["data"]["projectId"], granted_project);

    let denied_acl = knowledge_document_acl(&organization, &denied_project);
    let denied_create = app
        .call(post_json_as(
            &denied_documents,
            "knowledge-document-rest-create-denied",
            json!({"documentAcl": denied_acl}),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(denied_create.status(), 403);

    let denied_get = app
        .call(get_as(
            format!("{denied_documents}/{FIXTURE_DOCUMENT_ID}"),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(denied_get.status(), 403);

    let denied_chunk_get = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{denied_project}/knowledge-chunks/{FIXTURE_CHUNK_ID}"
            ),
            RESTRICTED_KNOWLEDGE_TOKEN,
        ))
        .await?;
    assert_eq!(denied_chunk_get.status(), 403);
    Ok(())
}

fn knowledge_document_acl(organization: &str, project: &str) -> String {
    KNOWLEDGE_DOCUMENT_ACL
        .replace(FIXTURE_ORGANIZATION_ID, organization)
        .replace(FIXTURE_PROJECT_ID, project)
}

fn knowledge_chunk_acl(organization: &str, project: &str) -> String {
    KNOWLEDGE_CHUNK_ACL
        .replace(FIXTURE_ORGANIZATION_ID, organization)
        .replace(FIXTURE_PROJECT_ID, project)
}
