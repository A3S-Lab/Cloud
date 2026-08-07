mod ontology_repository;

pub(crate) use ontology_repository::OntologyWriteReference;
pub use ontology_repository::{
    CreateOntologyWrite, IOntologyRepository, OntologyRecord, ReviseOntologyWrite,
};
