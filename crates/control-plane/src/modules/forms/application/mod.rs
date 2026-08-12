pub mod commands;
mod form_compilation;
pub mod queries;
mod resource_access;

use crate::modules::forms::domain::{FormDraft, FormPublicationRecord};

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
