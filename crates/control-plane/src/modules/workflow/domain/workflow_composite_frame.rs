use super::workflow_composite_regions::is_exact_child_workflow_revision;
use super::workflow_variable_materialization::{
    lookup_workflow_variable_path, materialize_workflow_variable_declaration,
    project_workflow_variable_reads, resolve_workflow_variable_assignment,
};
use super::{
    WorkflowCompositeRegionPolicy, WorkflowCompositeRegions, WorkflowPlan, WorkflowStepKind,
    WorkflowVariableContract, WorkflowVariableDefaults, WorkflowVariableMutationMode,
    WorkflowVariableReadMode, WorkflowVariableScope, WORKFLOW_PLAN_MAX_BYTES,
    WORKFLOW_RUN_INPUT_MAX_BYTES, WORKFLOW_RUN_OUTPUT_MAX_BYTES,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, OrganizationId, PlanRevisionId, ProjectId, Sha256Digest,
    WorkflowDefinitionId, WorkflowRevisionId, WorkflowRunId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

mod result;

pub const WORKFLOW_COMPOSITE_FRAME_SCHEMA: &str = "cloud.workflow.composite-frame.v1";
pub const WORKFLOW_COMPOSITE_FRAME_RESULT_SCHEMA: &str = "cloud.workflow.composite-frame-result.v1";
pub const WORKFLOW_COMPOSITE_FRAME_MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowCompositeFrameMode {
    Iteration,
    Loop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowCompositeFrameRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub region_step_id: String,
    /// Zero-based stable position within the immutable region policy bound.
    pub ordinal: u32,
    pub effective_input: Value,
    /// Parent-scope values reconstructed from immutable Run input and Flow
    /// history immediately before this region is entered.
    pub available_variables: BTreeMap<String, Value>,
}

/// Immutable input for one future Flow-owned composite child.
///
/// This value is runtime state, not product configuration. It contains only
/// exact Plan/contract authority and bounded semantic values; it owns no
/// scheduling, retry, queue, or child lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowCompositeFrame {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub variable_contract_digest: Sha256Digest,
    pub composite_regions_digest: Sha256Digest,
    pub region_step_id: String,
    pub mode: WorkflowCompositeFrameMode,
    pub ordinal: u32,
    pub child_workflow_definition_id: WorkflowDefinitionId,
    pub child_workflow_revision_id: WorkflowRevisionId,
    pub child_workflow_digest: Sha256Digest,
    pub typed_projection_authoritative: bool,
    pub child_input: Value,
    pub child_input_digest: Sha256Digest,
    pub captured_variables: BTreeMap<String, Value>,
    pub frame_digest: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowCompositeFrameResult {
    pub schema: String,
    pub frame_digest: Sha256Digest,
    pub child_output: Value,
    pub child_output_digest: Sha256Digest,
    pub local_variables: BTreeMap<String, Value>,
    pub run_variable_updates: BTreeMap<String, Value>,
    pub exported_variables: BTreeMap<String, Value>,
    pub result_digest: Sha256Digest,
}

impl WorkflowCompositeFrame {
    pub fn open(
        request: WorkflowCompositeFrameRequest,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
        defaults: Option<&WorkflowVariableDefaults>,
    ) -> Result<Self, String> {
        validate_request_authority(&request, plan, regions, variables)?;
        validate_defaults(variables, defaults)?;
        reject_application_variables(variables)?;

        let step = plan
            .steps
            .iter()
            .find(|step| step.id == request.region_step_id)
            .ok_or_else(|| "Workflow composite frame references a missing Plan step".to_owned())?;
        let policy = regions
            .resolve(&request.region_step_id)
            .ok_or_else(|| "Workflow composite frame has no immutable region policy".to_owned())?;
        let (mode, maximum_frames) = frame_mode_and_bound(policy);
        if request.ordinal >= maximum_frames {
            return Err("Workflow composite frame ordinal exceeds its immutable bound".into());
        }
        let capability = exact_child_capability(step)?;
        let child_workflow_revision_id = parse_child_revision(&capability.revision)?;

        let declarations = variables
            .spec()
            .declarations
            .iter()
            .map(|declaration| (declaration.name.as_str(), declaration))
            .collect::<BTreeMap<_, _>>();
        let mut captured_variables = request.available_variables;
        for (name, value) in &captured_variables {
            let declaration = declarations
                .get(name.as_str())
                .ok_or_else(|| format!("unknown Workflow frame variable {name:?}"))?;
            if matches!(
                declaration.scope,
                WorkflowVariableScope::CompositeLocal | WorkflowVariableScope::Application
            ) || (declaration.scope == WorkflowVariableScope::NodeOutput
                && declaration.source_step_id.as_deref() == Some(request.region_step_id.as_str()))
            {
                return Err(format!(
                    "Workflow frame caller cannot supply variable {name:?}"
                ));
            }
            validate_variable_value(name, &declaration.value_type, value)?;
        }

        for declaration in variables.spec().declarations.iter().filter(|declaration| {
            declaration.scope == WorkflowVariableScope::CompositeLocal
                && declaration.region_id.as_deref() == Some(request.region_step_id.as_str())
        }) {
            let default = defaults.and_then(|defaults| defaults.value(&declaration.name));
            let source = (declaration.mutation_mode == WorkflowVariableMutationMode::Immutable)
                .then_some(&request.effective_input);
            if let Some(value) =
                materialize_workflow_variable_declaration(declaration, source, default)?
            {
                captured_variables.insert(declaration.name.clone(), value);
            }
        }

        let projected = project_workflow_variable_reads(
            variables,
            &request.region_step_id,
            &captured_variables,
        )?;
        let (child_input, typed_projection_authoritative) = match projected {
            Some(projected) => (projected, true),
            None => (request.effective_input, false),
        };
        let child_input_bytes = canonical_json_bounded(
            &child_input,
            WORKFLOW_RUN_INPUT_MAX_BYTES,
            "Workflow composite child input",
        )?;

        let mut frame = Self {
            schema: WORKFLOW_COMPOSITE_FRAME_SCHEMA.into(),
            organization_id: request.organization_id,
            project_id: request.project_id,
            workflow_run_id: request.workflow_run_id,
            plan_revision_id: request.plan_revision_id,
            plan_digest: request.plan_digest,
            variable_contract_digest: variables.digest().clone(),
            composite_regions_digest: regions.digest().clone(),
            region_step_id: request.region_step_id,
            mode,
            ordinal: request.ordinal,
            child_workflow_definition_id: WorkflowDefinitionId::from_uuid(capability.resource_id),
            child_workflow_revision_id,
            child_workflow_digest: capability.digest.clone(),
            typed_projection_authoritative,
            child_input,
            child_input_digest: Sha256Digest::from_bytes(&child_input_bytes),
            captured_variables,
            frame_digest: Sha256Digest::from_bytes(&[]),
        };
        frame.frame_digest = frame.compute_digest()?;
        frame.validate(plan, regions, variables)?;
        Ok(frame)
    }

    pub fn validate(
        &self,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
    ) -> Result<(), String> {
        if self.schema != WORKFLOW_COMPOSITE_FRAME_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.workflow_run_id.as_uuid().is_nil()
            || self.plan_revision_id.as_uuid().is_nil()
            || self.region_step_id.is_empty()
            || self.region_step_id.len() > 96
        {
            return Err("Workflow composite frame authority is invalid".into());
        }
        validate_plan_bindings(plan, &self.plan_digest, regions, variables)?;
        if self.variable_contract_digest != *variables.digest()
            || self.composite_regions_digest != *regions.digest()
        {
            return Err("Workflow composite frame contract authority drifted".into());
        }
        reject_application_variables(variables)?;

        let step = plan
            .steps
            .iter()
            .find(|step| step.id == self.region_step_id)
            .ok_or_else(|| "Workflow composite frame references a missing Plan step".to_owned())?;
        let policy = regions
            .resolve(&self.region_step_id)
            .ok_or_else(|| "Workflow composite frame has no immutable region policy".to_owned())?;
        let (mode, maximum_frames) = frame_mode_and_bound(policy);
        if self.mode != mode || self.ordinal >= maximum_frames {
            return Err("Workflow composite frame policy authority drifted".into());
        }
        let capability = exact_child_capability(step)?;
        if self.child_workflow_definition_id.as_uuid() != capability.resource_id
            || self.child_workflow_revision_id != parse_child_revision(&capability.revision)?
            || self.child_workflow_digest != capability.digest
        {
            return Err("Workflow composite frame child revision drifted".into());
        }

        let declarations = variables
            .spec()
            .declarations
            .iter()
            .map(|declaration| (declaration.name.as_str(), declaration))
            .collect::<BTreeMap<_, _>>();
        for (name, value) in &self.captured_variables {
            let declaration = declarations
                .get(name.as_str())
                .ok_or_else(|| format!("unknown stored Workflow frame variable {name:?}"))?;
            match declaration.scope {
                WorkflowVariableScope::CompositeLocal
                    if declaration.region_id.as_deref() == Some(self.region_step_id.as_str()) => {}
                WorkflowVariableScope::CompositeLocal | WorkflowVariableScope::Application => {
                    return Err(format!(
                        "stored Workflow frame variable {name:?} crosses its authority"
                    ));
                }
                WorkflowVariableScope::NodeOutput
                    if declaration.source_step_id.as_deref()
                        == Some(self.region_step_id.as_str()) =>
                {
                    return Err(format!(
                        "stored Workflow frame variable {name:?} precedes its child result"
                    ));
                }
                _ => {}
            }
            validate_variable_value(name, &declaration.value_type, value)?;
        }
        for declaration in variables.spec().declarations.iter().filter(|declaration| {
            declaration.scope == WorkflowVariableScope::CompositeLocal
                && declaration.region_id.as_deref() == Some(self.region_step_id.as_str())
                && declaration.mutation_mode == WorkflowVariableMutationMode::Immutable
                && declaration.required
        }) {
            if !self.captured_variables.contains_key(&declaration.name) {
                return Err(format!(
                    "required Workflow frame input {:?} is unavailable",
                    declaration.name
                ));
            }
        }

        let projected = project_workflow_variable_reads(
            variables,
            &self.region_step_id,
            &self.captured_variables,
        )?;
        match (self.typed_projection_authoritative, projected) {
            (true, Some(projected)) if projected == self.child_input => {}
            (false, None) => {}
            _ => return Err("Workflow composite frame typed projection drifted".into()),
        }
        let child_input_bytes = canonical_json_bounded(
            &self.child_input,
            WORKFLOW_RUN_INPUT_MAX_BYTES,
            "Workflow composite child input",
        )?;
        if self.child_input_digest != Sha256Digest::from_bytes(&child_input_bytes)
            || self.frame_digest != self.compute_digest()?
        {
            return Err("Workflow composite frame digest drifted".into());
        }
        canonical_json_bounded(
            self,
            WORKFLOW_COMPOSITE_FRAME_MAX_BYTES,
            "Workflow composite frame",
        )?;
        Ok(())
    }

    pub fn resolve(
        &self,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
        child_output: Value,
    ) -> Result<WorkflowCompositeFrameResult, String> {
        self.validate(plan, regions, variables)?;
        let child_output_bytes = canonical_json_bounded(
            &child_output,
            WORKFLOW_RUN_OUTPUT_MAX_BYTES,
            "Workflow composite child output",
        )?;
        let export_targets = variables
            .spec()
            .exports
            .iter()
            .filter(|export| export.region_id == self.region_step_id)
            .map(|export| export.target_variable.as_str())
            .collect::<BTreeSet<_>>();
        let mut values = self.captured_variables.clone();

        for declaration in variables.spec().declarations.iter().filter(|declaration| {
            declaration.scope == WorkflowVariableScope::NodeOutput
                && declaration.source_step_id.as_deref() == Some(self.region_step_id.as_str())
                && !export_targets.contains(declaration.name.as_str())
        }) {
            if let Some(value) =
                materialize_workflow_variable_declaration(declaration, Some(&child_output), None)?
            {
                values.insert(declaration.name.clone(), value);
            }
        }

        let declarations = variables
            .spec()
            .declarations
            .iter()
            .map(|declaration| (declaration.name.as_str(), declaration))
            .collect::<BTreeMap<_, _>>();
        let updates = variables
            .spec()
            .assignments
            .iter()
            .filter(|assignment| assignment.writer_step_id == self.region_step_id)
            .map(|assignment| {
                resolve_workflow_variable_assignment(assignment, &values)
                    .map(|value| (assignment.target_variable.clone(), value))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut run_variable_updates = BTreeMap::new();
        for (target, value) in updates {
            let declaration = declarations
                .get(target.as_str())
                .ok_or_else(|| "Workflow frame assignment target disappeared".to_owned())?;
            if declaration.scope == WorkflowVariableScope::Run {
                run_variable_updates.insert(target.clone(), value.clone());
            }
            values.insert(target, value);
        }

        let mut local_variables = BTreeMap::new();
        for declaration in variables.spec().declarations.iter().filter(|declaration| {
            declaration.scope == WorkflowVariableScope::CompositeLocal
                && declaration.region_id.as_deref() == Some(self.region_step_id.as_str())
        }) {
            match values.get(&declaration.name) {
                Some(value) => {
                    validate_variable_value(&declaration.name, &declaration.value_type, value)?;
                    local_variables.insert(declaration.name.clone(), value.clone());
                }
                None if declaration.required => {
                    return Err(format!(
                        "required Workflow frame local {:?} is unavailable",
                        declaration.name
                    ));
                }
                None => {}
            }
        }

        let mut exported_variables = BTreeMap::new();
        for export in variables
            .spec()
            .exports
            .iter()
            .filter(|export| export.region_id == self.region_step_id)
        {
            let source = values.get(&export.source_variable).ok_or_else(|| {
                format!(
                    "Workflow frame export {:?} source is unavailable",
                    export.id
                )
            })?;
            let value =
                lookup_workflow_variable_path(source, &export.source_path).ok_or_else(|| {
                    format!(
                        "Workflow frame export {:?} source path is unavailable",
                        export.id
                    )
                })?;
            validate_variable_value(&export.id, &export.value_type, value)?;
            exported_variables.insert(export.target_variable.clone(), value.clone());
        }

        let mut result = WorkflowCompositeFrameResult {
            schema: WORKFLOW_COMPOSITE_FRAME_RESULT_SCHEMA.into(),
            frame_digest: self.frame_digest.clone(),
            child_output,
            child_output_digest: Sha256Digest::from_bytes(&child_output_bytes),
            local_variables,
            run_variable_updates,
            exported_variables,
            result_digest: Sha256Digest::from_bytes(&[]),
        };
        result.result_digest = result.compute_digest()?;
        result.validate(self, variables)?;
        Ok(result)
    }

    fn compute_digest(&self) -> Result<Sha256Digest, String> {
        let body = WorkflowCompositeFrameDigestBody {
            schema: &self.schema,
            organization_id: self.organization_id,
            project_id: self.project_id,
            workflow_run_id: self.workflow_run_id,
            plan_revision_id: self.plan_revision_id,
            plan_digest: &self.plan_digest,
            variable_contract_digest: &self.variable_contract_digest,
            composite_regions_digest: &self.composite_regions_digest,
            region_step_id: &self.region_step_id,
            mode: self.mode,
            ordinal: self.ordinal,
            child_workflow_definition_id: self.child_workflow_definition_id,
            child_workflow_revision_id: self.child_workflow_revision_id,
            child_workflow_digest: &self.child_workflow_digest,
            typed_projection_authoritative: self.typed_projection_authoritative,
            child_input: &self.child_input,
            child_input_digest: &self.child_input_digest,
            captured_variables: &self.captured_variables,
        };
        Ok(Sha256Digest::from_bytes(&canonical_json_bounded(
            &body,
            WORKFLOW_COMPOSITE_FRAME_MAX_BYTES,
            "Workflow composite frame digest body",
        )?))
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowCompositeFrameDigestBody<'a> {
    schema: &'a str,
    organization_id: OrganizationId,
    project_id: ProjectId,
    workflow_run_id: WorkflowRunId,
    plan_revision_id: PlanRevisionId,
    plan_digest: &'a Sha256Digest,
    variable_contract_digest: &'a Sha256Digest,
    composite_regions_digest: &'a Sha256Digest,
    region_step_id: &'a str,
    mode: WorkflowCompositeFrameMode,
    ordinal: u32,
    child_workflow_definition_id: WorkflowDefinitionId,
    child_workflow_revision_id: WorkflowRevisionId,
    child_workflow_digest: &'a Sha256Digest,
    typed_projection_authoritative: bool,
    child_input: &'a Value,
    child_input_digest: &'a Sha256Digest,
    captured_variables: &'a BTreeMap<String, Value>,
}

fn validate_request_authority(
    request: &WorkflowCompositeFrameRequest,
    plan: &WorkflowPlan,
    regions: &WorkflowCompositeRegions,
    variables: &WorkflowVariableContract,
) -> Result<(), String> {
    if request.organization_id.as_uuid().is_nil()
        || request.project_id.as_uuid().is_nil()
        || request.workflow_run_id.as_uuid().is_nil()
        || request.plan_revision_id.as_uuid().is_nil()
        || request.region_step_id.is_empty()
        || request.region_step_id.len() > 96
    {
        return Err("Workflow composite frame request authority is invalid".into());
    }
    validate_plan_bindings(plan, &request.plan_digest, regions, variables)
}

pub(super) fn validate_plan_bindings(
    plan: &WorkflowPlan,
    expected_digest: &Sha256Digest,
    regions: &WorkflowCompositeRegions,
    variables: &WorkflowVariableContract,
) -> Result<(), String> {
    plan.validate()?;
    let plan_bytes = canonical_json_bounded(plan, WORKFLOW_PLAN_MAX_BYTES, "Workflow plan")?;
    if &Sha256Digest::from_bytes(&plan_bytes) != expected_digest
        || plan.variable_contract_digest.as_ref() != Some(variables.digest())
        || plan.composite_regions_digest.as_ref() != Some(regions.digest())
    {
        return Err("Workflow composite frame Plan authority drifted".into());
    }
    if variables.id() != regions.spec().id
        || variables.revision() != regions.spec().revision
        || variables.compiler_schema_version() != regions.spec().compiler_schema_version
    {
        return Err("Workflow composite frame semantic contract identity drifted".into());
    }
    regions.validate_plan(plan)?;
    variables.validate_graph_bindings(&plan.workflow_spec()?)
}

fn validate_defaults(
    variables: &WorkflowVariableContract,
    defaults: Option<&WorkflowVariableDefaults>,
) -> Result<(), String> {
    let requires_defaults = variables
        .spec()
        .declarations
        .iter()
        .any(|declaration| declaration.default_value_digest.is_some());
    match (requires_defaults, defaults) {
        (false, None) => Ok(()),
        (true, Some(defaults)) => defaults.validate_contract(variables),
        (true, None) => Err("Workflow composite frame default material is unavailable".into()),
        (false, Some(_)) => Err("Workflow composite frame default material is unreferenced".into()),
    }
}

fn reject_application_variables(variables: &WorkflowVariableContract) -> Result<(), String> {
    if variables.spec().declarations.iter().any(|declaration| {
        declaration.scope == WorkflowVariableScope::Application
            || declaration.mutation_mode == WorkflowVariableMutationMode::OptimisticApplicationPort
    }) || variables
        .spec()
        .reads
        .iter()
        .any(|read| read.mode == WorkflowVariableReadMode::ApplicationPort)
    {
        return Err("Workflow composite frame v1 does not own Applications variables".into());
    }
    Ok(())
}

fn exact_child_capability(
    step: &super::WorkflowPlanStep,
) -> Result<&super::CapabilityReference, String> {
    if step.kind != WorkflowStepKind::Subworkflow {
        return Err("Workflow composite frame step is not a subworkflow".into());
    }
    let capability = step
        .capability
        .as_ref()
        .ok_or_else(|| "Workflow composite frame lost its child revision".to_owned())?;
    capability.validate()?;
    if !is_exact_child_workflow_revision(capability) {
        return Err("Workflow composite frame child revision is not exact".into());
    }
    Ok(capability)
}

fn parse_child_revision(value: &str) -> Result<WorkflowRevisionId, String> {
    let revision = Uuid::parse_str(value)
        .map_err(|_| "Workflow composite frame child revision ID is invalid".to_owned())?;
    if revision.is_nil() {
        return Err("Workflow composite frame child revision ID is nil".into());
    }
    Ok(WorkflowRevisionId::from_uuid(revision))
}

fn frame_mode_and_bound(
    policy: &WorkflowCompositeRegionPolicy,
) -> (WorkflowCompositeFrameMode, u32) {
    match policy {
        WorkflowCompositeRegionPolicy::Iteration(value) => {
            (WorkflowCompositeFrameMode::Iteration, value.maximum_items)
        }
        WorkflowCompositeRegionPolicy::Loop(value) => {
            (WorkflowCompositeFrameMode::Loop, value.maximum_iterations)
        }
    }
}

fn validate_variable_value(
    name: &str,
    value_type: &super::WorkflowDataType,
    value: &Value,
) -> Result<(), String> {
    if value_type.matches_json_value(value) {
        Ok(())
    } else {
        Err(format!(
            "Workflow frame value {name:?} does not match {}",
            value_type.as_str()
        ))
    }
}

fn validate_result_map(
    values: &BTreeMap<String, Value>,
    variables: &WorkflowVariableContract,
    frame: &WorkflowCompositeFrame,
    scope: WorkflowVariableScope,
) -> Result<(), String> {
    let declarations = variables
        .spec()
        .declarations
        .iter()
        .map(|declaration| (declaration.name.as_str(), declaration))
        .collect::<BTreeMap<_, _>>();
    let expected = variables
        .spec()
        .assignments
        .iter()
        .filter(|assignment| assignment.writer_step_id == frame.region_step_id)
        .filter_map(|assignment| {
            declarations
                .get(assignment.target_variable.as_str())
                .filter(|declaration| declaration.scope == scope)
                .map(|_| assignment.target_variable.as_str())
        })
        .collect::<BTreeSet<_>>();
    if values.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected {
        return Err("Workflow composite frame result update set drifted".into());
    }
    for (name, value) in values {
        let declaration = declarations
            .get(name.as_str())
            .ok_or_else(|| format!("unknown Workflow frame update {name:?}"))?;
        validate_variable_value(name, &declaration.value_type, value)?;
    }
    Ok(())
}
