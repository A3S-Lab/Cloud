use super::*;

type ValidatedRuntimeMaterial = (
    Option<WorkflowVariableContract>,
    Option<WorkflowVariableDefaults>,
    Option<WorkflowCompositeRegions>,
    bool,
    bool,
);

pub(super) fn validate(
    input: &WorkflowRunInput,
    resolved: &ResolvedWorkflowVariableContract,
    defaults: Option<&ResolvedWorkflowVariableDefaults>,
    regions: Option<&ResolvedWorkflowCompositeRegions>,
    application_projection: Option<&WorkflowRunApplicationProjection>,
) -> Result<ValidatedRuntimeMaterial, String> {
    super::v21::validate_for_generation(
        input,
        resolved,
        defaults,
        regions,
        application_projection,
        "v23",
    )
}
