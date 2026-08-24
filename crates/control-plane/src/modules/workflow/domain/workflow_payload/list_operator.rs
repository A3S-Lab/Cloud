use super::codec::{
    non_negative_integer, optional_number, optional_string, positive_integer, required_label,
    required_number, required_string,
};
use super::{validate_identifier, WorkflowDataType, WorkflowStepKind};
use crate::modules::shared_kernel::domain::canonical_json_bounded;
use a3s_acl::builder::{number, string, BlockBuilder};
use a3s_acl::{AttributeSchema, Block, BlockSchema, Cardinality, Schema, ValueSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const WORKFLOW_LIST_OPERATOR_CONFIGURATION_SCHEMA: &str =
    "cloud.workflow.configuration.list-operator.v1";
pub const WORKFLOW_LIST_OPERATOR_MAX_CONDITIONS: usize = 64;
pub const WORKFLOW_LIST_OPERATOR_MAX_ITEMS: u32 = 10_000;
const WORKFLOW_LIST_OPERATOR_MAX_LITERAL_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowListOperatorFilterOperator {
    Contains,
    StartsWith,
    EndsWith,
    Equals,
    In,
    IsEmpty,
    NotContains,
    NotEquals,
    NotIn,
    IsNotEmpty,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
}

impl WorkflowListOperatorFilterOperator {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::StartsWith => "starts_with",
            Self::EndsWith => "ends_with",
            Self::Equals => "equals",
            Self::In => "in",
            Self::IsEmpty => "is_empty",
            Self::NotContains => "not_contains",
            Self::NotEquals => "not_equals",
            Self::NotIn => "not_in",
            Self::IsNotEmpty => "is_not_empty",
            Self::LessThan => "less_than",
            Self::LessThanOrEqual => "less_than_or_equal",
            Self::GreaterThan => "greater_than",
            Self::GreaterThanOrEqual => "greater_than_or_equal",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "contains" => Ok(Self::Contains),
            "starts_with" => Ok(Self::StartsWith),
            "ends_with" => Ok(Self::EndsWith),
            "equals" => Ok(Self::Equals),
            "in" => Ok(Self::In),
            "is_empty" => Ok(Self::IsEmpty),
            "not_contains" => Ok(Self::NotContains),
            "not_equals" => Ok(Self::NotEquals),
            "not_in" => Ok(Self::NotIn),
            "is_not_empty" => Ok(Self::IsNotEmpty),
            "less_than" => Ok(Self::LessThan),
            "less_than_or_equal" => Ok(Self::LessThanOrEqual),
            "greater_than" => Ok(Self::GreaterThan),
            "greater_than_or_equal" => Ok(Self::GreaterThanOrEqual),
            _ => Err(format!(
                "unsupported Workflow List Operator filter {value:?}"
            )),
        }
    }

    pub const fn requires_operand(self) -> bool {
        !matches!(self, Self::IsEmpty | Self::IsNotEmpty)
    }

    fn supports(self, value_type: &WorkflowDataType) -> bool {
        match value_type {
            WorkflowDataType::String => matches!(
                self,
                Self::Contains
                    | Self::StartsWith
                    | Self::EndsWith
                    | Self::Equals
                    | Self::In
                    | Self::IsEmpty
                    | Self::NotContains
                    | Self::NotEquals
                    | Self::NotIn
                    | Self::IsNotEmpty
            ),
            WorkflowDataType::Number => matches!(
                self,
                Self::Equals
                    | Self::NotEquals
                    | Self::LessThan
                    | Self::LessThanOrEqual
                    | Self::GreaterThan
                    | Self::GreaterThanOrEqual
            ),
            WorkflowDataType::Boolean => matches!(self, Self::Equals | Self::NotEquals),
            WorkflowDataType::Any
            | WorkflowDataType::Object
            | WorkflowDataType::Array
            | WorkflowDataType::Null => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "configuration", rename_all = "snake_case")]
pub enum WorkflowListOperatorOperand {
    Literal(Value),
    InputPort {
        input_port: String,
        value_type: WorkflowDataType,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowListOperatorFilterCondition {
    pub id: String,
    pub ordinal: u32,
    pub key: Option<String>,
    pub value_type: WorkflowDataType,
    pub operator: WorkflowListOperatorFilterOperator,
    pub operand: Option<WorkflowListOperatorOperand>,
}

impl WorkflowListOperatorFilterCondition {
    fn validate(&self, item_type: &WorkflowDataType) -> Result<(), String> {
        validate_identifier("Workflow List Operator condition", &self.id)?;
        match item_type {
            WorkflowDataType::Object => {
                validate_identifier(
                    "Workflow List Operator object key",
                    self.key.as_deref().ok_or_else(|| {
                        format!(
                            "Workflow List Operator condition {:?} requires an object key",
                            self.id
                        )
                    })?,
                )?;
            }
            _ if self.key.is_some() => {
                return Err(format!(
                    "Workflow List Operator scalar condition {:?} cannot declare an object key",
                    self.id
                ));
            }
            _ if &self.value_type != item_type => {
                return Err(format!(
                    "Workflow List Operator scalar condition {:?} must match its item type",
                    self.id
                ));
            }
            _ => {}
        }
        if !matches!(
            self.value_type,
            WorkflowDataType::String | WorkflowDataType::Number | WorkflowDataType::Boolean
        ) || !self.operator.supports(&self.value_type)
        {
            return Err(format!(
                "Workflow List Operator condition {:?} has an incompatible operator and value type",
                self.id
            ));
        }
        match (self.operator.requires_operand(), &self.operand) {
            (true, Some(operand)) => self.validate_operand(operand),
            (true, None) => Err(format!(
                "Workflow List Operator condition {:?} requires an operand",
                self.id
            )),
            (false, None) => Ok(()),
            (false, Some(_)) => Err(format!(
                "Workflow List Operator condition {:?} cannot declare an operand",
                self.id
            )),
        }
    }

    fn validate_operand(&self, operand: &WorkflowListOperatorOperand) -> Result<(), String> {
        match operand {
            WorkflowListOperatorOperand::Literal(value) => {
                validate_operand_value(self.operator, &self.value_type, value)?;
                canonical_json_bounded(
                    value,
                    WORKFLOW_LIST_OPERATOR_MAX_LITERAL_BYTES,
                    "Workflow List Operator literal operand",
                )?;
                Ok(())
            }
            WorkflowListOperatorOperand::InputPort {
                input_port,
                value_type,
            } => {
                validate_identifier("Workflow List Operator operand input port", input_port)?;
                if operand_type_supported(self.operator, &self.value_type, value_type) {
                    Ok(())
                } else {
                    Err(format!(
                        "Workflow List Operator condition {:?} has an incompatible operand input type",
                        self.id
                    ))
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "configuration", rename_all = "snake_case")]
pub enum WorkflowListOperatorExtract {
    Literal { index: u32 },
    InputPort { input_port: String },
}

impl WorkflowListOperatorExtract {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Literal { index } if *index == 0 || *index > WORKFLOW_LIST_OPERATOR_MAX_ITEMS => {
                Err(format!(
                    "Workflow List Operator extract index must be between 1 and {WORKFLOW_LIST_OPERATOR_MAX_ITEMS}"
                ))
            }
            Self::Literal { .. } => Ok(()),
            Self::InputPort { input_port } => {
                validate_identifier("Workflow List Operator extract input port", input_port)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowListOperatorOrderDirection {
    Asc,
    Desc,
}

impl WorkflowListOperatorOrderDirection {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "asc" => Ok(Self::Asc),
            "desc" => Ok(Self::Desc),
            _ => Err(format!(
                "unsupported Workflow List Operator order direction {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowListOperatorOrder {
    pub key: Option<String>,
    pub value_type: WorkflowDataType,
    pub direction: WorkflowListOperatorOrderDirection,
}

impl WorkflowListOperatorOrder {
    fn validate(&self, item_type: &WorkflowDataType) -> Result<(), String> {
        match item_type {
            WorkflowDataType::Object => validate_identifier(
                "Workflow List Operator order key",
                self.key.as_deref().ok_or_else(|| {
                    "Workflow List Operator object order requires a key".to_owned()
                })?,
            )?,
            _ if self.key.is_some() => {
                return Err("Workflow List Operator scalar order cannot declare a key".into())
            }
            _ if &self.value_type != item_type => {
                return Err("Workflow List Operator scalar order must match its item type".into())
            }
            _ => {}
        }
        if matches!(
            self.value_type,
            WorkflowDataType::String | WorkflowDataType::Number | WorkflowDataType::Boolean
        ) {
            Ok(())
        } else {
            Err("Workflow List Operator order requires a scalar value type".into())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowListOperatorConfiguration {
    pub source_port: String,
    pub item_type: WorkflowDataType,
    pub conditions: Vec<WorkflowListOperatorFilterCondition>,
    pub extract: Option<WorkflowListOperatorExtract>,
    pub order: Option<WorkflowListOperatorOrder>,
    pub limit: Option<u32>,
}

impl WorkflowListOperatorConfiguration {
    pub fn validate(&self) -> Result<(), String> {
        validate_identifier("Workflow List Operator source port", &self.source_port)?;
        if !matches!(
            self.item_type,
            WorkflowDataType::Object
                | WorkflowDataType::String
                | WorkflowDataType::Number
                | WorkflowDataType::Boolean
        ) {
            return Err(
                "Workflow List Operator requires object, string, number, or boolean items".into(),
            );
        }
        if self.conditions.len() > WORKFLOW_LIST_OPERATOR_MAX_CONDITIONS {
            return Err(format!(
                "Workflow List Operator cannot declare more than {WORKFLOW_LIST_OPERATOR_MAX_CONDITIONS} conditions"
            ));
        }
        let mut ids = BTreeSet::new();
        let mut ordinals = BTreeSet::new();
        for condition in &self.conditions {
            condition.validate(&self.item_type)?;
            if !ids.insert(condition.id.as_str()) {
                return Err(format!(
                    "Workflow List Operator contains duplicate condition {:?}",
                    condition.id
                ));
            }
            if !ordinals.insert(condition.ordinal) {
                return Err(format!(
                    "Workflow List Operator contains duplicate condition ordinal {}",
                    condition.ordinal
                ));
            }
        }
        let expected = (0..self.conditions.len())
            .map(|ordinal| {
                u32::try_from(ordinal)
                    .map_err(|_| "Workflow List Operator condition ordinal exceeds u32".to_owned())
            })
            .collect::<Result<BTreeSet<_>, String>>()?;
        if ordinals != expected {
            return Err(
                "Workflow List Operator condition ordinals must be contiguous and zero-based"
                    .into(),
            );
        }
        if let Some(extract) = &self.extract {
            extract.validate()?;
        }
        if let Some(order) = &self.order {
            order.validate(&self.item_type)?;
        }
        if self
            .limit
            .is_some_and(|limit| limit == 0 || limit > WORKFLOW_LIST_OPERATOR_MAX_ITEMS)
        {
            return Err(format!(
                "Workflow List Operator limit must be between 1 and {WORKFLOW_LIST_OPERATOR_MAX_ITEMS}"
            ));
        }
        self.input_types()?;
        Ok(())
    }

    pub(crate) fn input_types(&self) -> Result<BTreeMap<&str, WorkflowDataType>, String> {
        let mut inputs = BTreeMap::from([(self.source_port.as_str(), WorkflowDataType::Array)]);
        for condition in &self.conditions {
            if let Some(WorkflowListOperatorOperand::InputPort {
                input_port,
                value_type,
            }) = &condition.operand
            {
                insert_input_type(&mut inputs, input_port, value_type.clone())?;
            }
        }
        if let Some(WorkflowListOperatorExtract::InputPort { input_port }) = &self.extract {
            insert_input_type(&mut inputs, input_port, WorkflowDataType::Number)?;
        }
        Ok(inputs)
    }
}

fn insert_input_type<'a>(
    inputs: &mut BTreeMap<&'a str, WorkflowDataType>,
    name: &'a str,
    value_type: WorkflowDataType,
) -> Result<(), String> {
    match inputs.insert(name, value_type.clone()) {
        Some(previous) if previous != value_type => Err(format!(
            "Workflow List Operator input port {name:?} is reused with incompatible types"
        )),
        Some(_) | None => Ok(()),
    }
}

fn operand_type_supported(
    operator: WorkflowListOperatorFilterOperator,
    value_type: &WorkflowDataType,
    operand_type: &WorkflowDataType,
) -> bool {
    if matches!(
        operator,
        WorkflowListOperatorFilterOperator::In | WorkflowListOperatorFilterOperator::NotIn
    ) && value_type == &WorkflowDataType::String
    {
        matches!(
            operand_type,
            WorkflowDataType::String | WorkflowDataType::Array
        )
    } else {
        operand_type == value_type
    }
}

fn validate_operand_value(
    operator: WorkflowListOperatorFilterOperator,
    value_type: &WorkflowDataType,
    value: &Value,
) -> Result<(), String> {
    if matches!(
        operator,
        WorkflowListOperatorFilterOperator::In | WorkflowListOperatorFilterOperator::NotIn
    ) && value_type == &WorkflowDataType::String
    {
        if value.is_string()
            || value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_string))
        {
            return Ok(());
        }
    } else if value_type.matches_json_value(value) {
        return Ok(());
    }
    Err(format!(
        "Workflow List Operator literal operand does not match {}",
        value_type.as_str()
    ))
}

pub(super) fn configuration_schema() -> Result<Schema, String> {
    let condition = Schema::new()
        .attribute("ordinal", AttributeSchema::required(ValueSchema::number()))
        .attribute("key", AttributeSchema::optional(ValueSchema::string()))
        .attribute(
            "value_type",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute("operator", AttributeSchema::required(ValueSchema::string()))
        .attribute(
            "operand_json",
            AttributeSchema::optional(ValueSchema::string()),
        )
        .attribute(
            "operand_port",
            AttributeSchema::optional(ValueSchema::string()),
        )
        .attribute(
            "operand_type",
            AttributeSchema::optional(ValueSchema::string()),
        );
    let extract = Schema::new()
        .attribute("index", AttributeSchema::optional(ValueSchema::number()))
        .attribute(
            "input_port",
            AttributeSchema::optional(ValueSchema::string()),
        );
    let order = Schema::new()
        .attribute("key", AttributeSchema::optional(ValueSchema::string()))
        .attribute(
            "value_type",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute(
            "direction",
            AttributeSchema::required(ValueSchema::string()),
        );
    let limit = Schema::new().attribute("size", AttributeSchema::required(ValueSchema::number()));
    let root = Schema::new()
        .attribute("schema", AttributeSchema::required(ValueSchema::string()))
        .attribute(
            "step_kind",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute(
            "source_port",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute(
            "item_type",
            AttributeSchema::required(ValueSchema::string()),
        )
        .block(
            "condition",
            BlockSchema::new(condition)
                .occurrences(
                    Cardinality::new(0, Some(WORKFLOW_LIST_OPERATOR_MAX_CONDITIONS)).map_err(
                        |error| {
                            format!("Workflow List Operator condition schema is invalid: {error}")
                        },
                    )?,
                )
                .labels(Cardinality::exactly(1))
                .unordered(true),
        )
        .block(
            "extract",
            BlockSchema::new(extract).occurrences(Cardinality::new(0, Some(1)).map_err(
                |error| format!("Workflow List Operator extract schema is invalid: {error}"),
            )?),
        )
        .block(
            "order",
            BlockSchema::new(order).occurrences(Cardinality::new(0, Some(1)).map_err(|error| {
                format!("Workflow List Operator order schema is invalid: {error}")
            })?),
        )
        .block(
            "limit",
            BlockSchema::new(limit).occurrences(Cardinality::new(0, Some(1)).map_err(|error| {
                format!("Workflow List Operator limit schema is invalid: {error}")
            })?),
        );
    Ok(Schema::new().block(
        "configuration",
        BlockSchema::new(root).occurrences(Cardinality::exactly(1)),
    ))
}

pub(super) fn parse_configuration(
    root: &Block,
) -> Result<WorkflowListOperatorConfiguration, String> {
    let mut conditions = root
        .blocks
        .iter()
        .filter(|block| block.name == "condition")
        .map(parse_condition)
        .collect::<Result<Vec<_>, String>>()?;
    conditions.sort_by(|left, right| {
        left.ordinal
            .cmp(&right.ordinal)
            .then_with(|| left.id.cmp(&right.id))
    });
    let configuration = WorkflowListOperatorConfiguration {
        source_port: required_string(root, "source_port")?,
        item_type: WorkflowDataType::parse(&required_string(root, "item_type")?)?,
        conditions,
        extract: root
            .blocks
            .iter()
            .find(|block| block.name == "extract")
            .map(parse_extract)
            .transpose()?,
        order: root
            .blocks
            .iter()
            .find(|block| block.name == "order")
            .map(parse_order)
            .transpose()?,
        limit: root
            .blocks
            .iter()
            .find(|block| block.name == "limit")
            .map(|block| parse_u32(required_number(block, "size")?, "limit"))
            .transpose()?,
    };
    configuration.validate()?;
    Ok(configuration)
}

fn parse_condition(block: &Block) -> Result<WorkflowListOperatorFilterCondition, String> {
    let operand_json = optional_string(block, "operand_json")?;
    let operand_port = optional_string(block, "operand_port")?;
    let operand_type = optional_string(block, "operand_type")?;
    let operand = match (operand_json, operand_port, operand_type) {
        (Some(source), None, None) => {
            let value = serde_json::from_str::<Value>(&source).map_err(|error| {
                format!("Workflow List Operator literal operand JSON is invalid: {error}")
            })?;
            let canonical = String::from_utf8(canonical_json_bounded(
                &value,
                WORKFLOW_LIST_OPERATOR_MAX_LITERAL_BYTES,
                "Workflow List Operator literal operand",
            )?)
            .map_err(|_| "Workflow List Operator literal operand JSON is not UTF-8".to_owned())?;
            if canonical != source {
                return Err("Workflow List Operator literal operand JSON is not canonical".into());
            }
            Some(WorkflowListOperatorOperand::Literal(value))
        }
        (None, Some(input_port), Some(value_type)) => {
            Some(WorkflowListOperatorOperand::InputPort {
                input_port,
                value_type: WorkflowDataType::parse(&value_type)?,
            })
        }
        (None, None, None) => None,
        _ => {
            return Err(
                "Workflow List Operator condition must declare one complete literal or input-port operand"
                    .into(),
            )
        }
    };
    Ok(WorkflowListOperatorFilterCondition {
        id: required_label(block, "Workflow List Operator condition")?,
        ordinal: parse_non_negative_u32(required_number(block, "ordinal")?, "condition ordinal")?,
        key: optional_string(block, "key")?,
        value_type: WorkflowDataType::parse(&required_string(block, "value_type")?)?,
        operator: WorkflowListOperatorFilterOperator::parse(&required_string(block, "operator")?)?,
        operand,
    })
}

fn parse_extract(block: &Block) -> Result<WorkflowListOperatorExtract, String> {
    match (
        optional_number(block, "index")?,
        optional_string(block, "input_port")?,
    ) {
        (Some(index), None) => Ok(WorkflowListOperatorExtract::Literal {
            index: parse_u32(index, "extract index")?,
        }),
        (None, Some(input_port)) => Ok(WorkflowListOperatorExtract::InputPort { input_port }),
        _ => Err(
            "Workflow List Operator extract must declare exactly one index or input port".into(),
        ),
    }
}

fn parse_order(block: &Block) -> Result<WorkflowListOperatorOrder, String> {
    Ok(WorkflowListOperatorOrder {
        key: optional_string(block, "key")?,
        value_type: WorkflowDataType::parse(&required_string(block, "value_type")?)?,
        direction: WorkflowListOperatorOrderDirection::parse(&required_string(
            block,
            "direction",
        )?)?,
    })
}

fn parse_u32(value: f64, label: &str) -> Result<u32, String> {
    u32::try_from(positive_integer(value)?)
        .map_err(|_| format!("Workflow List Operator {label} exceeds u32"))
}

fn parse_non_negative_u32(value: f64, label: &str) -> Result<u32, String> {
    u32::try_from(non_negative_integer(value)?)
        .map_err(|_| format!("Workflow List Operator {label} exceeds u32"))
}

pub(super) fn configuration_block(
    configuration: &WorkflowListOperatorConfiguration,
) -> Result<Block, String> {
    configuration.validate()?;
    let mut root = BlockBuilder::new("configuration")
        .attr(
            "schema",
            string(WORKFLOW_LIST_OPERATOR_CONFIGURATION_SCHEMA),
        )
        .attr("step_kind", string(WorkflowStepKind::Transform.as_str()))
        .attr("source_port", string(&configuration.source_port))
        .attr("item_type", string(configuration.item_type.as_str()));
    let mut conditions = configuration.conditions.iter().collect::<Vec<_>>();
    conditions.sort_by(|left, right| {
        left.ordinal
            .cmp(&right.ordinal)
            .then_with(|| left.id.cmp(&right.id))
    });
    for condition in conditions {
        let mut block = BlockBuilder::new("condition")
            .label(&condition.id)
            .attr("ordinal", number(f64::from(condition.ordinal)))
            .attr("value_type", string(condition.value_type.as_str()))
            .attr("operator", string(condition.operator.as_str()));
        if let Some(key) = &condition.key {
            block = block.attr("key", string(key));
        }
        match &condition.operand {
            Some(WorkflowListOperatorOperand::Literal(value)) => {
                let canonical = String::from_utf8(canonical_json_bounded(
                    value,
                    WORKFLOW_LIST_OPERATOR_MAX_LITERAL_BYTES,
                    "Workflow List Operator literal operand",
                )?)
                .map_err(|_| {
                    "Workflow List Operator literal operand JSON is not UTF-8".to_owned()
                })?;
                block = block.attr("operand_json", string(&canonical));
            }
            Some(WorkflowListOperatorOperand::InputPort {
                input_port,
                value_type,
            }) => {
                block = block
                    .attr("operand_port", string(input_port))
                    .attr("operand_type", string(value_type.as_str()));
            }
            None => {}
        }
        root = root.nested_block(block.build());
    }
    if let Some(extract) = &configuration.extract {
        let block = match extract {
            WorkflowListOperatorExtract::Literal { index } => {
                BlockBuilder::new("extract").attr("index", number(f64::from(*index)))
            }
            WorkflowListOperatorExtract::InputPort { input_port } => {
                BlockBuilder::new("extract").attr("input_port", string(input_port))
            }
        };
        root = root.nested_block(block.build());
    }
    if let Some(order) = &configuration.order {
        let mut block = BlockBuilder::new("order")
            .attr("value_type", string(order.value_type.as_str()))
            .attr("direction", string(order.direction.as_str()));
        if let Some(key) = &order.key {
            block = block.attr("key", string(key));
        }
        root = root.nested_block(block.build());
    }
    if let Some(limit) = configuration.limit {
        root = root.nested_block(
            BlockBuilder::new("limit")
                .attr("size", number(f64::from(limit)))
                .build(),
        );
    }
    Ok(root.build())
}

pub(super) fn validate_transform_kind(step_kind: WorkflowStepKind) -> Result<(), String> {
    if step_kind == WorkflowStepKind::Transform {
        Ok(())
    } else {
        Err("Workflow List Operator configuration requires transform step kind".into())
    }
}
