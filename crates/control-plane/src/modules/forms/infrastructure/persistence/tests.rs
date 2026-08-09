use super::InMemoryFormRepository;
use crate::modules::forms::domain::{
    CreateFormDraftWrite, FormDocument, FormDraft, FormDraftChanged, FormPublicationRecord,
    FormRelease, FormReleaseContent, FormReleasePublished, IFormRepository,
    PublishFormReleaseWrite, ReviseFormDraftWrite,
};
use crate::modules::shared_kernel::domain::{
    FormId, FormReleaseId, IdempotencyRequest, OrganizationId, PrincipalId, ProjectId,
    RepositoryError,
};
use a3s_form_core::{canonicalize_json, compile_bytes, COMPILE_REQUEST_API_VERSION};
use chrono::{Duration, TimeZone, Utc};
use uuid::Uuid;

#[tokio::test]
async fn draft_writes_are_replay_safe_optimistic_and_project_name_unique() {
    let repository = InMemoryFormRepository::new();
    let created_at = timestamp(8, 0);
    let actor = PrincipalId::new();
    let draft = draft("Approval", actor, created_at);
    let create_idempotency = idempotency("forms", "create-approval", b"approval-v1");
    let create = CreateFormDraftWrite {
        event: FormDraftChanged::created(&draft, Uuid::now_v7()).expect("create event"),
        draft: draft.clone(),
        actor_principal_id: actor,
        request_id: Uuid::now_v7(),
        idempotency: create_idempotency.clone(),
    };

    let created = repository
        .create_draft(create.clone())
        .await
        .expect("create");
    assert!(!created.replayed);
    let replay = repository
        .create_draft(create)
        .await
        .expect("create replay");
    assert!(replay.replayed);
    assert_eq!(replay.value, draft);
    assert_eq!(repository.outbox_events().await.len(), 1);

    let conflicting_replay = idempotency("forms", "create-approval", b"different");
    assert_eq!(
        repository
            .replay_draft_write(&conflicting_replay)
            .await
            .expect_err("changed idempotent replay must fail"),
        RepositoryError::IdempotencyConflict
    );

    let duplicate = FormDraft::create(
        draft.organization_id,
        draft.project_id,
        FormId::new(),
        " approval ".into(),
        String::new(),
        document("Approval duplicate"),
        actor,
        created_at,
    )
    .expect("duplicate draft");
    assert!(matches!(
        repository
            .create_draft(CreateFormDraftWrite {
                event: FormDraftChanged::created(&duplicate, Uuid::now_v7())
                    .expect("duplicate event"),
                draft: duplicate,
                actor_principal_id: actor,
                request_id: Uuid::now_v7(),
                idempotency: idempotency("forms", "create-duplicate", b"duplicate"),
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let revised = draft
        .revise(
            1,
            "Approval request".into(),
            "Requires a reason".into(),
            document("Approval request"),
            actor,
            created_at + Duration::minutes(1),
        )
        .expect("revised draft");
    let revised_write = ReviseFormDraftWrite {
        event: FormDraftChanged::revised(&revised, Uuid::now_v7()).expect("revise event"),
        draft: revised.clone(),
        expected_version: 1,
        actor_principal_id: actor,
        request_id: Uuid::now_v7(),
        idempotency: idempotency("form-revisions", "revise-approval", b"approval-v2"),
    };
    assert!(
        !repository
            .revise_draft(revised_write.clone())
            .await
            .expect("revise")
            .replayed
    );
    assert!(repository
        .revise_draft(ReviseFormDraftWrite {
            idempotency: idempotency("form-revisions", "stale", b"stale"),
            ..revised_write
        })
        .await
        .is_err());
    assert_eq!(
        repository
            .find_draft(draft.organization_id, draft.id)
            .await
            .expect("find"),
        Some(revised)
    );
}

#[tokio::test]
async fn publishing_is_immutable_replay_safe_and_unique_per_source_draft_version() {
    let repository = InMemoryFormRepository::new();
    let actor = PrincipalId::new();
    let created_at = timestamp(9, 0);
    let draft = draft("Approval", actor, created_at);
    repository
        .create_draft(CreateFormDraftWrite {
            event: FormDraftChanged::created(&draft, Uuid::now_v7()).expect("event"),
            draft: draft.clone(),
            actor_principal_id: actor,
            request_id: Uuid::now_v7(),
            idempotency: idempotency("forms", "create", b"create"),
        })
        .await
        .expect("create");

    let release = FormRelease::publish(
        &draft,
        FormReleaseId::new(),
        compiled_content(&draft.document),
        actor,
        created_at + Duration::minutes(1),
    )
    .expect("release");
    let published = draft.record_release(1, &release).expect("published head");
    let publication = FormPublicationRecord {
        draft: published.clone(),
        release: release.clone(),
    };
    let publish_write = PublishFormReleaseWrite {
        event: FormReleasePublished::envelope(&published, &release, Uuid::now_v7())
            .expect("publish event"),
        publication: publication.clone(),
        expected_version: 1,
        actor_principal_id: actor,
        request_id: Uuid::now_v7(),
        idempotency: idempotency("form-releases", "publish", b"publish-v1"),
    };
    let first = repository
        .publish_release(publish_write.clone())
        .await
        .expect("publish");
    assert!(!first.replayed);
    let replay = repository
        .publish_release(publish_write.clone())
        .await
        .expect("publish replay");
    assert!(replay.replayed);
    assert_eq!(replay.value, publication);

    assert!(matches!(
        repository
            .publish_release(PublishFormReleaseWrite {
                idempotency: idempotency("form-releases", "publish-again", b"publish-v1"),
                ..publish_write
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert_eq!(
        repository
            .find_release(draft.organization_id, draft.id, release.id)
            .await
            .expect("find release"),
        Some(release.clone())
    );
    assert_eq!(
        repository
            .list_releases(draft.organization_id, draft.id)
            .await
            .expect("list releases"),
        vec![release]
    );
    assert_eq!(repository.outbox_events().await.len(), 2);
}

fn draft(name: &str, actor: PrincipalId, created_at: chrono::DateTime<Utc>) -> FormDraft {
    FormDraft::create(
        OrganizationId::new(),
        ProjectId::new(),
        FormId::new(),
        name.into(),
        String::new(),
        document(name),
        actor,
        created_at,
    )
    .expect("draft")
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

fn timestamp(hour: u32, minute: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 9, hour, minute, 0)
        .single()
        .expect("timestamp")
}
