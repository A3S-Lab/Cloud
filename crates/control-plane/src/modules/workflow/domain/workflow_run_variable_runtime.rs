use super::{
    ResolvedWorkflowRunStep, WorkflowPlan, WorkflowStepKind, WorkflowVariableContract,
    WorkflowVariableDefaults, WorkflowVariableMutationMode, WorkflowVariableReadMode,
    WorkflowVariableScope,
};
use std::collections::{BTreeMap, BTreeSet};
pub(crate) fn validate_runtime_variable_contract(
    contract: &WorkflowVariableContract,
    defaults: Option<&WorkflowVariableDefaults>,
    plan: &WorkflowPlan,
) -> Result<(), String> {
    validate_runtime_variable_contract_generation(contract, defaults, plan, false)
}

pub(crate) fn validate_application_runtime_variable_contract(
    contract: &WorkflowVariableContract,
    defaults: Option<&WorkflowVariableDefaults>,
    plan: &WorkflowPlan,
) -> Result<(), String> {
    if !contract
        .spec()
        .declarations
        .iter()
        .any(|declaration| declaration.scope == WorkflowVariableScope::Application)
    {
        return Err(
            "WorkflowRun Application variable runtime has no Application-scoped declaration".into(),
        );
    }
    validate_runtime_variable_contract_generation(contract, defaults, plan, true)
}

fn validate_runtime_variable_contract_generation(
    contract: &WorkflowVariableContract,
    defaults: Option<&WorkflowVariableDefaults>,
    plan: &WorkflowPlan,
    application_runtime: bool,
) -> Result<(), String> {
    for declaration in &contract.spec().declarations {
        if declaration.scope == WorkflowVariableScope::Application && !application_runtime {
            return Err(format!(
                "WorkflowRun runtime v2 does not execute {} variable {:?}",
                declaration.scope.as_str(),
                declaration.name
            ));
        }
        if declaration.mutation_mode == WorkflowVariableMutationMode::OptimisticApplicationPort
            && !application_runtime
        {
            return Err(format!(
                "WorkflowRun runtime v2 does not own application mutation for variable {:?}",
                declaration.name
            ));
        }
    }
    let requires_defaults = contract
        .spec()
        .declarations
        .iter()
        .any(|declaration| declaration.default_value_digest.is_some());
    match (requires_defaults, defaults) {
        (false, None) => {}
        (true, Some(defaults)) => defaults.validate_contract(contract)?,
        (true, None) => {
            return Err(
                "WorkflowRun runtime v2 cannot materialize digest-only defaults without immutable material"
                    .into(),
            )
        }
        (false, Some(_)) => {
            return Err(
                "WorkflowRun runtime v2 received unreferenced variable default material".into(),
            )
        }
    }
    if contract
        .spec()
        .reads
        .iter()
        .any(|read| read.mode == WorkflowVariableReadMode::ApplicationPort)
        && !application_runtime
    {
        return Err("WorkflowRun runtime v2 does not own application variable reads".into());
    }
    let step_kinds = plan
        .steps
        .iter()
        .map(|step| (step.id.as_str(), step.kind))
        .collect::<BTreeMap<_, _>>();
    for read in &contract.spec().reads {
        let kind = step_kinds
            .get(read.consumer_step_id.as_str())
            .ok_or_else(|| {
                format!(
                    "WorkflowRun variable read {:?} references a missing consumer",
                    read.id
                )
            })?;
        if matches!(
            kind,
            WorkflowStepKind::Input | WorkflowStepKind::HumanDecision
        ) {
            return Err(format!(
                "WorkflowRun runtime v2 does not project variables into {} step {:?}",
                kind.as_str(),
                read.consumer_step_id
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_typed_projection_configurations(
    contract: &WorkflowVariableContract,
    steps: &[ResolvedWorkflowRunStep],
) -> Result<(), String> {
    let consumers = contract
        .spec()
        .reads
        .iter()
        .map(|read| read.consumer_step_id.as_str())
        .collect::<BTreeSet<_>>();
    for step in steps {
        if !consumers.contains(step.plan.id.as_str()) {
            continue;
        }
        let bypasses_projection = match step.plan.kind {
            WorkflowStepKind::Transform | WorkflowStepKind::Output => step
                .configuration
                .template
                .as_deref()
                .map(template_uses_legacy_variable_token)
                .transpose()?
                .unwrap_or(false),
            WorkflowStepKind::Branch => step
                .configuration
                .selector
                .as_deref()
                .is_some_and(is_legacy_variable_token),
            _ => false,
        };
        if bypasses_projection {
            return Err(format!(
                "WorkflowRun runtime v2 step {:?} has explicit variable reads but bypasses their typed projection",
                step.plan.id
            ));
        }
    }
    Ok(())
}

fn template_uses_legacy_variable_token(source: &str) -> Result<bool, String> {
    let mut remainder = source;
    while let Some(start) = remainder.find("{{") {
        let token_source = &remainder[start + 2..];
        let end = token_source
            .find("}}")
            .ok_or_else(|| "Workflow template contains an unclosed token".to_owned())?;
        if is_legacy_variable_token(token_source[..end].trim()) {
            return Ok(true);
        }
        remainder = &token_source[end + 2..];
    }
    Ok(false)
}

fn is_legacy_variable_token(value: &str) -> bool {
    value == "input" || value.starts_with("input.") || value.starts_with("steps.")
}
