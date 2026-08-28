#[cfg(test)]
mod lifecycle_tests;

pub mod entities;
pub mod events;
pub mod repositories;
pub mod services;
mod validation;
pub mod value_objects;

pub use entities::{FormDraft, FormRelease, FormReleaseSummary};
pub use events::{FormDraftChanged, FormReleasePublished};
pub use repositories::{
    CreateFormDraftWrite, FormPublicationRecord, IFormRepository, PublishFormReleaseWrite,
    ReviseFormDraftWrite,
};
pub use services::{FormSemanticCoreError, IFormSemanticCore};
pub use value_objects::{
    FormDocument, FormReleaseContent, CLOUD_FORM_DOCUMENT_MAX_BYTES,
    CLOUD_FORM_RELEASE_MAX_DOCUMENT_BYTES, CLOUD_FORM_RELEASE_MAX_PLAN_BYTES,
};
