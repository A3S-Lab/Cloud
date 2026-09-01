use super::commands::{
    create_form_draft::{CreateFormDraft, CreateFormDraftHandler},
    publish_form_release::{PublishFormRelease, PublishFormReleaseHandler},
    revise_form_draft::{ReviseFormDraft, ReviseFormDraftHandler},
};
use super::queries::{
    get_form_draft::{GetFormDraft, GetFormDraftHandler},
    get_form_release::{GetFormRelease, GetFormReleaseHandler},
    list_form_drafts::{ListFormDrafts, ListFormDraftsHandler},
    list_form_releases::{ListFormReleases, ListFormReleasesHandler},
};
use super::{FormAccess, FormAccessScope, FormProjectScope, IFormProjectAccess};
use crate::modules::forms::domain::IFormRepository;
use crate::modules::forms::infrastructure::{InMemoryFormRepository, NativeFormSemanticCore};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    OrganizationId, PrincipalId, ProjectId, RepositoryError,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn cqrs_form_lifecycle_compiles_publishes_replays_and_queries_one_authority() {
    let projects = project_access(true);
    let forms = Arc::new(InMemoryFormRepository::new());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let actor = PrincipalId::new();

    let create = CreateFormDraft {
        organization_id,
        project_id,
        name: "Approval".into(),
        description: "Manager approval".into(),
        document_json: document("Approval", false),
        actor_principal_id: actor,
        idempotency_key: "create-approval".into(),
        request_id: Uuid::now_v7(),
    };
    let create_handler = CreateFormDraftHandler::new(projects.clone(), forms.clone());
    let created = create_handler
        .execute(create.clone(), context())
        .await
        .expect("create command")
        .expect("create result");
    assert!(!created.replayed);
    let create_replay = create_handler
        .execute(create, context())
        .await
        .expect("create replay command")
        .expect("create replay result");
    assert!(create_replay.replayed);
    assert_eq!(create_replay.draft.id, created.draft.id);

    let denied = GetFormDraftHandler::new(forms.clone())
        .execute(
            GetFormDraft {
                organization_id,
                form_id: created.draft.id,
                access: FormAccess::restricted([FormAccessScope::Project {
                    project_id: ProjectId::new(),
                }]),
            },
            context(),
        )
        .await
        .expect("denied get command");
    assert_eq!(
        denied,
        Err(ApplicationError::NotFound("Form not found".into()))
    );

    let revise = ReviseFormDraft {
        organization_id,
        form_id: created.draft.id,
        access: FormAccess::organization_wide(),
        expected_version: 1,
        name: "Approval request".into(),
        description: "Manager approval with reason".into(),
        document_json: document("Approval request", true),
        actor_principal_id: actor,
        idempotency_key: "revise-approval".into(),
        request_id: Uuid::now_v7(),
    };
    let revised = ReviseFormDraftHandler::new(forms.clone())
        .execute(revise, context())
        .await
        .expect("revise command")
        .expect("revise result");
    assert_eq!(revised.draft.aggregate_version, 2);

    let publish = PublishFormRelease {
        organization_id,
        form_id: created.draft.id,
        access: FormAccess::organization_wide(),
        expected_version: 2,
        actor_principal_id: actor,
        idempotency_key: "publish-approval".into(),
        request_id: Uuid::now_v7(),
    };
    let publish_handler =
        PublishFormReleaseHandler::new(forms.clone(), Arc::new(NativeFormSemanticCore::new()));
    let published = publish_handler
        .execute(publish.clone(), context())
        .await
        .expect("publish command")
        .expect("publish result");
    assert!(!published.replayed);
    assert_eq!(published.publication.release.revision, 1);
    assert_eq!(published.publication.draft.aggregate_version, 3);
    let replay = publish_handler
        .execute(publish, context())
        .await
        .expect("publish replay command")
        .expect("publish replay result");
    assert!(replay.replayed);
    assert_eq!(replay.publication, published.publication);

    let fetched = GetFormDraftHandler::new(forms.clone())
        .execute(
            GetFormDraft {
                organization_id,
                form_id: created.draft.id,
                access: FormAccess::organization_wide(),
            },
            context(),
        )
        .await
        .expect("get command")
        .expect("get result");
    assert_eq!(fetched, published.publication.draft);
    let drafts = ListFormDraftsHandler::new(forms.clone())
        .execute(
            ListFormDrafts {
                organization_id,
                project_id,
            },
            context(),
        )
        .await
        .expect("list drafts command")
        .expect("list drafts result");
    assert_eq!(drafts, vec![published.publication.draft.clone()]);
    let release = GetFormReleaseHandler::new(forms.clone())
        .execute(
            GetFormRelease {
                organization_id,
                form_id: created.draft.id,
                release_id: published.publication.release.id,
                access: FormAccess::organization_wide(),
            },
            context(),
        )
        .await
        .expect("get release command")
        .expect("get release result");
    assert_eq!(release, published.publication.release);
    let releases = ListFormReleasesHandler::new(forms)
        .execute(
            ListFormReleases {
                organization_id,
                form_id: created.draft.id,
                access: FormAccess::organization_wide(),
            },
            context(),
        )
        .await
        .expect("list releases command")
        .expect("list releases result");
    assert_eq!(releases, vec![release]);
}

#[tokio::test]
async fn publish_rejects_form_core_diagnostics_without_persisting_a_release() {
    let projects = project_access(true);
    let forms = Arc::new(InMemoryFormRepository::new());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let actor = PrincipalId::new();
    let created = CreateFormDraftHandler::new(projects, forms.clone())
        .execute(
            CreateFormDraft {
                organization_id,
                project_id,
                name: "Invalid".into(),
                description: String::new(),
                document_json: "{}".into(),
                actor_principal_id: actor,
                idempotency_key: "create-invalid".into(),
                request_id: Uuid::now_v7(),
            },
            context(),
        )
        .await
        .expect("create command")
        .expect("create result");
    let error =
        PublishFormReleaseHandler::new(forms.clone(), Arc::new(NativeFormSemanticCore::new()))
            .execute(
                PublishFormRelease {
                    organization_id,
                    form_id: created.draft.id,
                    access: FormAccess::organization_wide(),
                    expected_version: 1,
                    actor_principal_id: actor,
                    idempotency_key: "publish-invalid".into(),
                    request_id: Uuid::now_v7(),
                },
                context(),
            )
            .await
            .expect("publish command")
            .expect_err("invalid Form publication must fail");
    assert!(matches!(error, ApplicationError::Invalid(_)));
    assert!(forms
        .list_releases(organization_id, created.draft.id)
        .await
        .expect("list releases")
        .is_empty());
    assert_eq!(
        forms
            .find_draft(organization_id, created.draft.id)
            .await
            .expect("find draft")
            .expect("draft")
            .aggregate_version,
        1
    );
}

#[tokio::test]
async fn create_requires_project_evidence_through_the_forms_owned_port() {
    let forms = Arc::new(InMemoryFormRepository::new());
    let result = CreateFormDraftHandler::new(project_access(false), forms.clone())
        .execute(
            CreateFormDraft {
                organization_id: OrganizationId::new(),
                project_id: ProjectId::new(),
                name: "Missing project".into(),
                description: String::new(),
                document_json: document("Missing project", false),
                actor_principal_id: PrincipalId::new(),
                idempotency_key: "missing-project".into(),
                request_id: Uuid::now_v7(),
            },
            context(),
        )
        .await
        .expect("create command");

    assert_eq!(
        result,
        Err(ApplicationError::NotFound("project not found".into()))
    );
}

struct StubFormProjectAccess {
    exists: bool,
}

#[async_trait]
impl IFormProjectAccess for StubFormProjectAccess {
    async fn project_exists(&self, _scope: FormProjectScope) -> Result<bool, RepositoryError> {
        Ok(self.exists)
    }
}

fn project_access(exists: bool) -> Arc<dyn IFormProjectAccess> {
    Arc::new(StubFormProjectAccess { exists })
}

fn document(title: &str, require_reason: bool) -> String {
    serde_json::to_string(&serde_json::json!({
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
    .expect("document")
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
