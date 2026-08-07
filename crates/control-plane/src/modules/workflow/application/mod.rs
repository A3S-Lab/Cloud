pub mod commands;
pub mod queries;

use crate::modules::workflow::domain::{OntologyDiff, OntologyRecord};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OntologyMutationResult {
    pub record: OntologyRecord,
    pub diff: Option<OntologyDiff>,
    pub replayed: bool,
}
