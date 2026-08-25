use crate::modules::workflow::domain::{
    WorkflowDataType, WorkflowListOperatorConfiguration, WorkflowListOperatorExtract,
    WorkflowListOperatorFilterCondition, WorkflowListOperatorFilterOperator,
    WorkflowListOperatorOperand, WorkflowListOperatorOrder, WorkflowListOperatorOrderDirection,
    WORKFLOW_LIST_OPERATOR_MAX_ITEMS,
};
use serde_json::{Map, Number, Value};
use std::cmp::Ordering;

pub(super) fn execute(
    configuration: &WorkflowListOperatorConfiguration,
    effective_input: &Value,
    typed_projection_authoritative: bool,
) -> Result<Value, String> {
    configuration.validate()?;
    if !typed_projection_authoritative {
        return Err(
            "Workflow List Operator requires authoritative typed variable projection".into(),
        );
    }
    let inputs = effective_input
        .as_object()
        .ok_or_else(|| "Workflow List Operator input must be an object".to_owned())?;
    let source = inputs
        .get(&configuration.source_port)
        .ok_or_else(|| "Workflow List Operator source input is missing".to_owned())?
        .as_array()
        .ok_or_else(|| "Workflow List Operator source input must be an array".to_owned())?;
    if source.len() > WORKFLOW_LIST_OPERATOR_MAX_ITEMS as usize {
        return Err(format!(
            "Workflow List Operator source contains too many items; maximum is {WORKFLOW_LIST_OPERATOR_MAX_ITEMS}"
        ));
    }
    for item in source {
        if !configuration.item_type.matches_json_value(item) {
            return Err(format!(
                "Workflow List Operator source item type does not match {}",
                configuration.item_type.as_str()
            ));
        }
    }
    if source.is_empty() {
        return Ok(build_output(Vec::new()));
    }

    let mut result = source.clone();
    let mut conditions = configuration.conditions.iter().collect::<Vec<_>>();
    conditions.sort_by_key(|condition| condition.ordinal);
    for condition in conditions {
        let operand = resolve_operand(condition, inputs)?;
        let mut filtered = Vec::with_capacity(result.len());
        for item in result {
            if matches_condition(condition, &item, operand)? {
                filtered.push(item);
            }
        }
        result = filtered;
    }
    if let Some(extract) = &configuration.extract {
        let index = resolve_extract_index(extract, inputs)?;
        let item = result.get(index - 1).cloned().ok_or_else(|| {
            format!(
                "Workflow List Operator extract index {} exceeds filtered item count {}",
                index,
                result.len()
            )
        })?;
        result = vec![item];
    }
    if let Some(order) = &configuration.order {
        result = order_items(result, order)?;
    }
    if let Some(limit) = configuration.limit {
        result.truncate(limit as usize);
    }

    Ok(build_output(result))
}

fn build_output(result: Vec<Value>) -> Value {
    let first = result.first().cloned();
    let last = result.last().cloned();
    let mut output = Map::from_iter([("result".into(), Value::Array(result))]);
    if let Some(first) = first {
        output.insert("first_record".into(), first);
    }
    if let Some(last) = last {
        output.insert("last_record".into(), last);
    }
    Value::Object(output)
}

fn resolve_operand<'a>(
    condition: &'a WorkflowListOperatorFilterCondition,
    inputs: &'a Map<String, Value>,
) -> Result<Option<&'a Value>, String> {
    match &condition.operand {
        Some(WorkflowListOperatorOperand::Literal(value)) => {
            validate_operand(condition, value)?;
            Ok(Some(value))
        }
        Some(WorkflowListOperatorOperand::InputPort {
            input_port,
            value_type,
        }) => {
            let value = inputs.get(input_port).ok_or_else(|| {
                format!("Workflow List Operator operand input {input_port:?} is missing")
            })?;
            if !value_type.matches_json_value(value) {
                return Err(format!(
                    "Workflow List Operator operand input {:?} does not match {}",
                    input_port,
                    value_type.as_str()
                ));
            }
            validate_operand(condition, value)?;
            Ok(Some(value))
        }
        None => Ok(None),
    }
}

fn validate_operand(
    condition: &WorkflowListOperatorFilterCondition,
    operand: &Value,
) -> Result<(), String> {
    if matches!(
        condition.operator,
        WorkflowListOperatorFilterOperator::In | WorkflowListOperatorFilterOperator::NotIn
    ) && condition.value_type == WorkflowDataType::String
    {
        if operand.is_string()
            || (condition.allows_string_sequence_operand()
                && operand
                    .as_array()
                    .is_some_and(|items| items.iter().all(Value::is_string)))
        {
            return Ok(());
        }
    } else if condition.value_type.matches_json_value(operand) {
        return Ok(());
    }
    Err(format!(
        "Workflow List Operator condition {:?} received an invalid operand",
        condition.id
    ))
}

fn matches_condition(
    condition: &WorkflowListOperatorFilterCondition,
    item: &Value,
    operand: Option<&Value>,
) -> Result<bool, String> {
    let value = match (&condition.key, item) {
        (Some(key), Value::Object(object)) => object.get(key),
        (None, _) => Some(item),
        (Some(_), _) => {
            return Err(format!(
                "Workflow List Operator condition {:?} expected an object item",
                condition.id
            ))
        }
    };
    match condition.value_type {
        WorkflowDataType::String => matches_string_condition(condition, value, operand),
        WorkflowDataType::Number => matches_number_condition(condition, value, operand),
        WorkflowDataType::Boolean => matches_boolean_condition(condition, value, operand),
        WorkflowDataType::Any
        | WorkflowDataType::Object
        | WorkflowDataType::Array
        | WorkflowDataType::Null => Err(format!(
            "Workflow List Operator condition {:?} has a non-scalar value type",
            condition.id
        )),
    }
}

fn matches_string_condition(
    condition: &WorkflowListOperatorFilterCondition,
    value: Option<&Value>,
    operand: Option<&Value>,
) -> Result<bool, String> {
    let value = match value {
        None | Some(Value::Null) => "",
        Some(Value::String(value)) => value,
        Some(_) => {
            return Err(format!(
                "Workflow List Operator condition {:?} object value is not a string",
                condition.id
            ))
        }
    };
    if condition.operator == WorkflowListOperatorFilterOperator::IsEmpty {
        return Ok(value.is_empty());
    }
    if condition.operator == WorkflowListOperatorFilterOperator::IsNotEmpty {
        return Ok(!value.is_empty());
    }
    let operand = operand.ok_or_else(|| {
        format!(
            "Workflow List Operator condition {:?} lost its operand",
            condition.id
        )
    })?;
    let matched = match condition.operator {
        WorkflowListOperatorFilterOperator::Contains => value.contains(string_operand(operand)?),
        WorkflowListOperatorFilterOperator::StartsWith => {
            value.starts_with(string_operand(operand)?)
        }
        WorkflowListOperatorFilterOperator::EndsWith => value.ends_with(string_operand(operand)?),
        WorkflowListOperatorFilterOperator::Equals => value == string_operand(operand)?,
        WorkflowListOperatorFilterOperator::In => string_in_operand(value, operand)?,
        WorkflowListOperatorFilterOperator::NotContains => {
            !value.contains(string_operand(operand)?)
        }
        WorkflowListOperatorFilterOperator::NotEquals => value != string_operand(operand)?,
        WorkflowListOperatorFilterOperator::NotIn => !string_in_operand(value, operand)?,
        WorkflowListOperatorFilterOperator::IsEmpty
        | WorkflowListOperatorFilterOperator::IsNotEmpty
        | WorkflowListOperatorFilterOperator::LessThan
        | WorkflowListOperatorFilterOperator::LessThanOrEqual
        | WorkflowListOperatorFilterOperator::GreaterThan
        | WorkflowListOperatorFilterOperator::GreaterThanOrEqual => {
            return Err(format!(
                "Workflow List Operator condition {:?} has an invalid string operator",
                condition.id
            ))
        }
    };
    Ok(matched)
}

fn string_operand(value: &Value) -> Result<&str, String> {
    value
        .as_str()
        .ok_or_else(|| "Workflow List Operator string operand is invalid".to_owned())
}

fn string_in_operand(value: &str, operand: &Value) -> Result<bool, String> {
    if let Some(container) = operand.as_str() {
        return Ok(container.contains(value));
    }
    let values = operand
        .as_array()
        .ok_or_else(|| "Workflow List Operator membership operand is invalid".to_owned())?;
    Ok(values
        .iter()
        .any(|candidate| candidate.as_str() == Some(value)))
}

fn matches_number_condition(
    condition: &WorkflowListOperatorFilterCondition,
    value: Option<&Value>,
    operand: Option<&Value>,
) -> Result<bool, String> {
    let value = value.and_then(Value::as_number).ok_or_else(|| {
        format!(
            "Workflow List Operator condition {:?} object value is not a number",
            condition.id
        )
    })?;
    let operand = operand.and_then(Value::as_number).ok_or_else(|| {
        format!(
            "Workflow List Operator condition {:?} operand is not a number",
            condition.id
        )
    })?;
    let ordering = compare_numbers(value, operand);
    match condition.operator {
        WorkflowListOperatorFilterOperator::Equals => Ok(ordering == Ordering::Equal),
        WorkflowListOperatorFilterOperator::NotEquals => Ok(ordering != Ordering::Equal),
        WorkflowListOperatorFilterOperator::LessThan => Ok(ordering == Ordering::Less),
        WorkflowListOperatorFilterOperator::LessThanOrEqual => Ok(ordering != Ordering::Greater),
        WorkflowListOperatorFilterOperator::GreaterThan => Ok(ordering == Ordering::Greater),
        WorkflowListOperatorFilterOperator::GreaterThanOrEqual => Ok(ordering != Ordering::Less),
        _ => Err(format!(
            "Workflow List Operator condition {:?} has an invalid number operator",
            condition.id
        )),
    }
}

fn matches_boolean_condition(
    condition: &WorkflowListOperatorFilterCondition,
    value: Option<&Value>,
    operand: Option<&Value>,
) -> Result<bool, String> {
    let value = value.and_then(Value::as_bool).ok_or_else(|| {
        format!(
            "Workflow List Operator condition {:?} object value is not a boolean",
            condition.id
        )
    })?;
    let operand = operand.and_then(Value::as_bool).ok_or_else(|| {
        format!(
            "Workflow List Operator condition {:?} operand is not a boolean",
            condition.id
        )
    })?;
    match condition.operator {
        WorkflowListOperatorFilterOperator::Equals => Ok(value == operand),
        WorkflowListOperatorFilterOperator::NotEquals => Ok(value != operand),
        _ => Err(format!(
            "Workflow List Operator condition {:?} has an invalid boolean operator",
            condition.id
        )),
    }
}

fn resolve_extract_index(
    extract: &WorkflowListOperatorExtract,
    inputs: &Map<String, Value>,
) -> Result<usize, String> {
    let index = match extract {
        WorkflowListOperatorExtract::Literal { index } => u64::from(*index),
        WorkflowListOperatorExtract::InputPort { input_port } => {
            let value = inputs.get(input_port).ok_or_else(|| {
                format!("Workflow List Operator extract input {input_port:?} is missing")
            })?;
            positive_json_integer(value).ok_or_else(|| {
                format!(
                    "Workflow List Operator extract input {input_port:?} must be a positive integer"
                )
            })?
        }
    };
    if index == 0 || index > u64::from(WORKFLOW_LIST_OPERATOR_MAX_ITEMS) {
        return Err(format!(
            "Workflow List Operator extract index must be between 1 and {WORKFLOW_LIST_OPERATOR_MAX_ITEMS}"
        ));
    }
    usize::try_from(index).map_err(|_| "Workflow List Operator extract index exceeds usize".into())
}

fn positive_json_integer(value: &Value) -> Option<u64> {
    if let Some(value) = value.as_u64() {
        return Some(value);
    }
    let value = value.as_f64()?;
    if value.is_finite() && value > 0.0 && value.fract() == 0.0 && value <= u64::MAX as f64 {
        Some(value as u64)
    } else {
        None
    }
}

#[derive(Debug, Clone)]
enum OrderKey {
    String(String),
    Number(Number),
    Boolean(bool),
}

fn order_items(items: Vec<Value>, order: &WorkflowListOperatorOrder) -> Result<Vec<Value>, String> {
    let mut decorated = items
        .into_iter()
        .map(|item| {
            let key = order_key(&item, order)?;
            Ok((item, key))
        })
        .collect::<Result<Vec<_>, String>>()?;
    decorated.sort_by(|left, right| {
        let ordering = compare_order_keys(&left.1, &right.1);
        if order.direction == WorkflowListOperatorOrderDirection::Desc {
            ordering.reverse()
        } else {
            ordering
        }
    });
    Ok(decorated.into_iter().map(|(item, _)| item).collect())
}

fn order_key(item: &Value, order: &WorkflowListOperatorOrder) -> Result<OrderKey, String> {
    let value = match (&order.key, item) {
        (Some(key), Value::Object(object)) => object.get(key),
        (None, _) => Some(item),
        (Some(_), _) => None,
    };
    match order.value_type {
        WorkflowDataType::String => match value {
            None | Some(Value::Null) => Ok(OrderKey::String(String::new())),
            Some(Value::String(value)) => Ok(OrderKey::String(value.clone())),
            Some(_) => Err("Workflow List Operator order value is not a string".into()),
        },
        WorkflowDataType::Number => value
            .and_then(Value::as_number)
            .cloned()
            .map(OrderKey::Number)
            .ok_or_else(|| "Workflow List Operator order value is not a number".to_owned()),
        WorkflowDataType::Boolean => value
            .and_then(Value::as_bool)
            .map(OrderKey::Boolean)
            .ok_or_else(|| "Workflow List Operator order value is not a boolean".to_owned()),
        WorkflowDataType::Any
        | WorkflowDataType::Object
        | WorkflowDataType::Array
        | WorkflowDataType::Null => {
            Err("Workflow List Operator order value type is not scalar".into())
        }
    }
}

fn compare_order_keys(left: &OrderKey, right: &OrderKey) -> Ordering {
    match (left, right) {
        (OrderKey::String(left), OrderKey::String(right)) => left.cmp(right),
        (OrderKey::Number(left), OrderKey::Number(right)) => compare_numbers(left, right),
        (OrderKey::Boolean(left), OrderKey::Boolean(right)) => left.cmp(right),
        _ => Ordering::Equal,
    }
}

fn compare_numbers(left: &Number, right: &Number) -> Ordering {
    if let (Some(left), Some(right)) = (left.as_i64(), right.as_i64()) {
        return left.cmp(&right);
    }
    if let (Some(left), Some(right)) = (left.as_u64(), right.as_u64()) {
        return left.cmp(&right);
    }
    if let (Some(left), Some(right)) = (left.as_i64(), right.as_u64()) {
        return if left < 0 {
            Ordering::Less
        } else {
            (left as u64).cmp(&right)
        };
    }
    if let (Some(left), Some(right)) = (left.as_u64(), right.as_i64()) {
        return if right < 0 {
            Ordering::Greater
        } else {
            left.cmp(&(right as u64))
        };
    }
    if let (Some(left), Some(right)) = (left.as_i64(), right.as_f64()) {
        return compare_i64_to_f64(left, right);
    }
    if let (Some(left), Some(right)) = (left.as_u64(), right.as_f64()) {
        return compare_u64_to_f64(left, right);
    }
    if let (Some(left), Some(right)) = (left.as_f64(), right.as_i64()) {
        return compare_i64_to_f64(right, left).reverse();
    }
    if let (Some(left), Some(right)) = (left.as_f64(), right.as_u64()) {
        return compare_u64_to_f64(right, left).reverse();
    }
    match (left.as_f64(), right.as_f64()) {
        (Some(left), Some(right)) => left.partial_cmp(&right).unwrap_or(Ordering::Equal),
        _ => left.to_string().cmp(&right.to_string()),
    }
}

fn compare_i64_to_f64(integer: i64, float: f64) -> Ordering {
    const I64_UPPER_EXCLUSIVE: f64 = 9_223_372_036_854_775_808.0;

    if float < i64::MIN as f64 {
        return Ordering::Greater;
    }
    if float >= I64_UPPER_EXCLUSIVE {
        return Ordering::Less;
    }
    let truncated = float.trunc() as i64;
    match integer.cmp(&truncated) {
        Ordering::Equal if float.fract() > 0.0 => Ordering::Less,
        Ordering::Equal if float.fract() < 0.0 => Ordering::Greater,
        ordering => ordering,
    }
}

fn compare_u64_to_f64(integer: u64, float: f64) -> Ordering {
    const U64_UPPER_EXCLUSIVE: f64 = 18_446_744_073_709_551_616.0;

    if float < 0.0 {
        return Ordering::Greater;
    }
    if float >= U64_UPPER_EXCLUSIVE {
        return Ordering::Less;
    }
    let truncated = float.trunc() as u64;
    match integer.cmp(&truncated) {
        Ordering::Equal if float.fract() > 0.0 => Ordering::Less,
        ordering => ordering,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn literal_condition(
        id: &str,
        ordinal: u32,
        key: Option<&str>,
        value_type: WorkflowDataType,
        operator: WorkflowListOperatorFilterOperator,
        value: serde_json::Value,
    ) -> WorkflowListOperatorFilterCondition {
        WorkflowListOperatorFilterCondition {
            id: id.into(),
            ordinal,
            key: key.map(str::to_owned),
            value_type,
            operator,
            operand: Some(WorkflowListOperatorOperand::Literal(value)),
        }
    }

    #[test]
    fn filters_orders_and_limits_string_arrays_with_stable_outputs() {
        let configuration = WorkflowListOperatorConfiguration {
            source_port: "items".into(),
            item_type: WorkflowDataType::String,
            conditions: vec![literal_condition(
                "contains_a",
                0,
                None,
                WorkflowDataType::String,
                WorkflowListOperatorFilterOperator::Contains,
                json!("a"),
            )],
            extract: None,
            order: Some(WorkflowListOperatorOrder {
                key: None,
                value_type: WorkflowDataType::String,
                direction: WorkflowListOperatorOrderDirection::Desc,
            }),
            limit: Some(2),
        };

        assert_eq!(
            super::execute(
                &configuration,
                &json!({"items": ["beta", "gamma", "alpha", "echo"]}),
                true,
            )
            .expect("string List Operator"),
            json!({
                "result": ["gamma", "beta"],
                "first_record": "gamma",
                "last_record": "beta"
            })
        );
    }

    #[test]
    fn object_filters_use_literal_and_typed_dynamic_operands() {
        let configuration = WorkflowListOperatorConfiguration {
            source_port: "items".into(),
            item_type: WorkflowDataType::Object,
            conditions: vec![
                literal_condition(
                    "supported_type",
                    0,
                    Some("type"),
                    WorkflowDataType::String,
                    WorkflowListOperatorFilterOperator::In,
                    json!(["document", "image"]),
                ),
                WorkflowListOperatorFilterCondition {
                    id: "minimum_size".into(),
                    ordinal: 1,
                    key: Some("size".into()),
                    value_type: WorkflowDataType::Number,
                    operator: WorkflowListOperatorFilterOperator::GreaterThanOrEqual,
                    operand: Some(WorkflowListOperatorOperand::InputPort {
                        input_port: "minimum_size".into(),
                        value_type: WorkflowDataType::Number,
                    }),
                },
            ],
            extract: None,
            order: Some(WorkflowListOperatorOrder {
                key: Some("size".into()),
                value_type: WorkflowDataType::Number,
                direction: WorkflowListOperatorOrderDirection::Desc,
            }),
            limit: Some(2),
        };
        let input = json!({
            "items": [
                {"name": "small", "type": "image", "size": 2},
                {"name": "document", "type": "document", "size": 12},
                {"name": "largest", "type": "image", "size": 20},
                {"name": "ignored", "type": "audio", "size": 99}
            ],
            "minimum_size": 10
        });

        assert_eq!(
            super::execute(&configuration, &input, true).expect("object List Operator"),
            json!({
                "result": [
                    {"name": "largest", "type": "image", "size": 20},
                    {"name": "document", "type": "document", "size": 12}
                ],
                "first_record": {"name": "largest", "type": "image", "size": 20},
                "last_record": {"name": "document", "type": "document", "size": 12}
            })
        );
    }

    #[test]
    fn extraction_is_one_based_and_runs_before_order_and_limit() {
        let configuration = WorkflowListOperatorConfiguration {
            source_port: "items".into(),
            item_type: WorkflowDataType::Number,
            conditions: Vec::new(),
            extract: Some(WorkflowListOperatorExtract::InputPort {
                input_port: "serial".into(),
            }),
            order: Some(WorkflowListOperatorOrder {
                key: None,
                value_type: WorkflowDataType::Number,
                direction: WorkflowListOperatorOrderDirection::Asc,
            }),
            limit: Some(1),
        };

        assert_eq!(
            super::execute(
                &configuration,
                &json!({"items": [30, 10, 20], "serial": 2}),
                true,
            )
            .expect("dynamic extraction"),
            json!({"result": [10], "first_record": 10, "last_record": 10})
        );
    }

    #[test]
    fn empty_arrays_succeed_without_resolving_operations_or_optional_records() {
        let configuration = WorkflowListOperatorConfiguration {
            source_port: "items".into(),
            item_type: WorkflowDataType::String,
            conditions: vec![WorkflowListOperatorFilterCondition {
                id: "filter".into(),
                ordinal: 0,
                key: None,
                value_type: WorkflowDataType::String,
                operator: WorkflowListOperatorFilterOperator::Equals,
                operand: Some(WorkflowListOperatorOperand::InputPort {
                    input_port: "missing_filter".into(),
                    value_type: WorkflowDataType::String,
                }),
            }],
            extract: Some(WorkflowListOperatorExtract::InputPort {
                input_port: "missing_serial".into(),
            }),
            order: Some(WorkflowListOperatorOrder {
                key: None,
                value_type: WorkflowDataType::String,
                direction: WorkflowListOperatorOrderDirection::Asc,
            }),
            limit: Some(1),
        };
        assert_eq!(
            super::execute(&configuration, &json!({"items": []}), true)
                .expect("empty List Operator"),
            json!({"result": []})
        );
    }

    #[test]
    fn string_filter_family_is_case_sensitive_and_closed() {
        let run = |operator, operand: Option<serde_json::Value>, items: serde_json::Value| {
            let condition = WorkflowListOperatorFilterCondition {
                id: "condition".into(),
                ordinal: 0,
                key: None,
                value_type: WorkflowDataType::String,
                operator,
                operand: operand.map(WorkflowListOperatorOperand::Literal),
            };
            super::execute(
                &WorkflowListOperatorConfiguration {
                    source_port: "items".into(),
                    item_type: WorkflowDataType::String,
                    conditions: vec![condition],
                    extract: None,
                    order: None,
                    limit: None,
                },
                &json!({"items": items}),
                true,
            )
            .expect("string filter")["result"]
                .clone()
        };

        assert_eq!(
            run(
                WorkflowListOperatorFilterOperator::StartsWith,
                Some(json!("al")),
                json!(["alpha", "Alpha", "beta"]),
            ),
            json!(["alpha"])
        );
        assert_eq!(
            run(
                WorkflowListOperatorFilterOperator::EndsWith,
                Some(json!("ta")),
                json!(["alpha", "beta"]),
            ),
            json!(["beta"])
        );
        assert_eq!(
            run(
                WorkflowListOperatorFilterOperator::Equals,
                Some(json!("alpha")),
                json!(["alpha", "beta"]),
            ),
            json!(["alpha"])
        );
        assert_eq!(
            run(
                WorkflowListOperatorFilterOperator::NotContains,
                Some(json!("a")),
                json!(["alpha", "echo"]),
            ),
            json!(["echo"])
        );
        assert_eq!(
            run(
                WorkflowListOperatorFilterOperator::NotEquals,
                Some(json!("alpha")),
                json!(["alpha", "beta"]),
            ),
            json!(["beta"])
        );
        assert_eq!(
            run(
                WorkflowListOperatorFilterOperator::In,
                Some(json!("alphabet")),
                json!(["alpha", "gamma"]),
            ),
            json!(["alpha"])
        );
        assert_eq!(
            run(
                WorkflowListOperatorFilterOperator::NotIn,
                Some(json!("alphabet")),
                json!(["alpha", "gamma"]),
            ),
            json!(["gamma"])
        );
        assert_eq!(
            run(
                WorkflowListOperatorFilterOperator::IsEmpty,
                None,
                json!(["", "value"]),
            ),
            json!([""])
        );
        assert_eq!(
            run(
                WorkflowListOperatorFilterOperator::IsNotEmpty,
                None,
                json!(["", "value"]),
            ),
            json!(["value"])
        );
    }

    #[test]
    fn number_and_boolean_comparisons_cover_every_admitted_operator() {
        let number_result = |operator, operand| {
            super::execute(
                &WorkflowListOperatorConfiguration {
                    source_port: "items".into(),
                    item_type: WorkflowDataType::Number,
                    conditions: vec![literal_condition(
                        "condition",
                        0,
                        None,
                        WorkflowDataType::Number,
                        operator,
                        json!(operand),
                    )],
                    extract: None,
                    order: None,
                    limit: None,
                },
                &json!({"items": [1, 2, 3]}),
                true,
            )
            .expect("number filter")["result"]
                .clone()
        };
        assert_eq!(
            number_result(WorkflowListOperatorFilterOperator::Equals, 2),
            json!([2])
        );
        assert_eq!(
            number_result(WorkflowListOperatorFilterOperator::NotEquals, 2),
            json!([1, 3])
        );
        assert_eq!(
            number_result(WorkflowListOperatorFilterOperator::LessThan, 2),
            json!([1])
        );
        assert_eq!(
            number_result(WorkflowListOperatorFilterOperator::LessThanOrEqual, 2),
            json!([1, 2])
        );
        assert_eq!(
            number_result(WorkflowListOperatorFilterOperator::GreaterThan, 2),
            json!([3])
        );
        assert_eq!(
            number_result(WorkflowListOperatorFilterOperator::GreaterThanOrEqual, 2,),
            json!([2, 3])
        );

        for (operator, expected) in [
            (WorkflowListOperatorFilterOperator::Equals, json!([true])),
            (
                WorkflowListOperatorFilterOperator::NotEquals,
                json!([false]),
            ),
        ] {
            let configuration = WorkflowListOperatorConfiguration {
                source_port: "items".into(),
                item_type: WorkflowDataType::Boolean,
                conditions: vec![literal_condition(
                    "condition",
                    0,
                    None,
                    WorkflowDataType::Boolean,
                    operator,
                    json!(true),
                )],
                extract: None,
                order: None,
                limit: None,
            };
            assert_eq!(
                super::execute(&configuration, &json!({"items": [false, true]}), true)
                    .expect("boolean filter")["result"],
                expected
            );
        }
    }

    #[test]
    fn negative_integer_and_decimal_comparisons_preserve_numeric_order() {
        let filtered = super::execute(
            &WorkflowListOperatorConfiguration {
                source_port: "items".into(),
                item_type: WorkflowDataType::Number,
                conditions: vec![literal_condition(
                    "condition",
                    0,
                    None,
                    WorkflowDataType::Number,
                    WorkflowListOperatorFilterOperator::GreaterThan,
                    json!(-1.5),
                )],
                extract: None,
                order: Some(WorkflowListOperatorOrder {
                    key: None,
                    value_type: WorkflowDataType::Number,
                    direction: WorkflowListOperatorOrderDirection::Asc,
                }),
                limit: None,
            },
            &json!({"items": [-1, -2.0, -1.25, 0]}),
            true,
        )
        .expect("mixed negative numeric comparison");

        assert_eq!(
            filtered,
            json!({
                "result": [-1.25, -1, 0],
                "first_record": -1.25,
                "last_record": 0
            })
        );
    }

    #[test]
    fn mixed_numbers_beyond_f64_safe_integer_range_preserve_numeric_order() {
        let filtered = super::execute(
            &WorkflowListOperatorConfiguration {
                source_port: "items".into(),
                item_type: WorkflowDataType::Number,
                conditions: vec![literal_condition(
                    "condition",
                    0,
                    None,
                    WorkflowDataType::Number,
                    WorkflowListOperatorFilterOperator::GreaterThan,
                    json!(9_007_199_254_740_992.0),
                )],
                extract: None,
                order: Some(WorkflowListOperatorOrder {
                    key: None,
                    value_type: WorkflowDataType::Number,
                    direction: WorkflowListOperatorOrderDirection::Asc,
                }),
                limit: None,
            },
            &json!({
                "items": [
                    9_007_199_254_740_994_u64,
                    9_007_199_254_740_993_u64,
                    9_007_199_254_740_992.0
                ]
            }),
            true,
        )
        .expect("large mixed numeric comparison");

        assert_eq!(
            filtered,
            json!({
                "result": [
                    9_007_199_254_740_993_u64,
                    9_007_199_254_740_994_u64
                ],
                "first_record": 9_007_199_254_740_993_u64,
                "last_record": 9_007_199_254_740_994_u64
            })
        );
    }

    #[test]
    fn execution_fails_closed_on_untrusted_or_invalid_runtime_values() {
        let configuration = WorkflowListOperatorConfiguration {
            source_port: "items".into(),
            item_type: WorkflowDataType::Number,
            conditions: Vec::new(),
            extract: Some(WorkflowListOperatorExtract::Literal { index: 2 }),
            order: None,
            limit: None,
        };
        assert!(
            super::execute(&configuration, &json!({"items": [1, 2]}), false)
                .expect_err("legacy projection")
                .contains("authoritative typed variable projection")
        );
        assert!(
            super::execute(&configuration, &json!({"items": [1, "2"]}), true)
                .expect_err("mixed item types")
                .contains("item type")
        );
        assert!(super::execute(&configuration, &json!({"items": [1]}), true)
            .expect_err("out-of-range extraction")
            .contains("extract index"));

        let mut dynamic = configuration.clone();
        dynamic.extract = Some(WorkflowListOperatorExtract::InputPort {
            input_port: "serial".into(),
        });
        assert!(
            super::execute(&dynamic, &json!({"items": [1, 2], "serial": 1.5}), true,)
                .expect_err("fractional extraction")
                .contains("positive integer")
        );

        let too_many = vec![json!(0); WORKFLOW_LIST_OPERATOR_MAX_ITEMS as usize + 1];
        assert!(
            super::execute(&configuration, &json!({"items": too_many}), true)
                .expect_err("unbounded input")
                .contains("too many items")
        );
    }
}
