mod ontology_diff;

pub use ontology_diff::{
    diff_ontology_contracts, resolve_migration_policy, OntologyChange, OntologyChangeCompatibility,
    OntologyChangeKind, OntologyDiff, OntologyResourceKind,
};
