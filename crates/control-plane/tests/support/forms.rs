use crate::migrate_and_connect_for_test;
use a3s_cloud_control_plane::modules::forms::{
    CreateFormDraftWrite, FormDocument, FormDraft, FormDraftChanged, FormPublicationRecord,
    FormRelease, FormReleaseContent, FormReleasePublished, IFormRepository, PostgresFormRepository,
    PublishFormReleaseWrite, ReviseFormDraftWrite,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    FormId, FormReleaseId, IdempotencyRequest, OrganizationId, PrincipalId, ProjectId,
    RepositoryError,
};
use a3s_form_core::{canonicalize_json, compile_bytes, COMPILE_REQUEST_API_VERSION};
use a3s_orm::{sql_query, Database, PostgresDialect};
use chrono::{Duration, Utc};
use uuid::Uuid;

pub(super) async fn exercise_form_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let organization_id = OrganizationId::new();
    let other_organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let other_project_id = ProjectId::new();
    let actor = PrincipalId::new();
    let created_at = Utc::now();
    for (organization_id, name) in [
        (organization_id, "Form persistence tenant"),
        (other_organization_id, "Other Form tenant"),
    ] {
        database
            .execute(
                sql_query::<()>(
                    "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
                )
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(name)
                .append(", ")
                .bind(name.to_lowercase().replace(' ', "-"))
                .append(", 1, ")
                .bind(created_at)
                .append(")"),
            )
            .await?;
    }
    database
        .execute(
            sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (")
                .bind(actor.as_uuid())
                .append(", 'human', 'Form publisher', 1, ")
                .bind(created_at)
                .append(", null)"),
        )
        .await?;
    for (organization_id, project_id, name) in [
        (organization_id, project_id, "Forms"),
        (other_organization_id, other_project_id, "Other Forms"),
    ] {
        database
            .execute(
                sql_query::<()>("insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (")
                    .bind(organization_id.as_uuid())
                    .append(", ")
                    .bind(project_id.as_uuid())
                    .append(", ")
                    .bind(name)
                    .append(", ")
                    .bind(name.to_lowercase().replace(' ', "-"))
                    .append(", 1, ")
                    .bind(created_at)
                    .append(")"),
            )
            .await?;
    }

    let repository = PostgresFormRepository::new(executor.clone());
    let draft = FormDraft::create(
        organization_id,
        project_id,
        FormId::new(),
        "Approval".into(),
        "Manager approval".into(),
        document("Approval"),
        actor,
        created_at,
    )?;
    let create = CreateFormDraftWrite {
        event: FormDraftChanged::created(&draft, Uuid::now_v7())?,
        draft: draft.clone(),
        actor_principal_id: actor,
        request_id: Uuid::now_v7(),
        idempotency: idempotency("postgres-forms", "create", b"approval-v1"),
    };
    assert!(!repository.create_draft(create.clone()).await?.replayed);
    assert!(repository.create_draft(create).await?.replayed);

    let cross_tenant = FormDraft::create(
        other_organization_id,
        project_id,
        FormId::new(),
        "Cross tenant".into(),
        String::new(),
        document("Cross tenant"),
        actor,
        created_at,
    )?;
    assert_eq!(
        repository
            .create_draft(CreateFormDraftWrite {
                event: FormDraftChanged::created(&cross_tenant, Uuid::now_v7())?,
                draft: cross_tenant,
                actor_principal_id: actor,
                request_id: Uuid::now_v7(),
                idempotency: idempotency("postgres-forms", "cross-tenant", b"cross-tenant"),
            })
            .await
            .expect_err("cross-tenant Form draft creation must fail"),
        RepositoryError::NotFound
    );

    let revised = draft.revise(
        1,
        "Approval request".into(),
        "Manager approval with reason".into(),
        document("Approval request"),
        actor,
        created_at + Duration::seconds(1),
    )?;
    let revise = ReviseFormDraftWrite {
        event: FormDraftChanged::revised(&revised, Uuid::now_v7())?,
        draft: revised.clone(),
        expected_version: 1,
        actor_principal_id: actor,
        request_id: Uuid::now_v7(),
        idempotency: idempotency("postgres-form-revisions", "revise", b"approval-v2"),
    };
    assert!(!repository.revise_draft(revise.clone()).await?.replayed);
    assert!(repository.revise_draft(revise).await?.replayed);

    let release = FormRelease::publish(
        &revised,
        FormReleaseId::new(),
        compiled_content(&revised.document),
        actor,
        created_at + Duration::seconds(2),
    )?;
    let published = revised.record_release(2, &release)?;
    let publication = FormPublicationRecord {
        draft: published.clone(),
        release: release.clone(),
    };
    let publish = PublishFormReleaseWrite {
        event: FormReleasePublished::envelope(&published, &release, Uuid::now_v7())?,
        publication: publication.clone(),
        expected_version: 2,
        actor_principal_id: actor,
        request_id: Uuid::now_v7(),
        idempotency: idempotency("postgres-form-releases", "publish", b"approval-r1"),
    };
    assert!(!repository.publish_release(publish.clone()).await?.replayed);
    assert!(repository.publish_release(publish.clone()).await?.replayed);
    assert!(matches!(
        repository
            .publish_release(PublishFormReleaseWrite {
                idempotency: idempotency(
                    "postgres-form-releases",
                    "publish-different-key",
                    b"approval-r1"
                ),
                ..publish
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    assert_eq!(
        repository.find_draft(organization_id, draft.id).await?,
        Some(published.clone())
    );
    assert_eq!(
        repository
            .find_draft(other_organization_id, draft.id)
            .await?,
        None
    );
    assert_eq!(
        repository
            .find_release(organization_id, draft.id, release.id)
            .await?,
        Some(release.clone())
    );
    assert_eq!(
        repository.list_releases(organization_id, draft.id).await?,
        vec![release.clone()]
    );

    let persisted_counts = database
        .fetch_one_as(sql_query::<(i64, i64, i64, i64)>(
            "select (select count(*) from form_drafts), (select count(*) from form_releases), (select count(*) from audit_records where aggregate_id = ",
        )
        .bind(draft.id.as_uuid())
        .append("), (select count(*) from outbox_events where aggregate_id = ")
        .bind(draft.id.as_uuid())
        .append(")"))
        .await?;
    assert_eq!(persisted_counts, (1, 1, 3, 3));
    let idempotency_count = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from idempotency_records where scope_key like 'postgres-form%'",
        ))
        .await?;
    assert_eq!(idempotency_count, 3);

    assert!(database
        .execute(
            sql_query::<()>("update form_releases set name = name where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and form_id = ")
                .bind(draft.id.as_uuid())
                .append(" and id = ")
                .bind(release.id.as_uuid()),
        )
        .await
        .is_err());
    Ok(())
}

fn document(title: &str) -> FormDocument {
    FormDocument::parse(
        &serde_json::to_vec(&serde_json::json!({
            "kind": "a3s.form",
            "apiVersion": "a3s.dev/form/v1alpha1",
            "revision": 1,
            "metadata": { "title": title },
            "schema": {
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": { "approved": { "type": "boolean" } },
                "required": ["approved"],
                "additionalProperties": false
            },
            "ui": {
                "root": "root",
                "nodes": [
                    { "id": "root", "kind": "root", "children": ["approved"] },
                    {
                        "id": "approved",
                        "kind": "field",
                        "schemaPath": "/properties/approved",
                        "widget": "switch"
                    }
                ]
            },
            "rules": [],
            "dataSources": [],
            "actions": []
        }))
        .expect("document JSON"),
    )
    .expect("Form document")
}

fn compiled_content(document: &FormDocument) -> FormReleaseContent {
    let document: serde_json::Value =
        serde_json::from_str(document.canonical_json()).expect("document JSON");
    let request = serde_json::to_vec(&serde_json::json!({
        "apiVersion": COMPILE_REQUEST_API_VERSION,
        "document": document,
    }))
    .expect("compile request");
    let response = compile_bytes(&request).expect("compile response");
    let response: serde_json::Value = serde_json::from_slice(&response).expect("response JSON");
    assert_eq!(response["ok"], true, "{response}");
    let plan =
        canonicalize_json(&serde_json::to_vec(&response["formPlan"]).expect("plan should encode"))
            .expect("canonical plan");
    FormReleaseContent::restore(
        response["normalizedDocumentJson"]
            .as_str()
            .expect("normalized document")
            .to_owned(),
        String::from_utf8(plan).expect("UTF-8 plan"),
        response["compilerRevision"]
            .as_str()
            .expect("compiler revision")
            .to_owned(),
        response["schemaProfile"]
            .as_str()
            .expect("schema profile")
            .to_owned(),
        response["digest"].as_str().expect("digest"),
    )
    .expect("release content")
}

fn idempotency(scope: &str, key: &str, content: &[u8]) -> IdempotencyRequest {
    IdempotencyRequest::new(scope, key, content).expect("idempotency")
}
