use super::*;

const ONTOLOGY_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/w0.1/ontology.acl"
));
const RESTRICTED_ONTOLOGY_TOKEN: &str =
    "a3s_7777777777777777777777777777777777777777777777777777777777777777";

#[tokio::test]
async fn ontology_lifecycle_is_versioned_idempotent_and_diffable() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization_id = bootstrap_organization(&app, "ontology-bootstrap", "Acme").await?;
    let project_id =
        create_project(&app, &organization_id, "ontology-project", "Knowledge").await?;
    let collection =
        format!("/api/v1/organizations/{organization_id}/projects/{project_id}/ontologies");

    let create = || {
        post_acl(
            &collection,
            "ontology-create",
            ONTOLOGY_ACL.as_bytes().to_vec(),
        )
    };
    let first = app.call(create()).await?;
    let replay = app.call(create()).await?;
    assert_eq!(first.status(), 201);
    assert_eq!(replay.status(), 200);
    let first_body = response_json(&first)?;
    let replay_body = response_json(&replay)?;
    assert_eq!(
        first_body["data"]["ontology"]["id"],
        replay_body["data"]["ontology"]["id"]
    );
    assert_eq!(
        first_body["data"]["revision"]["id"],
        replay_body["data"]["revision"]["id"]
    );
    assert_eq!(replay_body["data"]["replayed"], true);

    let ontology_id = first_body["data"]["ontology"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("Ontology ID is missing".into()))?;
    let first_revision_id = first_body["data"]["revision"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("Ontology revision ID is missing".into()))?;
    let root = format!("/api/v1/organizations/{organization_id}/ontologies/{ontology_id}");

    let listed = app.call(get_as(&collection, ADMIN_TOKEN)).await?;
    assert_eq!(listed.status(), 200);
    assert_eq!(
        response_json(&listed)?["data"].as_array().map(Vec::len),
        Some(1)
    );
    let fetched = app.call(get_as(&root, ADMIN_TOKEN)).await?;
    assert_eq!(fetched.status(), 200);
    assert_eq!(response_json(&fetched)?["data"]["currentRevisionNumber"], 1);

    let compatible_acl = ONTOLOGY_ACL.replace(
        "Deterministic W0.1 Ontology contract fixture",
        "Compatible description revision",
    );
    let compatible = app
        .call(
            post_acl(
                format!("{root}/revisions"),
                "ontology-compatible",
                compatible_acl.as_bytes().to_vec(),
            )
            .with_header("x-a3s-expected-version", "1"),
        )
        .await?;
    assert_eq!(compatible.status(), 201);
    let compatible_body = response_json(&compatible)?;
    assert_eq!(compatible_body["data"]["revision"]["revisionNumber"], 2);
    assert_eq!(
        compatible_body["data"]["revision"]["migrationPolicy"]["kind"],
        "compatible"
    );
    assert_eq!(compatible_body["data"]["diff"]["breaking"], false);
    let second_revision_id = compatible_body["data"]["revision"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("second Ontology revision ID is missing".into()))?;

    let revisions = app
        .call(get_as(format!("{root}/revisions"), ADMIN_TOKEN))
        .await?;
    assert_eq!(revisions.status(), 200);
    let revisions_body = response_json(&revisions)?;
    let revision_numbers = revisions_body["data"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value["revisionNumber"].as_u64())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert_eq!(revision_numbers, vec![2, 1]);
    let revision = app
        .call(get_as(
            format!("{root}/revisions/{second_revision_id}"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(revision.status(), 200);
    assert!(response_json(&revision)?["data"]["canonicalAcl"]
        .as_str()
        .is_some_and(|acl| acl.contains("Compatible description revision")));

    let diff = app
        .call(get_as(
            format!("{root}/revisions/{first_revision_id}/diff/{second_revision_id}"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(diff.status(), 200);
    let diff_body = response_json(&diff)?;
    assert_eq!(diff_body["data"]["breaking"], false);
    assert_eq!(diff_body["data"]["changes"][0]["resourceKind"], "metadata");

    let breaking_acl = breaking_acl(&compatible_acl);
    let rejected = app
        .call(
            post_acl(
                format!("{root}/revisions"),
                "ontology-breaking-without-rule",
                breaking_acl.as_bytes().to_vec(),
            )
            .with_header("x-a3s-expected-version", "2"),
        )
        .await?;
    assert_eq!(rejected.status(), 422);

    let migrated = app
        .call(
            post_acl(
                format!("{root}/revisions"),
                "ontology-breaking-with-rule",
                breaking_acl.as_bytes().to_vec(),
            )
            .with_header("x-a3s-expected-version", "2")
            .with_header("x-a3s-migration-rule", "migrate_ticket_v2"),
        )
        .await?;
    assert_eq!(migrated.status(), 201);
    let migrated_body = response_json(&migrated)?;
    assert_eq!(migrated_body["data"]["revision"]["revisionNumber"], 3);
    assert_eq!(
        migrated_body["data"]["revision"]["migrationPolicy"]["kind"],
        "explicit"
    );
    assert_eq!(migrated_body["data"]["diff"]["breaking"], true);

    let initial_replay = app.call(create()).await?;
    assert_eq!(initial_replay.status(), 200);
    let initial_replay = response_json(&initial_replay)?;
    assert_eq!(
        initial_replay["data"]["ontology"]["currentRevisionNumber"],
        1
    );
    assert_eq!(initial_replay["data"]["revision"]["revisionNumber"], 1);

    let compatible_replay = app
        .call(
            post_acl(
                format!("{root}/revisions"),
                "ontology-compatible",
                compatible_acl.as_bytes().to_vec(),
            )
            .with_header("x-a3s-expected-version", "1"),
        )
        .await?;
    assert_eq!(compatible_replay.status(), 200);
    let compatible_replay = response_json(&compatible_replay)?;
    assert_eq!(
        compatible_replay["data"]["ontology"]["currentRevisionNumber"],
        2
    );
    assert_eq!(compatible_replay["data"]["revision"]["revisionNumber"], 2);
    assert_eq!(compatible_replay["data"]["replayed"], true);
    Ok(())
}

#[tokio::test]
async fn restricted_ontology_access_resolves_project_before_reads_revisions_and_replay(
) -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "ontology-grants", "Ontology grants").await?;

    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "ontology-grants-membership",
            json!({"name": "Restricted Ontology author", "role": "restricted"}),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let membership = response_json(&membership)?;
    let membership_id = membership["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted Ontology membership has no ID".into()))?;
    let principal_id = membership["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted Ontology principal has no ID".into()))?;
    let token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "ontology-grants-token",
            json!({
                "name": "Restricted Ontology author",
                "token": RESTRICTED_ONTOLOGY_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ, ApiTokenScope::ONTOLOGY_WRITE],
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(token.status(), 201);

    let granted_project =
        create_project(&app, &organization, "ontology-granted-project", "Granted").await?;
    let environment_only_project = create_project(
        &app,
        &organization,
        "ontology-environment-project",
        "Environment only",
    )
    .await?;
    let environment = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/projects/{environment_only_project}/environments"
            ),
            "ontology-environment",
            json!({"name": "Environment only"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment_id = response_id(&environment)?;

    let granted_collection =
        format!("/api/v1/organizations/{organization}/projects/{granted_project}/ontologies");
    let environment_collection = format!(
        "/api/v1/organizations/{organization}/projects/{environment_only_project}/ontologies"
    );
    let granted = app
        .call(post_acl(
            &granted_collection,
            "ontology-grants-create-granted",
            ONTOLOGY_ACL.as_bytes().to_vec(),
        ))
        .await?;
    assert_eq!(granted.status(), 201);
    let granted = response_json(&granted)?;
    let granted_ontology_id = granted["data"]["ontology"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("granted Ontology has no ID".into()))?
        .to_owned();
    let granted_revision_id = granted["data"]["revision"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("granted Ontology revision has no ID".into()))?
        .to_owned();

    let denied = app
        .call(post_acl(
            &environment_collection,
            "ontology-grants-create-environment",
            ONTOLOGY_ACL.as_bytes().to_vec(),
        ))
        .await?;
    assert_eq!(denied.status(), 201);
    let denied = response_json(&denied)?;
    let denied_ontology_id = denied["data"]["ontology"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("denied Ontology has no ID".into()))?
        .to_owned();
    let denied_revision_id = denied["data"]["revision"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("denied Ontology revision has no ID".into()))?
        .to_owned();

    let resource_grants =
        format!("/api/v1/organizations/{organization}/memberships/{membership_id}/resource-grants");
    let project_grant = app
        .call(post_json(
            &resource_grants,
            "ontology-grants-create-project",
            json!({"scope": {"kind": "project", "projectId": granted_project}}),
        ))
        .await?;
    assert_eq!(project_grant.status(), 201);
    let project_grant_id = response_json(&project_grant)?["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("Ontology Resource Grant has no ID".into()))?
        .to_owned();
    let environment_grant = app
        .call(post_json(
            &resource_grants,
            "ontology-grants-create-environment",
            json!({
                "scope": {
                    "kind": "environment",
                    "projectId": environment_only_project,
                    "environmentId": environment_id
                }
            }),
        ))
        .await?;
    assert_eq!(environment_grant.status(), 201);

    let granted_root =
        format!("/api/v1/organizations/{organization}/ontologies/{granted_ontology_id}");
    let denied_root =
        format!("/api/v1/organizations/{organization}/ontologies/{denied_ontology_id}");
    let missing_root = format!(
        "/api/v1/organizations/{organization}/ontologies/{}",
        Uuid::now_v7()
    );
    assert_eq!(
        app.call(get_as(&granted_collection, RESTRICTED_ONTOLOGY_TOKEN))
            .await?
            .status(),
        200
    );
    assert_eq!(
        app.call(get_as(&environment_collection, RESTRICTED_ONTOLOGY_TOKEN,))
            .await?
            .status(),
        403
    );
    for path in [
        granted_root.clone(),
        format!("{granted_root}/revisions"),
        format!("{granted_root}/revisions/{granted_revision_id}"),
    ] {
        assert_eq!(
            app.call(get_as(path, RESTRICTED_ONTOLOGY_TOKEN))
                .await?
                .status(),
            200
        );
    }
    for (denied_path, missing_path) in [
        (denied_root.clone(), missing_root.clone()),
        (
            format!("{denied_root}/revisions"),
            format!("{missing_root}/revisions"),
        ),
        (
            format!("{denied_root}/revisions/{denied_revision_id}"),
            format!("{missing_root}/revisions/{}", Uuid::now_v7()),
        ),
        (
            format!("{denied_root}/revisions/{denied_revision_id}/diff/{denied_revision_id}"),
            format!(
                "{missing_root}/revisions/{}/diff/{}",
                Uuid::now_v7(),
                Uuid::now_v7()
            ),
        ),
    ] {
        assert_resource_not_found_equivalent(
            &app,
            get_as(denied_path, RESTRICTED_ONTOLOGY_TOKEN),
            get_as(missing_path, RESTRICTED_ONTOLOGY_TOKEN),
        )
        .await?;
    }

    for (id, name, arguments) in [
        (
            1,
            "a3s_cloud_ontologies_get",
            json!({"ontologyId": granted_ontology_id}),
        ),
        (
            2,
            "a3s_cloud_ontology_revisions_list",
            json!({"ontologyId": granted_ontology_id}),
        ),
        (
            3,
            "a3s_cloud_ontology_revisions_get",
            json!({
                "ontologyId": granted_ontology_id,
                "revisionId": granted_revision_id
            }),
        ),
    ] {
        let response = app
            .call(mcp_tool_call_as(
                id,
                name,
                arguments,
                RESTRICTED_ONTOLOGY_TOKEN,
            ))
            .await?;
        let response = response_json(&response)?;
        assert_eq!(response["result"]["isError"], false, "{name}");
        assert_eq!(response["result"]["structuredContent"]["code"], 200);
    }
    for (id, name, denied_arguments, missing_arguments) in [
        (
            4,
            "a3s_cloud_ontologies_get",
            json!({"ontologyId": denied_ontology_id}),
            json!({"ontologyId": Uuid::now_v7()}),
        ),
        (
            6,
            "a3s_cloud_ontology_revisions_list",
            json!({"ontologyId": denied_ontology_id}),
            json!({"ontologyId": Uuid::now_v7()}),
        ),
        (
            8,
            "a3s_cloud_ontology_revisions_get",
            json!({
                "ontologyId": denied_ontology_id,
                "revisionId": denied_revision_id
            }),
            json!({
                "ontologyId": Uuid::now_v7(),
                "revisionId": Uuid::now_v7()
            }),
        ),
        (
            10,
            "a3s_cloud_ontology_revisions_diff",
            json!({
                "ontologyId": denied_ontology_id,
                "fromRevisionId": denied_revision_id,
                "toRevisionId": denied_revision_id
            }),
            json!({
                "ontologyId": Uuid::now_v7(),
                "fromRevisionId": Uuid::now_v7(),
                "toRevisionId": Uuid::now_v7()
            }),
        ),
    ] {
        let denied_response = app
            .call(mcp_tool_call_as(
                id,
                name,
                denied_arguments,
                RESTRICTED_ONTOLOGY_TOKEN,
            ))
            .await?;
        let missing_response = app
            .call(mcp_tool_call_as(
                id + 1,
                name,
                missing_arguments,
                RESTRICTED_ONTOLOGY_TOKEN,
            ))
            .await?;
        assert_mcp_not_found_equivalent(&denied_response, &missing_response)?;
    }

    let denied_revision_acl = ONTOLOGY_ACL.replace(
        "Deterministic W0.1 Ontology contract fixture",
        "Denied Ontology revision",
    );
    let denied_mcp = app
        .call(mcp_tool_call_as(
            12,
            "a3s_cloud_ontologies_revise",
            json!({
                "ontologyId": denied_ontology_id,
                "acl": denied_revision_acl,
                "expectedVersion": 1,
                "idempotencyKey": "ontology-mcp-revise-denied"
            }),
            RESTRICTED_ONTOLOGY_TOKEN,
        ))
        .await?;
    let missing_mcp = app
        .call(mcp_tool_call_as(
            13,
            "a3s_cloud_ontologies_revise",
            json!({
                "ontologyId": Uuid::now_v7(),
                "acl": denied_revision_acl,
                "expectedVersion": 1,
                "idempotencyKey": "ontology-mcp-revise-missing"
            }),
            RESTRICTED_ONTOLOGY_TOKEN,
        ))
        .await?;
    assert_mcp_not_found_equivalent(&denied_mcp, &missing_mcp)?;
    assert_resource_not_found_equivalent(
        &app,
        post_acl_as(
            format!("{denied_root}/revisions"),
            "ontology-revise-denied",
            denied_revision_acl.as_bytes().to_vec(),
            RESTRICTED_ONTOLOGY_TOKEN,
        )
        .with_header("x-a3s-expected-version", "1"),
        post_acl_as(
            format!("{missing_root}/revisions"),
            "ontology-revise-missing",
            denied_revision_acl.as_bytes().to_vec(),
            RESTRICTED_ONTOLOGY_TOKEN,
        )
        .with_header("x-a3s-expected-version", "1"),
    )
    .await?;

    let granted_revision_acl = ONTOLOGY_ACL.replace(
        "Deterministic W0.1 Ontology contract fixture",
        "Granted Ontology revision",
    );
    let revise_granted = || {
        post_acl_as(
            format!("{granted_root}/revisions"),
            "ontology-revise-granted",
            granted_revision_acl.as_bytes().to_vec(),
            RESTRICTED_ONTOLOGY_TOKEN,
        )
        .with_header("x-a3s-expected-version", "1")
    };
    assert_eq!(app.call(revise_granted()).await?.status(), 201);
    let replay = app.call(revise_granted()).await?;
    assert_eq!(replay.status(), 200);
    assert_eq!(response_json(&replay)?["data"]["replayed"], true);

    let revoked = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/resource-grants/{project_grant_id}/revocation"
            ),
            "ontology-grants-revoke",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(revoked.status(), 200);
    assert_resource_not_found_equivalent(
        &app,
        revise_granted(),
        post_acl_as(
            format!("{missing_root}/revisions"),
            "ontology-revise-missing-after-revoke",
            granted_revision_acl.as_bytes().to_vec(),
            RESTRICTED_ONTOLOGY_TOKEN,
        )
        .with_header("x-a3s-expected-version", "1"),
    )
    .await?;
    assert_resource_not_found_equivalent(
        &app,
        get_as(&granted_root, RESTRICTED_ONTOLOGY_TOKEN),
        get_as(&missing_root, RESTRICTED_ONTOLOGY_TOKEN),
    )
    .await?;
    Ok(())
}

fn breaking_acl(compatible_acl: &str) -> String {
    let changed = compatible_acl.replacen(
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        1,
    );
    let body = changed
        .strip_suffix("}\r\n")
        .or_else(|| changed.strip_suffix("}\n"))
        .expect("public Ontology fixture must end with its root block");
    format!(
        "{body}\n  rule \"migrate_ticket_v2\" {{\n    label = \"Migrate ticket v2\"\n    kind = \"migration\"\n    expression_digest = \"sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff\"\n  }}\n}}\n"
    )
}
