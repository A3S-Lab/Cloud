use super::*;
use crate::modules::files::{
    UserFileAdmissionContract, UserFileAdmissionContractSpec, UserFileScanPolicy,
};
use crate::modules::shared_kernel::domain::{Sha256Digest, UserFileId, UserFileUploadId};
use chrono::TimeDelta;

const RESTRICTED_FILE_TOKEN: &str =
    "a3s_f777777777777777777777777777777777777777777777777777777777777777";

#[tokio::test]
async fn user_file_rest_and_mcp_share_one_replay_quota_and_lifecycle_authority() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let files = Arc::new(InMemoryUserFileRepository::default());
    let app = build_test_application_with_user_files(
        identity,
        projects,
        files.clone(),
        Arc::new(UnavailableUserFileObjectStore),
    )?;
    let organization = bootstrap_organization(&app, "files-bootstrap", "Files").await?;
    let project = create_project(&app, &organization, "files-project", "Knowledge").await?;
    let (admission_acl, user_file_id, size_bytes) = admission_acl(&organization, &project, 4_096)?;
    let collection = format!("/api/v1/organizations/{organization}/projects/{project}/user-files");

    let reserved = app
        .call(post_json(
            &collection,
            "files-cross-surface-reserve",
            json!({"admissionAcl": admission_acl.clone()}),
        ))
        .await?;
    assert_eq!(reserved.status(), 201);
    assert_no_store(&reserved);
    let reserved_body = response_json(&reserved)?;
    assert_eq!(
        reserved_body["data"]["file"]["userFileId"],
        user_file_id.to_string()
    );
    assert_eq!(reserved_body["data"]["file"]["state"], "awaiting_upload");
    assert_eq!(reserved_body["data"]["file"]["sizeBytes"], size_bytes);
    assert_eq!(reserved_body["data"]["replayed"], false);

    let mcp_replay = app
        .call(mcp_tool_call_as(
            1,
            "a3s_cloud_user_files_reserve",
            json!({
                "projectId": project,
                "admissionAcl": admission_acl,
                "idempotencyKey": "files-cross-surface-reserve"
            }),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(mcp_replay.status(), 200);
    let mcp_replay = response_json(&mcp_replay)?;
    assert_eq!(mcp_replay["result"]["structuredContent"]["code"], 200);
    assert_eq!(
        mcp_replay["result"]["structuredContent"]["data"]["file"]["userFileId"],
        user_file_id.to_string()
    );
    assert_eq!(
        mcp_replay["result"]["structuredContent"]["data"]["replayed"],
        true
    );

    let listed = app
        .call(get_as(format!("{collection}?limit=1"), ADMIN_TOKEN))
        .await?;
    assert_eq!(listed.status(), 200);
    assert_eq!(
        response_json(&listed)?["data"].as_array().map(Vec::len),
        Some(1)
    );

    let fetched = app
        .call(get_as(format!("{collection}/{user_file_id}"), ADMIN_TOKEN))
        .await?;
    assert_eq!(fetched.status(), 200);
    assert_eq!(
        response_json(&fetched)?["data"]["contractSchema"],
        "cloud.user-file.v1"
    );

    let mcp_list = app
        .call(mcp_tool_call_as(
            2,
            "a3s_cloud_user_files_list",
            json!({"projectId": project, "limit": 1}),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(
        response_json(&mcp_list)?["result"]["structuredContent"]["data"][0]["userFileId"],
        user_file_id.to_string()
    );

    let quota = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/user-file-quota"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(quota.status(), 200);
    assert_eq!(response_json(&quota)?["data"]["allocatedBytes"], size_bytes);

    let tombstoned = app
        .call(mcp_tool_call_as(
            3,
            "a3s_cloud_user_files_tombstone",
            json!({
                "projectId": project,
                "userFileId": user_file_id,
                "expectedVersion": 1,
                "idempotencyKey": "files-cross-surface-tombstone"
            }),
            ADMIN_TOKEN,
        ))
        .await?;
    let tombstoned = response_json(&tombstoned)?;
    assert_eq!(
        tombstoned["result"]["structuredContent"]["data"]["file"]["state"],
        "tombstoned"
    );
    assert_eq!(
        tombstoned["result"]["structuredContent"]["data"]["replayed"],
        false
    );

    let tombstone_replay = app
        .call(post_json(
            format!("{collection}/{user_file_id}/tombstone"),
            "files-cross-surface-tombstone",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(tombstone_replay.status(), 200);
    assert_eq!(response_json(&tombstone_replay)?["data"]["replayed"], true);

    let quota_after = app
        .call(mcp_tool_call_as(
            4,
            "a3s_cloud_user_file_quota_get",
            json!({}),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(
        response_json(&quota_after)?["result"]["structuredContent"]["data"]["allocatedBytes"],
        0
    );
    assert_eq!(files.event_count().await, 2);

    let imaginary_buffered_upload = app
        .call(post_json(
            format!("{collection}/{user_file_id}/upload"),
            "files-no-buffered-upload",
            json!({"bytes": "forbidden"}),
        ))
        .await?;
    assert_eq!(imaginary_buffered_upload.status(), 404);
    Ok(())
}

#[tokio::test]
async fn user_file_rest_rejects_invalid_bounds_before_allocating_quota() -> Result<()> {
    let files = Arc::new(InMemoryUserFileRepository::new(8_192).map_err(BootError::Internal)?);
    let app = build_test_application_with_user_files(
        Arc::new(InMemoryIdentityRepository::new()),
        Arc::new(InMemoryProjectsRepository::new()),
        files.clone(),
        Arc::new(UnavailableUserFileObjectStore),
    )?;
    let organization = bootstrap_organization(&app, "files-bounds", "Files bounds").await?;
    let project = create_project(&app, &organization, "files-bounds-project", "Files").await?;
    let collection = format!("/api/v1/organizations/{organization}/projects/{project}/user-files");

    let invalid_limit = app
        .call(get_as(format!("{collection}?limit=201"), ADMIN_TOKEN))
        .await?;
    assert_eq!(invalid_limit.status(), 400);

    let (oversized_acl, _, _) = admission_acl(&organization, &project, 8_193)?;
    let over_quota = app
        .call(post_json(
            &collection,
            "files-over-quota",
            json!({"admissionAcl": oversized_acl}),
        ))
        .await?;
    assert_eq!(over_quota.status(), 409);
    let quota = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/user-file-quota"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(response_json(&quota)?["data"]["allocatedBytes"], 0);
    assert_eq!(files.event_count().await, 0);
    Ok(())
}

#[tokio::test]
async fn restricted_user_files_authorize_before_replay_and_conceal_quota() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let files = Arc::new(InMemoryUserFileRepository::default());
    let app = build_test_application_with_user_files(
        identity,
        projects,
        files.clone(),
        Arc::new(UnavailableUserFileObjectStore),
    )?;
    let organization = bootstrap_organization(&app, "files-grants", "Files grants").await?;
    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "files-grants-membership",
            json!({"name": "Restricted Files operator", "role": "restricted"}),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let membership = response_json(&membership)?;
    let membership_id = membership["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted Files membership has no ID".into()))?;
    let principal_id = membership["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted Files principal has no ID".into()))?;
    let token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "files-grants-token",
            json!({
                "name": "Restricted Files operator",
                "token": RESTRICTED_FILE_TOKEN,
                "scopes": [ApiTokenScope::FILE_WRITE, ApiTokenScope::CLOUD_READ],
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(token.status(), 201);

    let granted_project =
        create_project(&app, &organization, "files-granted-project", "Granted").await?;
    let denied_project =
        create_project(&app, &organization, "files-denied-project", "Denied").await?;
    let resource_grant = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/memberships/{membership_id}/resource-grants"
            ),
            "files-grants-project",
            json!({"scope": {"kind": "project", "projectId": granted_project}}),
        ))
        .await?;
    assert_eq!(resource_grant.status(), 201);

    let granted_collection =
        format!("/api/v1/organizations/{organization}/projects/{granted_project}/user-files");
    let denied_collection =
        format!("/api/v1/organizations/{organization}/projects/{denied_project}/user-files");
    let (granted_acl, granted_file_id, _) = admission_acl(&organization, &granted_project, 1_024)?;
    let granted = app
        .call(post_json_as(
            &granted_collection,
            "files-grants-reserve-granted",
            json!({"admissionAcl": granted_acl}),
            RESTRICTED_FILE_TOKEN,
        ))
        .await?;
    assert_eq!(granted.status(), 201);
    assert_eq!(
        app.call(get_as(
            format!("{granted_collection}/{granted_file_id}"),
            RESTRICTED_FILE_TOKEN,
        ))
        .await?
        .status(),
        200
    );

    let (denied_acl, denied_file_id, _) = admission_acl(&organization, &denied_project, 1_024)?;
    let denied_reservation = app
        .call(post_json(
            &denied_collection,
            "files-grants-reserve-denied",
            json!({"admissionAcl": denied_acl.clone()}),
        ))
        .await?;
    assert_eq!(denied_reservation.status(), 201);
    let duplicate_identity = app
        .call(post_json(
            &denied_collection,
            "files-duplicate-cross-project-identity",
            json!({
                "admissionAcl": admission_acl_with_user_file_id(
                    &organization,
                    &denied_project,
                    granted_file_id,
                    512,
                )?
            }),
        ))
        .await?;
    assert_eq!(duplicate_identity.status(), 409);
    let denied_replay = app
        .call(post_json_as(
            &denied_collection,
            "files-grants-reserve-denied",
            json!({"admissionAcl": denied_acl.clone()}),
            RESTRICTED_FILE_TOKEN,
        ))
        .await?;
    assert_eq!(denied_replay.status(), 403);
    let denied_read = app
        .call(get_as(
            format!("{denied_collection}/{denied_file_id}"),
            RESTRICTED_FILE_TOKEN,
        ))
        .await?;
    assert_eq!(denied_read.status(), 403);

    let concealed_replay = app
        .call(mcp_tool_call_as(
            50,
            "a3s_cloud_user_files_reserve",
            json!({
                "projectId": denied_project,
                "admissionAcl": denied_acl,
                "idempotencyKey": "files-grants-reserve-denied"
            }),
            RESTRICTED_FILE_TOKEN,
        ))
        .await?;
    assert_eq!(
        response_json(&concealed_replay)?["result"]["structuredContent"]["code"],
        404
    );
    assert_eq!(
        app.call(get_as(
            format!("/api/v1/organizations/{organization}/user-file-quota"),
            RESTRICTED_FILE_TOKEN,
        ))
        .await?
        .status(),
        404
    );
    let concealed_quota = app
        .call(mcp_tool_call_as(
            51,
            "a3s_cloud_user_file_quota_get",
            json!({}),
            RESTRICTED_FILE_TOKEN,
        ))
        .await?;
    let concealed_quota = response_json(&concealed_quota)?;
    assert_eq!(concealed_quota["error"]["code"], -32602);
    assert_eq!(
        concealed_quota["error"]["message"],
        "Unknown or unavailable tool"
    );
    assert_eq!(files.event_count().await, 2);
    Ok(())
}

fn admission_acl(
    organization: &str,
    project: &str,
    size_bytes: u64,
) -> Result<(String, UserFileId, u64)> {
    let (organization_id, project_id) = user_file_scope(organization, project)?;
    let user_file_id = UserFileId::new();
    Ok((
        admission_acl_document(organization_id, project_id, user_file_id, size_bytes)?,
        user_file_id,
        size_bytes,
    ))
}

fn admission_acl_with_user_file_id(
    organization: &str,
    project: &str,
    user_file_id: UserFileId,
    size_bytes: u64,
) -> Result<String> {
    let (organization_id, project_id) = user_file_scope(organization, project)?;
    admission_acl_document(organization_id, project_id, user_file_id, size_bytes)
}

fn user_file_scope(organization: &str, project: &str) -> Result<(OrganizationId, ProjectId)> {
    let organization_id = OrganizationId::from_uuid(
        Uuid::parse_str(organization)
            .map_err(|error| BootError::Internal(format!("invalid organization ID: {error}")))?,
    );
    let project_id = ProjectId::from_uuid(
        Uuid::parse_str(project)
            .map_err(|error| BootError::Internal(format!("invalid project ID: {error}")))?,
    );
    Ok((organization_id, project_id))
}

fn admission_acl_document(
    organization_id: OrganizationId,
    project_id: ProjectId,
    user_file_id: UserFileId,
    size_bytes: u64,
) -> Result<String> {
    let now = Utc::now();
    let contract = UserFileAdmissionContract::from_spec(UserFileAdmissionContractSpec {
        original_name: "knowledge.txt".into(),
        upload_expires_at: now + TimeDelta::hours(1),
        retention_until: now + TimeDelta::days(30),
        scan_policy: UserFileScanPolicy::Required,
        content: UserFileContentReference::new(
            organization_id,
            project_id,
            user_file_id,
            UserFileUploadId::new(),
            Sha256Digest::from_bytes(format!("files:{user_file_id}:{size_bytes}").as_bytes()),
            size_bytes,
            "text/plain",
        )
        .map_err(BootError::Internal)?,
    })
    .map_err(BootError::Internal)?;
    Ok(contract.canonical_acl().to_owned())
}
