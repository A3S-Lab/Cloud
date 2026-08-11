use crate::modules::workflow::domain::{WorkflowPlan, WorkflowStepKind};

/// The first WorkflowRun slice deliberately admits only deterministic,
/// Workflow-local data steps. Other step kinds keep their compiled identities
/// but remain unavailable until their owning application ports and recovery
/// gates land.
pub fn validate_locally_executable_plan(plan: &WorkflowPlan) -> Result<(), String> {
    plan.validate()?;
    for step in &plan.steps {
        if !matches!(
            step.kind,
            WorkflowStepKind::Input
                | WorkflowStepKind::Transform
                | WorkflowStepKind::Branch
                | WorkflowStepKind::Output
        ) {
            return Err(format!(
                "Workflow step {:?} uses unavailable run capability {}; this WorkflowRun slice supports only input, transform, branch, and output",
                step.id,
                step.kind.as_str()
            ));
        }
    }
    Ok(())
}
