use super::types::{
    WorkflowLocalStepInput, WorkflowLocalStepResult, WORKFLOW_LOCAL_STEP_RESULT_SCHEMA,
};
use crate::modules::workflow::domain::WorkflowStepKind;
use a3s_flow::FlowError;
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) fn execute(input: WorkflowLocalStepInput) -> a3s_flow::Result<WorkflowLocalStepResult> {
    input.validate().map_err(FlowError::Runtime)?;
    let step_input = dependency_value(&input.dependencies, &input.workflow_input);
    input
        .input_schema
        .validate_value(&step_input)
        .map_err(|error| FlowError::Runtime(format!("Workflow step input is invalid: {error}")))?;
    let (output, route) = match input.step.kind {
        WorkflowStepKind::Input => (input.workflow_input.clone(), None),
        WorkflowStepKind::Transform => (
            render_template(
                input.configuration.template.as_deref().ok_or_else(|| {
                    FlowError::Runtime("Workflow transform template is missing".into())
                })?,
                &input.workflow_input,
                &input.dependencies,
            )?,
            None,
        ),
        WorkflowStepKind::Branch => {
            let selector =
                input.configuration.selector.as_deref().ok_or_else(|| {
                    FlowError::Runtime("Workflow branch selector is missing".into())
                })?;
            let selected = lookup_token(selector, &input.workflow_input, &input.dependencies)?;
            let selected = scalar_text(selected)?;
            let route = input
                .configuration
                .routes
                .iter()
                .find(|route| route.equals == selected)
                .map(|route| route.handle.clone())
                .or_else(|| input.configuration.default_handle.clone())
                .ok_or_else(|| {
                    FlowError::Runtime("Workflow branch default route is missing".into())
                })?;
            (step_input, Some(route))
        }
        WorkflowStepKind::Output => (
            match input.configuration.template.as_deref() {
                Some(template) => {
                    render_template(template, &input.workflow_input, &input.dependencies)?
                }
                None => step_input,
            },
            None,
        ),
        kind => {
            return Err(FlowError::Runtime(format!(
                "Workflow step kind {} is not a local semantic step",
                kind.as_str()
            )))
        }
    };
    let result = WorkflowLocalStepResult {
        schema: WORKFLOW_LOCAL_STEP_RESULT_SCHEMA.into(),
        step_id: input.step.id.clone(),
        kind: input.step.kind,
        output,
        route,
    };
    result
        .validate(&input.step, &input.output_schema)
        .map_err(|error| FlowError::Runtime(format!("Workflow step output is invalid: {error}")))?;
    Ok(result)
}

fn dependency_value(dependencies: &BTreeMap<String, Value>, workflow_input: &Value) -> Value {
    match dependencies.len() {
        0 => workflow_input.clone(),
        1 => dependencies.values().next().cloned().unwrap_or(Value::Null),
        _ => Value::Object(
            dependencies
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        ),
    }
}

fn render_template(
    source: &str,
    workflow_input: &Value,
    dependencies: &BTreeMap<String, Value>,
) -> a3s_flow::Result<Value> {
    let trimmed = source.trim();
    if let Some(token) = whole_token(trimmed) {
        return lookup_token(token, workflow_input, dependencies).cloned();
    }
    let mut output = String::new();
    let mut remainder = source;
    while let Some(start) = remainder.find("{{") {
        output.push_str(&remainder[..start]);
        let token_source = &remainder[start + 2..];
        let end = token_source.find("}}").ok_or_else(|| {
            FlowError::Runtime("Workflow template contains an unclosed token".into())
        })?;
        let token = token_source[..end].trim();
        output.push_str(&scalar_text(lookup_token(
            token,
            workflow_input,
            dependencies,
        )?)?);
        remainder = &token_source[end + 2..];
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
    dependencies: &'a BTreeMap<String, Value>,
) -> a3s_flow::Result<&'a Value> {
    if token == "input" {
        return Ok(workflow_input);
    }
    if let Some(path) = token.strip_prefix("input.") {
        return lookup_path(workflow_input, path, token);
    }
    if token == "steps" {
        return Err(FlowError::Runtime(
            "Workflow template token steps must name a dependency".into(),
        ));
    }
    if let Some(path) = token.strip_prefix("steps.") {
        let mut segments = path.split('.');
        let dependency_id = segments.next().unwrap_or_default();
        let value = dependencies.get(dependency_id).ok_or_else(|| {
            FlowError::Runtime(format!(
                "Workflow dependency {dependency_id:?} is unavailable"
            ))
        })?;
        let remainder = segments.collect::<Vec<_>>().join(".");
        return if remainder.is_empty() {
            Ok(value)
        } else {
            lookup_path(value, &remainder, token)
        };
    }
    Err(FlowError::Runtime(format!(
        "unsupported Workflow template token {token:?}"
    )))
}

fn lookup_path<'a>(value: &'a Value, path: &str, token: &str) -> a3s_flow::Result<&'a Value> {
    let mut current = value;
    for segment in path.split('.') {
        if segment.is_empty() {
            return Err(FlowError::Runtime(format!(
                "Workflow template token {token:?} contains an empty path segment"
            )));
        }
        current = match current {
            Value::Object(values) => values.get(segment),
            Value::Array(values) => segment
                .parse::<usize>()
                .ok()
                .and_then(|index| values.get(index)),
            _ => None,
        }
        .ok_or_else(|| {
            FlowError::Runtime(format!("Workflow template token {token:?} was not found"))
        })?;
    }
    Ok(current)
}

fn scalar_text(value: &Value) -> a3s_flow::Result<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Null => Ok("null".into()),
        Value::Array(_) | Value::Object(_) => Err(FlowError::Runtime(
            "Workflow interpolation requires a scalar value".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{PlanRevisionId, Sha256Digest, WorkflowRunId};
    use crate::modules::workflow::domain::{
        WorkflowBranchRoute, WorkflowDataField, WorkflowDataSchema, WorkflowDataType,
        WorkflowPlanStep, WorkflowStepConfiguration,
    };
    use serde_json::json;

    fn digest(value: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", value.to_string().repeat(64))).expect("digest")
    }

    fn input(
        kind: WorkflowStepKind,
        configuration: WorkflowStepConfiguration,
    ) -> WorkflowLocalStepInput {
        WorkflowLocalStepInput {
            schema: super::super::types::WORKFLOW_LOCAL_STEP_INPUT_SCHEMA.into(),
            workflow_run_id: WorkflowRunId::new(),
            plan_revision_id: PlanRevisionId::new(),
            plan_digest: digest('a').to_string(),
            step: WorkflowPlanStep {
                id: "step".into(),
                kind,
                configuration_digest: digest('b'),
                input_schema_digest: digest('c'),
                output_schema_digest: digest('d'),
                policy_digest: None,
                capability: None,
            },
            configuration,
            input_schema: WorkflowDataSchema {
                value_type: WorkflowDataType::Any,
                fields: vec![],
            },
            output_schema: WorkflowDataSchema {
                value_type: WorkflowDataType::Any,
                fields: vec![],
            },
            workflow_input: json!({"kind": "fix", "name": "Ada"}),
            dependencies: BTreeMap::new(),
        }
    }

    #[test]
    fn templates_preserve_whole_token_types_and_interpolate_scalars() {
        let mut configuration = WorkflowStepConfiguration::empty(WorkflowStepKind::Transform);
        configuration.template = Some("{{input}}".into());
        assert_eq!(
            execute(input(WorkflowStepKind::Transform, configuration))
                .expect("whole token")
                .output,
            json!({"kind": "fix", "name": "Ada"})
        );

        let mut configuration = WorkflowStepConfiguration::empty(WorkflowStepKind::Transform);
        configuration.template = Some("Hello {{input.name}}".into());
        assert_eq!(
            execute(input(WorkflowStepKind::Transform, configuration))
                .expect("interpolation")
                .output,
            json!("Hello Ada")
        );
    }

    #[test]
    fn branch_records_one_explicit_route_and_preserves_input() {
        let mut configuration = WorkflowStepConfiguration::empty(WorkflowStepKind::Branch);
        configuration.selector = Some("input.kind".into());
        configuration.default_handle = Some("other".into());
        configuration.routes = vec![WorkflowBranchRoute {
            handle: "fix".into(),
            equals: "fix".into(),
        }];
        let result = execute(input(WorkflowStepKind::Branch, configuration)).expect("branch");
        assert_eq!(result.route.as_deref(), Some("fix"));
        assert_eq!(result.output, json!({"kind": "fix", "name": "Ada"}));
    }

    #[test]
    fn data_schema_rejects_missing_and_wrong_typed_fields() {
        let schema = WorkflowDataSchema {
            value_type: WorkflowDataType::Object,
            fields: vec![WorkflowDataField {
                name: "name".into(),
                value_type: WorkflowDataType::String,
                required: true,
            }],
        };
        assert!(schema.validate_value(&json!({})).is_err());
        assert!(schema.validate_value(&json!({"name": 42})).is_err());
        schema
            .validate_value(&json!({"name": "Ada", "extra": true}))
            .expect("typed object");
    }
}
