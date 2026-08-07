mod capability_reference;
pub mod entities;
pub mod events;
mod ontology_contract;
pub mod repositories;
pub mod services;
mod validation;
pub mod value_objects;
mod workflow_contract;
mod workflow_graph;

pub use capability_reference::{CapabilityOwner, CapabilityReference, CapabilityType};
pub use entities::{Ontology, OntologyRevision, ONTOLOGY_COMPILER_SCHEMA_VERSION};
pub use events::OntologyRevisionPublished;
pub use ontology_contract::{
    OntologyContract, OntologyContractQuotas, OntologyObjectType, OntologyRelationCardinality,
    OntologyRelationType, OntologyRule, OntologyRuleKind, OntologySpec, ONTOLOGY_MAX_ACL_BYTES,
    ONTOLOGY_SCHEMA,
};
pub use repositories::{
    CreateOntologyWrite, IOntologyRepository, OntologyRecord, ReviseOntologyWrite,
};
pub use services::{
    diff_ontology_contracts, resolve_migration_policy, OntologyChange, OntologyChangeCompatibility,
    OntologyChangeKind, OntologyDiff, OntologyResourceKind,
};
pub use value_objects::{OntologyMigrationPolicy, OntologyName};
pub use workflow_contract::{
    WorkflowContract, WorkflowContractQuotas, WorkflowEdgeSpec, WorkflowSpec, WorkflowStepKind,
    WorkflowStepSpec, WORKFLOW_DEFINITION_SCHEMA,
};

#[cfg(test)]
mod authority_tests;
