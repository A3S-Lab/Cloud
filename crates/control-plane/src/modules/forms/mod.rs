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
pub(crate) use application::FormAccessScope;
pub use application::{
    FormAccess, FormDraftMutationResult, FormProjectScope, FormPublicationMutationResult,
    IFormProjectAccess,
};

pub use domain::{
    CreateFormDraftWrite, FormDocument, FormDraft, FormDraftChanged, FormPublicationRecord,
    FormRelease, FormReleaseContent, FormReleasePublished, FormReleaseSummary,
    FormSemanticCoreError, IFormRepository, IFormSemanticCore, PublishFormReleaseWrite,
    ReviseFormDraftWrite, CLOUD_FORM_DOCUMENT_MAX_BYTES, CLOUD_FORM_RELEASE_MAX_DOCUMENT_BYTES,
    CLOUD_FORM_RELEASE_MAX_PLAN_BYTES,
};
pub use infrastructure::{
    InMemoryFormRepository, NativeFormSemanticCore, PostgresFormRepository,
    ProjectsFormProjectAccessAdapter,
};
pub use presentation::FormsModule;
