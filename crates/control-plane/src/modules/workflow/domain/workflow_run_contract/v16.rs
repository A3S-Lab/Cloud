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
    if let Some(projection) = application_projection {
        if !matches!(
            projection.schema.as_str(),
            WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA
                | WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V2
                | WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3
                | WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V4
                | WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5
        ) {
            return Err("WorkflowRun v16 Application projection version is unsupported".into());
        }
        projection.validate(&input.plan)?;
        if projection.schema == WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V4 {
            projection
                .frame_authority
                .as_ref()
                .ok_or_else(|| {
                    "WorkflowRun Application frame projection lost its authority".to_owned()
                })?
                .validate_for_child(
                    input.organization_id,
                    input.project_id,
                    input.workflow_run_id,
                    &input.plan,
                )?;
        }
    }

    let contract = resolved.restore()?;
    if input.plan.variable_contract_digest.as_ref() != Some(contract.digest()) {
        return Err("WorkflowRun variable contract drifted from the PlanRevision".into());
    }
    let defaults = defaults
        .map(ResolvedWorkflowVariableDefaults::restore)
        .transpose()?;
    match application_projection.filter(|projection| !projection.variable_step_ids.is_empty()) {
        Some(projection) => {
            validate_application_runtime_variable_contract(
                &contract,
                defaults.as_ref(),
                &input.plan,
            )?;
            projection.validate_variable_contract(&input.plan, &contract)?;
        }
        None => validate_runtime_variable_contract(&contract, defaults.as_ref(), &input.plan)?,
    }

    if application_projection.is_some_and(|projection| {
        projection.schema == WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5
    }) && regions.is_none()
    {
        return Err("WorkflowRun composite Application projection lost its region material".into());
    }
    let regions = regions
        .map(ResolvedWorkflowCompositeRegions::restore)
        .transpose()?;
    match (
        input.plan.composite_regions_digest.as_ref(),
        regions.as_ref().map(WorkflowCompositeRegions::digest),
    ) {
        (None, None) => {}
        (Some(expected), Some(actual)) if expected == actual => {}
        _ => {
            return Err(
                "WorkflowRun composite region material drifted from the PlanRevision".into(),
            )
        }
    }
    let composite_runtime = regions.is_some();
    Ok((Some(contract), defaults, regions, composite_runtime, true))
}
