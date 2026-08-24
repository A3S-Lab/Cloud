use crate::modules::workflow::domain::WorkflowVariableAggregateConfiguration;
use serde_json::{Map, Value};

pub(super) fn execute(
    configuration: &WorkflowVariableAggregateConfiguration,
    effective_input: &Value,
    typed_projection_authoritative: bool,
) -> Result<Value, String> {
    configuration.validate()?;
    if !typed_projection_authoritative {
        return Err(
            "Workflow Variable Aggregator requires authoritative typed variable projection".into(),
        );
    }
    let inputs = effective_input
        .as_object()
        .ok_or_else(|| "Workflow Variable Aggregator input must be an object".to_owned())?;
    let mut output = Map::new();
    for group in &configuration.groups {
        let mut candidates = group.candidates.iter().collect::<Vec<_>>();
        candidates.sort_by_key(|candidate| candidate.ordinal);
        let selected = candidates
            .into_iter()
            .find_map(|candidate| {
                inputs
                    .get(&candidate.input_port)
                    .filter(|value| !value.is_null())
            })
            .ok_or_else(|| {
                format!(
                    "Workflow Variable Aggregator group {:?} has no available candidate",
                    group.output_port
                )
            })?;
        if !group.output_type.matches_json_value(selected) {
            return Err(format!(
                "Workflow Variable Aggregator group {:?} selected a value that does not match {}",
                group.output_port,
                group.output_type.as_str()
            ));
        }
        if configuration.group_enabled {
            output.insert(
                group.output_port.clone(),
                Value::Object(Map::from_iter([("output".into(), selected.clone())])),
            );
        } else {
            output.insert("output".into(), selected.clone());
        }
    }
    Ok(Value::Object(output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::workflow::domain::{
        WorkflowDataType, WorkflowVariableAggregateCandidate, WorkflowVariableAggregateGroup,
    };
    use serde_json::json;

    fn group(
        output_port: &str,
        output_type: WorkflowDataType,
        candidates: &[(&str, u32)],
    ) -> WorkflowVariableAggregateGroup {
        WorkflowVariableAggregateGroup {
            output_port: output_port.into(),
            output_type,
            candidates: candidates
                .iter()
                .map(|(input_port, ordinal)| WorkflowVariableAggregateCandidate {
                    input_port: (*input_port).into(),
                    ordinal: *ordinal,
                })
                .collect(),
        }
    }

    #[test]
    fn simple_mode_selects_the_first_available_candidate_by_immutable_ordinal() {
        let configuration = WorkflowVariableAggregateConfiguration {
            group_enabled: false,
            groups: vec![group(
                "output",
                WorkflowDataType::String,
                &[("fallback", 1), ("preferred", 0)],
            )],
        };
        assert_eq!(
            execute(
                &configuration,
                &json!({"preferred": "first", "fallback": "second"}),
                true,
            )
            .expect("simple aggregation"),
            json!({"output": "first"})
        );
        assert_eq!(
            execute(&configuration, &json!({"fallback": "second"}), true)
                .expect("fallback aggregation"),
            json!({"output": "second"})
        );
    }

    #[test]
    fn grouped_mode_emits_one_nested_output_per_group() {
        let configuration = WorkflowVariableAggregateConfiguration {
            group_enabled: true,
            groups: vec![
                group(
                    "message",
                    WorkflowDataType::String,
                    &[("left", 0), ("right", 1)],
                ),
                group("score", WorkflowDataType::Number, &[("primary_score", 0)]),
            ],
        };
        assert_eq!(
            execute(
                &configuration,
                &json!({"right": "ready", "primary_score": 7}),
                true,
            )
            .expect("grouped aggregation"),
            json!({
                "message": {"output": "ready"},
                "score": {"output": 7}
            })
        );
    }

    #[test]
    fn aggregation_fails_closed_without_projection_candidate_or_exact_type() {
        let configuration = WorkflowVariableAggregateConfiguration {
            group_enabled: false,
            groups: vec![group(
                "output",
                WorkflowDataType::String,
                &[("candidate", 0)],
            )],
        };
        assert!(
            execute(&configuration, &json!({"candidate": "value"}), false)
                .expect_err("legacy input must be rejected")
                .contains("authoritative typed variable projection")
        );
        assert!(execute(&configuration, &json!({"candidate": null}), true)
            .expect_err("null is unavailable")
            .contains("no available candidate"));
        assert!(execute(&configuration, &json!({"candidate": 7}), true)
            .expect_err("wrong type")
            .contains("does not match string"));
    }
}
