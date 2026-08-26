use super::{WorkflowLocalStepInput, WorkflowLocalStepResult};
use crate::modules::shared_kernel::domain::{canonical_json_bounded, sha256_digest, Sha256Digest};
use crate::modules::workflow::domain::{
    descriptor_failure_output, WorkflowDataSchema, WorkflowStepKind, WORKFLOW_RUN_OUTPUT_MAX_BYTES,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V11,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V12, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V13,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V14, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V15,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V16, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V17,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V18, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V19,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V2, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V20,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V21, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V22,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V23, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V24,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V25, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V3,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V4, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V5,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V6, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V7,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V9,
};
use serde_json::Value;

mod list_operator;
mod variable_aggregate;

pub(super) fn execute_local_step(
    input: &WorkflowLocalStepInput,
) -> Result<WorkflowLocalStepResult, String> {
    let allow_current_template = matches!(
        input.runtime_contract_revision.as_str(),
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V2
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V3
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V4
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V5
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V6
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V7
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V9
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V11
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V12
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V13
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V14
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V15
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V16
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V17
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V18
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V19
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V20
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V21
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V22
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V23
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V24
            | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V25
    );
    let allow_legacy_tokens = !input.typed_projection_authoritative;
    if let Some(failure) = input.routed_failure.as_ref() {
        if !matches!(
            input.runtime_contract_revision.as_str(),
            WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V19
                | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V20
                | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V21
                | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V22
                | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V23
                | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V24
                | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V25
        ) || input.step.plan.kind != WorkflowStepKind::Subworkflow
            || input.composite_region_result.is_some()
        {
            return Err("Workflow routed failure materializer has invalid authority".into());
        }
        failure.validate(&input.step)?;
        let selected_handle =
            descriptor_failure_output(input.step.plan.failure.as_ref().ok_or_else(|| {
                "Workflow routed failure materializer lost its immutable contract".to_owned()
            })?)?
            .name
            .clone();
        let output = serde_json::to_value(failure)
            .map_err(|error| format!("Workflow routed failure is not serializable: {error}"))?;
        let result = WorkflowLocalStepResult {
            step_id: input.step.plan.id.clone(),
            kind: input.step.plan.kind,
            output_digest: value_digest(&output, "Workflow routed failure")?,
            output,
            selected_handle: Some(selected_handle),
            composite_region_result: None,
            default_output_evidence: None,
        };
        result.validate(&input.step)?;
        return Ok(result);
    }
    if input.step.plan.kind == WorkflowStepKind::Subworkflow {
        let region = input
            .composite_region_result
            .clone()
            .ok_or_else(|| "Workflow composite local step lost its region result".to_owned())?;
        let result = WorkflowLocalStepResult {
            step_id: input.step.plan.id.clone(),
            kind: input.step.plan.kind,
            output: region.output.clone(),
            output_digest: region.output_digest.clone(),
            selected_handle: None,
            composite_region_result: Some(region),
            default_output_evidence: None,
        };
        result.validate(&input.step)?;
        validate_data_schema(
            &input.step.output_schema,
            &result.output,
            "Workflow composite step output",
        )?;
        return Ok(result);
    }
    if input.composite_region_result.is_some() {
        return Err("non-composite Workflow local step retained region evidence".into());
    }
    validate_data_schema(
        &input.step.input_schema,
        &input.effective_input,
        "Workflow step input",
    )?;
    let (output, selected_handle) = match input.step.plan.kind {
        WorkflowStepKind::Input => (input.workflow_input.clone(), None),
        WorkflowStepKind::Transform => {
            if let Some(configuration) = input.step.configuration.list_operator() {
                if !matches!(
                    input.runtime_contract_revision.as_str(),
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V21
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V22
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V23
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V24
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V25
                ) {
                    return Err(
                        "Workflow List Operator requires runtime contract v21 or a later composing generation"
                            .into(),
                    );
                }
                return finish_local_transform(
                    input,
                    list_operator::execute(
                        configuration,
                        &input.effective_input,
                        input.typed_projection_authoritative,
                    )?,
                );
            }
            if let Some(configuration) = input.step.configuration.variable_aggregate() {
                if !matches!(
                    input.runtime_contract_revision.as_str(),
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V20
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V21
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V22
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V23
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V24
                        | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V25
                ) {
                    return Err(
                        "Workflow Variable Aggregator requires runtime contract v20 or a later composing generation"
                            .into(),
                    );
                }
                return finish_local_transform(
                    input,
                    variable_aggregate::execute(
                        configuration,
                        &input.effective_input,
                        input.typed_projection_authoritative,
                    )?,
                );
            }
            let template = input
                .step
                .configuration
                .template
                .as_deref()
                .ok_or_else(|| "Workflow transform template is missing".to_owned())?;
            (
                render_template(
                    template,
                    &input.workflow_input,
                    &input.effective_input,
                    &input.steps,
                    allow_current_template,
                    allow_legacy_tokens,
                )?,
                None,
            )
        }
        WorkflowStepKind::Branch => {
            let selector = input
                .step
                .configuration
                .selector
                .as_deref()
                .ok_or_else(|| "Workflow branch selector is missing".to_owned())?;
            let selected = lookup_selector(
                selector,
                &input.workflow_input,
                &input.effective_input,
                &input.steps,
                allow_legacy_tokens,
            )?;
            let selected_text = scalar_text(selected);
            let handle = input
                .step
                .configuration
                .routes
                .iter()
                .find(|route| route.equals == selected_text)
                .map(|route| route.handle.clone())
                .or_else(|| input.step.configuration.default_handle.clone())
                .ok_or_else(|| "Workflow branch default handle is missing".to_owned())?;
            (input.effective_input.clone(), Some(handle))
        }
        WorkflowStepKind::Output => match input.step.configuration.template.as_deref() {
            Some(template) => (
                render_template(
                    template,
                    &input.workflow_input,
                    &input.effective_input,
                    &input.steps,
                    allow_current_template,
                    allow_legacy_tokens,
                )?,
                None,
            ),
            None => (input.effective_input.clone(), None),
        },
        kind => {
            return Err(format!(
                "WorkflowRun local executor does not support {}",
                kind.as_str()
            ))
        }
    };
    validate_data_schema(&input.step.output_schema, &output, "Workflow step output")?;
    let output_digest = value_digest(&output, "Workflow step output")?;
    let result = WorkflowLocalStepResult {
        step_id: input.step.plan.id.clone(),
        kind: input.step.plan.kind,
        output,
        output_digest,
        selected_handle,
        composite_region_result: None,
        default_output_evidence: None,
    };
    result.validate(&input.step)?;
    Ok(result)
}

fn finish_local_transform(
    input: &WorkflowLocalStepInput,
    output: Value,
) -> Result<WorkflowLocalStepResult, String> {
    validate_data_schema(&input.step.output_schema, &output, "Workflow step output")?;
    let result = WorkflowLocalStepResult {
        step_id: input.step.plan.id.clone(),
        kind: input.step.plan.kind,
        output_digest: value_digest(&output, "Workflow step output")?,
        output,
        selected_handle: None,
        composite_region_result: None,
        default_output_evidence: None,
    };
    result.validate(&input.step)?;
    Ok(result)
}

pub(super) fn validate_data_schema(
    schema: &WorkflowDataSchema,
    value: &Value,
    label: &str,
) -> Result<(), String> {
    schema.validate_value(value, label)
}

pub(super) fn value_digest(value: &Value, label: &str) -> Result<Sha256Digest, String> {
    let canonical = canonical_json_bounded(value, WORKFLOW_RUN_OUTPUT_MAX_BYTES, label)?;
    Sha256Digest::parse(sha256_digest(&canonical))
}

fn render_template(
    source: &str,
    workflow_input: &Value,
    effective_input: &Value,
    steps: &std::collections::BTreeMap<String, Value>,
    allow_current: bool,
    allow_legacy: bool,
) -> Result<Value, String> {
    let trimmed = source.trim();
    if let Some(token) = whole_token(trimmed) {
        return lookup_token(
            token,
            workflow_input,
            effective_input,
            steps,
            allow_current,
            allow_legacy,
        )
        .cloned();
    }
    let mut output = String::new();
    let mut remainder = source;
    while let Some(start) = remainder.find("{{") {
        output.push_str(&remainder[..start]);
        let token_source = &remainder[start + 2..];
        let end = token_source
            .find("}}")
            .ok_or_else(|| "Workflow template contains an unclosed token".to_owned())?;
        let token = token_source[..end].trim();
        output.push_str(&scalar_text(lookup_token(
            token,
            workflow_input,
            effective_input,
            steps,
            allow_current,
            allow_legacy,
        )?));
        remainder = &token_source[end + 2..];
    }
    if remainder.contains("}}") {
        return Err("Workflow template contains an unmatched closing token".into());
    }
    output.push_str(remainder);
    Ok(Value::String(output))
}

fn whole_token(source: &str) -> Option<&str> {
    source
        .strip_prefix("{{")?
        .strip_suffix("}}")
        .map(str::trim)
        .filter(|token| !token.is_empty() && !token.contains("}}"))
}

fn lookup_token<'a>(
    token: &str,
    workflow_input: &'a Value,
    effective_input: &'a Value,
    steps: &'a std::collections::BTreeMap<String, Value>,
    allow_current: bool,
    allow_legacy: bool,
) -> Result<&'a Value, String> {
    if allow_legacy && token == "input" {
        return Ok(workflow_input);
    }
    if allow_legacy {
        if let Some(path) = token.strip_prefix("input.") {
            return lookup_path(workflow_input, path)
                .ok_or_else(|| format!("Workflow template token {token:?} was not found"));
        }
    }
    if allow_current && token == "current" {
        return Ok(effective_input);
    }
    if allow_current {
        if let Some(path) = token.strip_prefix("current.") {
            return lookup_path(effective_input, path)
                .ok_or_else(|| format!("Workflow template token {token:?} was not found"));
        }
    }
    if allow_legacy {
        if let Some(path) = token.strip_prefix("steps.") {
            let (step_id, nested) = path.split_once('.').unwrap_or((path, ""));
            let value = steps
                .get(step_id)
                .ok_or_else(|| format!("Workflow template step {step_id:?} is unavailable"))?;
            return if nested.is_empty() {
                Ok(value)
            } else {
                lookup_path(value, nested)
                    .ok_or_else(|| format!("Workflow template token {token:?} was not found"))
            };
        }
    }
    let allowed = match (allow_current, allow_legacy) {
        (true, true) => "input, current, or steps.<id>",
        (true, false) => "current",
        (false, true) => "input or steps.<id>",
        (false, false) => "no data token",
    };
    Err(format!(
        "unsupported Workflow template token {token:?}; use {allowed}"
    ))
}

fn lookup_selector<'a>(
    selector: &str,
    workflow_input: &'a Value,
    effective_input: &'a Value,
    steps: &'a std::collections::BTreeMap<String, Value>,
    allow_legacy: bool,
) -> Result<&'a Value, String> {
    if allow_legacy
        && (selector == "input" || selector.starts_with("input.") || selector.starts_with("steps."))
    {
        return lookup_token(selector, workflow_input, effective_input, steps, true, true)
            .map_err(|error| error.replace("template", "branch selector"));
    }
    if selector == "current" {
        return Ok(effective_input);
    }
    if let Some(path) = selector.strip_prefix("current.") {
        return lookup_path(effective_input, path)
            .ok_or_else(|| format!("Workflow branch selector {selector:?} was not found"));
    }
    let allowed = if allow_legacy {
        "input, current, or steps.<id>"
    } else {
        "current"
    };
    Err(format!(
        "unsupported Workflow branch selector {selector:?}; use {allowed}"
    ))
}

fn lookup_path<'a>(mut value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(value);
    }
    for segment in path.split('.') {
        value = match value {
            Value::Object(object) => object.get(segment)?,
            Value::Array(values) => values.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(value)
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::workflow::domain::{
        WorkflowDataField, WorkflowDataType, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V14, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V15,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V2,
    };
    use serde_json::json;

    #[test]
    fn templates_preserve_typed_whole_tokens_and_interpolate_scalars() {
        let input = json!({"name": "Ada", "items": [1, 2]});
        let current = json!({"name": "Grace"});
        let steps = std::collections::BTreeMap::from([("draft".into(), json!({"id": 7}))]);
        assert_eq!(
            render_template("{{input.items}}", &input, &current, &steps, false, true)
                .expect("typed token"),
            json!([1, 2])
        );
        assert_eq!(
            render_template(
                "Hello {{input.name}} #{{steps.draft.id}}",
                &input,
                &current,
                &steps,
                false,
                true,
            )
            .expect("interpolation"),
            json!("Hello Ada #7")
        );
        assert_eq!(
            render_template("{{current.name}}", &input, &current, &steps, true, false)
                .expect("current token"),
            json!("Grace")
        );
        assert!(render_template("{{input.name}}", &input, &current, &steps, true, false).is_err());
        assert!(
            render_template("{{current.name}}", &input, &current, &steps, false, true).is_err()
        );
        assert!(
            render_template("{{steps.missing}}", &input, &current, &steps, false, true).is_err()
        );
        assert!(render_template("{{input.name", &input, &current, &steps, false, true).is_err());
    }

    #[test]
    fn closed_data_shape_checks_required_field_types() {
        let schema = WorkflowDataSchema {
            value_type: WorkflowDataType::Object,
            fields: vec![WorkflowDataField {
                name: "name".into(),
                value_type: WorkflowDataType::String,
                required: true,
            }],
        };
        validate_data_schema(&schema, &json!({"name": "Ada"}), "input").expect("valid");
        assert!(validate_data_schema(&schema, &json!({}), "input").is_err());
        assert!(validate_data_schema(&schema, &json!({"name": 1}), "input").is_err());
    }

    #[test]
    fn local_executor_exposes_projected_current_to_typed_runtime_versions() {
        let input = crate::modules::workflow::test_support::typed_variable_workflow_run_input()
            .expect("typed-variable WorkflowRun input");
        let mut step = input
            .resolved_steps()
            .expect("resolved steps")
            .into_iter()
            .find(|step| step.plan.id == "output")
            .expect("output step");
        step.configuration.template = Some("{{current.result.ticketId}}".into());
        let step_input =
            |runtime_contract_revision: &str, authoritative: bool| WorkflowLocalStepInput {
                runtime_contract_revision: runtime_contract_revision.into(),
                typed_projection_authoritative: authoritative,
                step: step.clone(),
                workflow_input: input.goal_input.clone(),
                effective_input: json!({"result": input.goal_input}),
                dependencies: std::collections::BTreeMap::new(),
                steps: std::collections::BTreeMap::new(),
                routed_failure: None,
                composite_region_result: None,
            };

        for runtime_revision in [
            WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V2,
            WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V14,
            WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V15,
        ] {
            let result = execute_local_step(&step_input(runtime_revision, true))
                .expect("typed runtime projected current");
            assert_eq!(result.output, json!("T-42"));
        }
        assert!(
            execute_local_step(&step_input(WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION, false)).is_err()
        );

        let mut bypass = step_input(WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V2, true);
        bypass.step.configuration.template = Some("{{input.ticketId}}".into());
        assert!(execute_local_step(&bypass)
            .expect_err("typed projection must reject legacy input tokens")
            .contains("use current"));
        bypass.step.configuration.template = Some("{{steps.input.ticketId}}".into());
        assert!(execute_local_step(&bypass)
            .expect_err("typed projection must reject legacy step tokens")
            .contains("use current"));
    }

    #[test]
    fn local_executor_runs_input_transform_branch_and_output_with_typed_results() {
        let input = crate::modules::workflow::test_support::workflow_run_input()
            .expect("WorkflowRun input");
        let resolved = input
            .resolved_steps()
            .expect("resolved steps")
            .into_iter()
            .map(|step| (step.plan.id.clone(), step))
            .collect::<std::collections::BTreeMap<_, _>>();
        let run = |step_id: &str,
                   effective_input: Value,
                   dependencies: std::collections::BTreeMap<String, Value>,
                   steps: std::collections::BTreeMap<String, Value>| {
            execute_local_step(&WorkflowLocalStepInput {
                runtime_contract_revision: WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION.into(),
                typed_projection_authoritative: false,
                step: resolved.get(step_id).expect("resolved step").clone(),
                workflow_input: input.goal_input.clone(),
                effective_input,
                dependencies,
                steps,
                routed_failure: None,
                composite_region_result: None,
            })
        };

        let input_result = run(
            "input",
            input.goal_input.clone(),
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::new(),
        )
        .expect("input step");
        assert_eq!(input_result.output, input.goal_input);

        let normalize_result = run(
            "normalize",
            input_result.output.clone(),
            std::collections::BTreeMap::from([("input".into(), input_result.output.clone())]),
            std::collections::BTreeMap::from([("input".into(), input_result.output.clone())]),
        )
        .expect("transform step");
        assert_eq!(normalize_result.output, input.goal_input);

        let route_result = run(
            "route",
            normalize_result.output.clone(),
            std::collections::BTreeMap::from([(
                "normalize".into(),
                normalize_result.output.clone(),
            )]),
            std::collections::BTreeMap::from([
                ("input".into(), input_result.output.clone()),
                ("normalize".into(), normalize_result.output.clone()),
            ]),
        )
        .expect("branch step");
        assert_eq!(route_result.selected_handle.as_deref(), Some("high"));

        let high_result = run(
            "high",
            route_result.output.clone(),
            std::collections::BTreeMap::from([("route".into(), route_result.output.clone())]),
            std::collections::BTreeMap::from([
                ("input".into(), input_result.output),
                ("normalize".into(), normalize_result.output),
                ("route".into(), route_result.output),
            ]),
        )
        .expect("selected transform step");
        assert_eq!(high_result.output, json!("HIGH T-42"));

        let output_result = run(
            "output",
            high_result.output.clone(),
            std::collections::BTreeMap::from([("high".into(), high_result.output.clone())]),
            std::collections::BTreeMap::from([("high".into(), high_result.output)]),
        )
        .expect("output step");
        assert_eq!(output_result.output, json!("HIGH T-42"));
        assert!(output_result.selected_handle.is_none());
    }

    #[test]
    fn legacy_step_input_does_not_gain_the_v2_projection_field() {
        let input = crate::modules::workflow::test_support::workflow_run_input()
            .expect("WorkflowRun input");
        let step = input
            .resolved_steps()
            .expect("resolved steps")
            .into_iter()
            .find(|step| step.plan.id == "normalize")
            .expect("normalize step");
        let encoded = serde_json::to_value(WorkflowLocalStepInput {
            runtime_contract_revision: WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION.into(),
            typed_projection_authoritative: false,
            step,
            workflow_input: input.goal_input.clone(),
            effective_input: input.goal_input,
            dependencies: std::collections::BTreeMap::new(),
            steps: std::collections::BTreeMap::new(),
            routed_failure: None,
            composite_region_result: None,
        })
        .expect("legacy step input JSON");
        assert!(encoded.get("typedProjectionAuthoritative").is_none());
    }
}
