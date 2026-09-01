pub mod commands;
mod form_compilation;
mod project_access;
pub mod queries;
mod resource_access;

use crate::modules::forms::domain::{FormDraft, FormPublicationRecord};

pub use project_access::{FormProjectScope, IFormProjectAccess};
pub use resource_access::FormAccess;
pub(crate) use resource_access::FormAccessScope;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormDraftMutationResult {
    pub draft: FormDraft,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormPublicationMutationResult {
    pub publication: FormPublicationRecord,
    pub replayed: bool,
}

#[cfg(test)]
mod tests;
