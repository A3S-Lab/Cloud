mod ontology_diff;
mod workflow_plan_compiler;

pub use ontology_diff::{
    diff_ontology_contracts, resolve_migration_policy, OntologyChange, OntologyChangeCompatibility,
    OntologyChangeKind, OntologyDiff, OntologyResourceKind,
};
pub use workflow_plan_compiler::{CompiledWorkflowGoal, WorkflowPlanCompiler};
