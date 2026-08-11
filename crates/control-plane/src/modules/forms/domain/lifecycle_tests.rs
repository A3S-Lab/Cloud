use super::{FormDocument, FormDraft, FormRelease, FormReleaseContent};
use crate::modules::shared_kernel::domain::{
    FormId, FormReleaseId, OrganizationId, PrincipalId, ProjectId,
};
use a3s_form_core::{canonicalize_json, compile_bytes, COMPILE_REQUEST_API_VERSION};
use chrono::{Duration, TimeZone, Utc};

#[test]
fn draft_edits_and_release_publication_have_independent_monotonic_versions() {
    let created_at = Utc
        .with_ymd_and_hms(2026, 8, 9, 8, 0, 0)
        .single()
        .expect("valid timestamp");
    let actor = PrincipalId::new();
    let document = FormDocument::parse(&form_document("Approval", false)).expect("draft document");
    let draft = FormDraft::create(
        OrganizationId::new(),
        ProjectId::new(),
        FormId::new(),
        "Approval".into(),
        "Manager approval".into(),
        document,
        actor,
        created_at,
    )
    .expect("draft");
    assert_eq!(draft.aggregate_version, 1);
    assert!(draft.latest_release.is_none());

    let updated_at = created_at + Duration::minutes(1);
    let updated_document =
        FormDocument::parse(&form_document("Approval request", true)).expect("updated document");
    let draft = draft
        .revise(
            1,
            "Approval request".into(),
            "Manager approval with reason".into(),
            updated_document,
            actor,
            updated_at,
        )
        .expect("revision");
    assert_eq!(draft.aggregate_version, 2);
    assert!(draft.latest_release.is_none());
    assert!(draft
        .revise(
            1,
            draft.name.clone(),
            draft.description.clone(),
            draft.document.clone(),
            actor,
            updated_at,
        )
        .expect_err("stale draft revision must fail")
        .contains("expected version"));

    let published_at = updated_at + Duration::minutes(1);
    let release = FormRelease::publish(
        &draft,
        FormReleaseId::new(),
        compiled_content(&draft.document),
        actor,
        published_at,
    )
    .expect("release");
    assert_eq!(release.revision, 1);
    assert_eq!(release.source_draft_version, 2);
    assert_eq!(release.name, draft.name);
    assert_eq!(release.description, draft.description);
    let release_ref = release.release_ref().expect("release reference");
    assert_eq!(
        release_ref.organization_id,
        draft.organization_id.to_string()
    );
    assert_eq!(release_ref.project_id, draft.project_id.to_string());
    assert_eq!(release_ref.form_id, draft.id.to_string());
    assert_eq!(release_ref.release_id, release.id.to_string());
    assert_eq!(release_ref.revision, 1);
    assert_eq!(release_ref.digest, release.content.digest().as_str());
    release_ref.validate().expect("owner release reference");

    let published = draft
        .record_release(2, &release)
        .expect("published draft head");
    assert_eq!(published.aggregate_version, 3);
    let latest = published.latest_release.as_ref().expect("latest release");
    assert_eq!(latest.id, release.id);
    assert_eq!(latest.revision, 1);
    assert_eq!(latest.source_draft_version, 2);
    assert!(published.record_release(2, &release).is_err());

    let edited_again = published
        .revise(
            3,
            published.name.clone(),
            "Second release".into(),
            published.document.clone(),
            actor,
            published_at + Duration::minutes(1),
        )
        .expect("post-publication edit");
    assert_eq!(edited_again.aggregate_version, 4);
    let second = FormRelease::publish(
        &edited_again,
        FormReleaseId::new(),
        compiled_content(&edited_again.document),
        actor,
        published_at + Duration::minutes(2),
    )
    .expect("second release");
    assert_eq!(second.revision, 2);
    assert_eq!(second.source_draft_version, 4);
}

#[test]
fn drafts_reject_invalid_documents_stale_updates_and_no_op_edits() {
    assert!(FormDocument::parse(br#"[]"#)
        .expect_err("array Form document must fail")
        .contains("JSON object"));
    assert!(FormDocument::parse(br#"{"a":1,"a":2}"#).is_err());

    let created_at = Utc
        .with_ymd_and_hms(2026, 8, 9, 9, 0, 0)
        .single()
        .expect("valid timestamp");
    let actor = PrincipalId::new();
    let draft = FormDraft::create(
        OrganizationId::new(),
        ProjectId::new(),
        FormId::new(),
        "Approval".into(),
        String::new(),
        FormDocument::parse(&form_document("Approval", false)).expect("document"),
        actor,
        created_at,
    )
    .expect("draft");

    assert!(draft
        .revise(
            1,
            draft.name.clone(),
            draft.description.clone(),
            draft.document.clone(),
            actor,
            created_at + Duration::seconds(1),
        )
        .expect_err("no-op draft revision must fail")
        .contains("must change"));
    assert!(draft
        .revise(
            2,
            "Changed".into(),
            String::new(),
            draft.document.clone(),
            actor,
            created_at + Duration::seconds(1),
        )
        .expect_err("stale expected version must fail")
        .contains("expected version"));
    assert!(draft
        .revise(
            1,
            "Changed".into(),
            String::new(),
            draft.document.clone(),
            actor,
            created_at - Duration::seconds(1),
        )
        .expect_err("regressing draft timestamp must fail")
        .contains("precedes"));
}

#[test]
fn release_content_restore_rejects_digest_and_canonical_byte_drift() {
    let document = FormDocument::parse(&form_document("Approval", false)).expect("document");
    let content = compiled_content(&document);
    assert!(FormReleaseContent::restore(
        content.normalized_document_json().to_owned(),
        content.form_plan_json().to_owned(),
        content.compiler_revision().to_owned(),
        content.schema_profile().to_owned(),
        "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    )
    .is_err());
    assert!(FormReleaseContent::restore(
        format!("{}\n", content.normalized_document_json()),
        content.form_plan_json().to_owned(),
        content.compiler_revision().to_owned(),
        content.schema_profile().to_owned(),
        content.digest().as_str(),
    )
    .expect_err("noncanonical restored release content must fail")
    .contains("canonical"));
}

fn compiled_content(document: &FormDocument) -> FormReleaseContent {
    let document_value: serde_json::Value =
        serde_json::from_str(document.canonical_json()).expect("document JSON");
    let request = serde_json::to_vec(&serde_json::json!({
        "apiVersion": COMPILE_REQUEST_API_VERSION,
        "document": document_value,
    }))
    .expect("compile request");
    let response = compile_bytes(&request).expect("compile response");
    let response: serde_json::Value = serde_json::from_slice(&response).expect("response JSON");
    assert_eq!(response["ok"], true, "{response}");
    let plan = serde_json::to_vec(&response["formPlan"]).expect("plan JSON");
    let plan = canonicalize_json(&plan).expect("canonical plan");
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

fn form_document(title: &str, require_reason: bool) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "kind": "a3s.form",
        "apiVersion": "a3s.dev/form/v1alpha1",
        "revision": 1,
        "metadata": { "title": title },
        "schema": {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": {
                "approved": { "type": "boolean" },
                "reason": { "type": "string" }
            },
            "required": if require_reason {
                vec!["approved", "reason"]
            } else {
                vec!["approved"]
            },
            "additionalProperties": false
        },
        "ui": {
            "root": "root",
            "nodes": [
                { "id": "root", "kind": "root", "children": ["approved", "reason"] },
                {
                    "id": "approved",
                    "kind": "field",
                    "schemaPath": "/properties/approved",
                    "widget": "switch"
                },
                {
                    "id": "reason",
                    "kind": "field",
                    "schemaPath": "/properties/reason",
                    "widget": "textarea"
                }
            ]
        },
        "rules": [],
        "dataSources": [],
        "actions": []
    }))
    .expect("form document")
}
