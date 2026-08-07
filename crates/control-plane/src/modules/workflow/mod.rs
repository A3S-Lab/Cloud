pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::commands::create_ontology::{CreateOntology, CreateOntologyHandler};
pub use application::commands::revise_ontology::{ReviseOntology, ReviseOntologyHandler};
pub use application::queries::diff_ontology_revisions::{
    DiffOntologyRevisions, DiffOntologyRevisionsHandler, OntologyRevisionDiff,
};
pub use application::queries::get_ontology::{GetOntology, GetOntologyHandler};
pub use application::queries::get_ontology_revision::{
    GetOntologyRevision, GetOntologyRevisionHandler,
};
pub use application::queries::list_ontologies::{ListOntologies, ListOntologiesHandler};
pub use application::queries::list_ontology_revisions::{
    ListOntologyRevisions, ListOntologyRevisionsHandler,
};
pub use application::OntologyMutationResult;

pub use domain::{
    CapabilityOwner, CapabilityReference, CapabilityType, IOntologyRepository, Ontology,
    OntologyChange, OntologyChangeCompatibility, OntologyChangeKind, OntologyContract,
    OntologyContractQuotas, OntologyDiff, OntologyMigrationPolicy, OntologyName,
    OntologyObjectType, OntologyRelationCardinality, OntologyRelationType, OntologyResourceKind,
    OntologyRevision, OntologyRule, OntologyRuleKind, OntologySpec, WorkflowContract,
    WorkflowContractQuotas, WorkflowEdgeSpec, WorkflowSpec, WorkflowStepKind, WorkflowStepSpec,
    ONTOLOGY_COMPILER_SCHEMA_VERSION, ONTOLOGY_MAX_ACL_BYTES, ONTOLOGY_SCHEMA,
    WORKFLOW_DEFINITION_SCHEMA,
};
pub use infrastructure::persistence::{InMemoryOntologyRepository, PostgresOntologyRepository};
pub use presentation::WorkflowModule;
