pub mod application;
#[cfg(test)]
mod authority_tests;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::commands::{
    create_form_draft::{CreateFormDraft, CreateFormDraftHandler},
    publish_form_release::{PublishFormRelease, PublishFormReleaseHandler},
    revise_form_draft::{ReviseFormDraft, ReviseFormDraftHandler},
};
pub use application::queries::{
    get_form_draft::{GetFormDraft, GetFormDraftHandler},
    get_form_release::{GetFormRelease, GetFormReleaseHandler},
    list_form_drafts::{ListFormDrafts, ListFormDraftsHandler},
    list_form_releases::{ListFormReleases, ListFormReleasesHandler},
};
pub use application::{FormDraftMutationResult, FormPublicationMutationResult};

pub use domain::{
    AcceptedFormSubmission, CreateFormDraftWrite, FormDocument, FormDraft, FormDraftChanged,
    FormPublicationRecord, FormRelease, FormReleaseContent, FormReleasePublished,
    FormReleaseSummary, FormSemanticCoreError, FormSubmission, IFormRepository, IFormSemanticCore,
    PublishFormReleaseWrite, ReviseFormDraftWrite, CLOUD_FORM_DOCUMENT_MAX_BYTES,
    CLOUD_FORM_RELEASE_MAX_DOCUMENT_BYTES, CLOUD_FORM_RELEASE_MAX_PLAN_BYTES,
    CLOUD_FORM_SUBMISSION_MAX_VALUE_BYTES,
};
pub use infrastructure::{InMemoryFormRepository, NativeFormSemanticCore, PostgresFormRepository};
pub use presentation::FormsModule;
