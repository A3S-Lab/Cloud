mod capability_reference;
mod ontology_contract;
mod validation;
mod workflow_contract;
mod workflow_graph;

pub use capability_reference::{CapabilityOwner, CapabilityReference, CapabilityType};
pub use ontology_contract::{
    OntologyContract, OntologyContractQuotas, OntologyObjectType, OntologyRelationCardinality,
    OntologyRelationType, OntologyRule, OntologyRuleKind, OntologySpec, ONTOLOGY_SCHEMA,
};
pub use workflow_contract::{
    WorkflowContract, WorkflowContractQuotas, WorkflowEdgeSpec, WorkflowSpec, WorkflowStepKind,
    WorkflowStepSpec, WORKFLOW_DEFINITION_SCHEMA,
};

#[cfg(test)]
mod authority_tests;
