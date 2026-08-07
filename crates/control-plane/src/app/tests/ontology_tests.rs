use super::*;

const ONTOLOGY_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/w0.1/ontology.acl"
));

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

fn breaking_acl(compatible_acl: &str) -> String {
    let changed = compatible_acl.replacen(
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        1,
    );
    let body = changed
        .strip_suffix("}\n")
        .expect("public Ontology fixture must end with its root block");
    format!(
        "{body}\n  rule \"migrate_ticket_v2\" {{\n    label = \"Migrate ticket v2\"\n    kind = \"migration\"\n    expression_digest = \"sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff\"\n  }}\n}}\n"
    )
}
