use super::{
    validate_result_map, validate_variable_value, WorkflowCompositeFrame,
    WorkflowCompositeFrameResult, WORKFLOW_COMPOSITE_FRAME_MAX_BYTES,
    WORKFLOW_COMPOSITE_FRAME_RESULT_SCHEMA,
};
use crate::modules::shared_kernel::domain::{canonical_json_bounded, Sha256Digest};
use crate::modules::workflow::domain::{
    WorkflowVariableContract, WorkflowVariableScope, WORKFLOW_RUN_OUTPUT_MAX_BYTES,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

impl WorkflowCompositeFrameResult {
    pub fn validate(
        &self,
        frame: &WorkflowCompositeFrame,
        variables: &WorkflowVariableContract,
    ) -> Result<(), String> {
        if self.schema != WORKFLOW_COMPOSITE_FRAME_RESULT_SCHEMA
            || self.frame_digest != frame.frame_digest
        {
            return Err("Workflow composite frame result authority drifted".into());
        }
        if frame.variable_contract_digest != *variables.digest() {
            return Err("Workflow composite frame result contract authority drifted".into());
        }
        let output_bytes = canonical_json_bounded(
            &self.child_output,
            WORKFLOW_RUN_OUTPUT_MAX_BYTES,
            "Workflow composite child output",
        )?;
        if self.child_output_digest != Sha256Digest::from_bytes(&output_bytes) {
            return Err("Workflow composite child output digest drifted".into());
        }
        let declarations = variables
            .spec()
            .declarations
            .iter()
            .map(|declaration| (declaration.name.as_str(), declaration))
            .collect::<BTreeMap<_, _>>();
        for (name, value) in &self.local_variables {
            let declaration = declarations
                .get(name.as_str())
                .ok_or_else(|| format!("unknown Workflow frame local {name:?}"))?;
            if declaration.scope != WorkflowVariableScope::CompositeLocal
                || declaration.region_id.as_deref() != Some(frame.region_step_id.as_str())
            {
                return Err(format!("Workflow frame local {name:?} crossed its region"));
            }
            validate_variable_value(name, &declaration.value_type, value)?;
        }
        for declaration in variables.spec().declarations.iter().filter(|declaration| {
            declaration.scope == WorkflowVariableScope::CompositeLocal
                && declaration.region_id.as_deref() == Some(frame.region_step_id.as_str())
                && declaration.required
        }) {
            if !self.local_variables.contains_key(&declaration.name) {
                return Err(format!(
                    "required Workflow frame local {:?} is unavailable",
                    declaration.name
                ));
            }
        }
        validate_result_map(
            &self.run_variable_updates,
            variables,
            frame,
            WorkflowVariableScope::Run,
        )?;

        let expected_exports = variables
            .spec()
            .exports
            .iter()
            .filter(|export| export.region_id == frame.region_step_id)
            .map(|export| (export.target_variable.as_str(), export))
            .collect::<BTreeMap<_, _>>();
        if self.exported_variables.len() != expected_exports.len() {
            return Err("Workflow composite frame export set drifted".into());
        }
        for (name, value) in &self.exported_variables {
            let export = expected_exports
                .get(name.as_str())
                .ok_or_else(|| format!("unknown Workflow frame export {name:?}"))?;
            validate_variable_value(name, &export.value_type, value)?;
        }
        if self.result_digest != self.compute_digest()? {
            return Err("Workflow composite frame result digest drifted".into());
        }
        canonical_json_bounded(
            self,
            WORKFLOW_COMPOSITE_FRAME_MAX_BYTES,
            "Workflow composite frame result",
        )?;
        Ok(())
    }

    pub(super) fn compute_digest(&self) -> Result<Sha256Digest, String> {
        let body = WorkflowCompositeFrameResultDigestBody {
            schema: &self.schema,
            frame_digest: &self.frame_digest,
            child_output: &self.child_output,
            child_output_digest: &self.child_output_digest,
            local_variables: &self.local_variables,
            run_variable_updates: &self.run_variable_updates,
            exported_variables: &self.exported_variables,
        };
        Ok(Sha256Digest::from_bytes(&canonical_json_bounded(
            &body,
            WORKFLOW_COMPOSITE_FRAME_MAX_BYTES,
            "Workflow composite frame result digest body",
        )?))
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowCompositeFrameResultDigestBody<'a> {
    schema: &'a str,
    frame_digest: &'a Sha256Digest,
    child_output: &'a Value,
    child_output_digest: &'a Sha256Digest,
    local_variables: &'a BTreeMap<String, Value>,
    run_variable_updates: &'a BTreeMap<String, Value>,
    exported_variables: &'a BTreeMap<String, Value>,
}
