use super::codec::{
    non_negative_integer, required_bool, required_label, required_number, required_string,
};
use super::{validate_identifier, WorkflowDataType, WorkflowStepKind};
use a3s_acl::{AttributeSchema, Block, BlockSchema, Cardinality, Schema, ValueSchema};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const WORKFLOW_VARIABLE_AGGREGATE_CONFIGURATION_SCHEMA: &str =
    "cloud.workflow.configuration.variable-aggregate.v1";
pub const WORKFLOW_VARIABLE_AGGREGATE_MAX_GROUPS: usize = 32;
pub const WORKFLOW_VARIABLE_AGGREGATE_MAX_CANDIDATES_PER_GROUP: usize = 64;
pub const WORKFLOW_VARIABLE_AGGREGATE_MAX_CANDIDATES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowVariableAggregateCandidate {
    pub input_port: String,
    pub ordinal: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowVariableAggregateGroup {
    pub output_port: String,
    pub output_type: WorkflowDataType,
    pub candidates: Vec<WorkflowVariableAggregateCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowVariableAggregateConfiguration {
    pub group_enabled: bool,
    pub groups: Vec<WorkflowVariableAggregateGroup>,
}

impl WorkflowVariableAggregateConfiguration {
    pub fn validate(&self) -> Result<(), String> {
        if self.groups.is_empty() || self.groups.len() > WORKFLOW_VARIABLE_AGGREGATE_MAX_GROUPS {
            return Err(format!(
                "Workflow Variable Aggregator must declare between 1 and {WORKFLOW_VARIABLE_AGGREGATE_MAX_GROUPS} groups"
            ));
        }
        if !self.group_enabled
            && !matches!(self.groups.as_slice(), [group] if group.output_port == "output")
        {
            return Err(
                "Workflow Variable Aggregator simple mode requires exactly one output group".into(),
            );
        }

        let mut output_ports = BTreeSet::new();
        let mut candidate_total = 0usize;
        for group in &self.groups {
            validate_identifier(
                "Workflow Variable Aggregator output port",
                &group.output_port,
            )?;
            if !output_ports.insert(group.output_port.as_str()) {
                return Err(format!(
                    "Workflow Variable Aggregator contains duplicate output port {:?}",
                    group.output_port
                ));
            }
            if matches!(
                group.output_type,
                WorkflowDataType::Any | WorkflowDataType::Null
            ) {
                return Err(format!(
                    "Workflow Variable Aggregator group {:?} requires a concrete non-null output type",
                    group.output_port
                ));
            }
            if group.candidates.is_empty()
                || group.candidates.len() > WORKFLOW_VARIABLE_AGGREGATE_MAX_CANDIDATES_PER_GROUP
            {
                return Err(format!(
                    "Workflow Variable Aggregator group {:?} must declare between 1 and {WORKFLOW_VARIABLE_AGGREGATE_MAX_CANDIDATES_PER_GROUP} candidates",
                    group.output_port
                ));
            }
            candidate_total = candidate_total
                .checked_add(group.candidates.len())
                .ok_or_else(|| {
                    "Workflow Variable Aggregator candidate count overflowed".to_owned()
                })?;
            let mut input_ports = BTreeSet::new();
            let mut ordinals = BTreeSet::new();
            for candidate in &group.candidates {
                validate_identifier(
                    "Workflow Variable Aggregator candidate input port",
                    &candidate.input_port,
                )?;
                if !input_ports.insert(candidate.input_port.as_str()) {
                    return Err(format!(
                        "Workflow Variable Aggregator group {:?} contains duplicate candidate input {:?}",
                        group.output_port, candidate.input_port
                    ));
                }
                if !ordinals.insert(candidate.ordinal) {
                    return Err(format!(
                        "Workflow Variable Aggregator group {:?} contains duplicate candidate ordinal {}",
                        group.output_port, candidate.ordinal
                    ));
                }
            }
            let expected = (0..group.candidates.len())
                .map(|ordinal| {
                    u32::try_from(ordinal).map_err(|_| {
                        "Workflow Variable Aggregator candidate ordinal exceeds u32".to_owned()
                    })
                })
                .collect::<Result<BTreeSet<_>, String>>()?;
            if ordinals != expected {
                return Err(format!(
                    "Workflow Variable Aggregator group {:?} candidate ordinals must be contiguous and zero-based",
                    group.output_port
                ));
            }
        }
        if candidate_total > WORKFLOW_VARIABLE_AGGREGATE_MAX_CANDIDATES {
            return Err(format!(
                "Workflow Variable Aggregator cannot declare more than {WORKFLOW_VARIABLE_AGGREGATE_MAX_CANDIDATES} candidates"
            ));
        }
        Ok(())
    }

    pub(crate) fn candidate_types(&self) -> Result<BTreeMap<&str, &WorkflowDataType>, String> {
        let mut values = BTreeMap::new();
        for group in &self.groups {
            for candidate in &group.candidates {
                match values.insert(candidate.input_port.as_str(), &group.output_type) {
                    Some(previous) if previous != &group.output_type => {
                        return Err(format!(
                            "Workflow Variable Aggregator candidate input {:?} is reused with incompatible group types",
                            candidate.input_port
                        ));
                    }
                    Some(_) | None => {}
                }
            }
        }
        Ok(values)
    }
}

pub(super) fn configuration_schema() -> Result<Schema, String> {
    let candidate =
        Schema::new().attribute("ordinal", AttributeSchema::required(ValueSchema::number()));
    let group = Schema::new()
        .attribute(
            "output_type",
            AttributeSchema::required(ValueSchema::string()),
        )
        .block(
            "candidate",
            BlockSchema::new(candidate)
                .occurrences(
                    Cardinality::new(
                        1,
                        Some(WORKFLOW_VARIABLE_AGGREGATE_MAX_CANDIDATES_PER_GROUP),
                    )
                    .map_err(|error| {
                        format!("Workflow Variable Aggregator candidate schema is invalid: {error}")
                    })?,
                )
                .labels(Cardinality::exactly(1))
                .unordered(true),
        );
    let root = Schema::new()
        .attribute("schema", AttributeSchema::required(ValueSchema::string()))
        .attribute(
            "step_kind",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute(
            "group_enabled",
            AttributeSchema::required(ValueSchema::bool()),
        )
        .block(
            "group",
            BlockSchema::new(group)
                .occurrences(
                    Cardinality::new(1, Some(WORKFLOW_VARIABLE_AGGREGATE_MAX_GROUPS)).map_err(
                        |error| {
                            format!("Workflow Variable Aggregator group schema is invalid: {error}")
                        },
                    )?,
                )
                .labels(Cardinality::exactly(1))
                .unordered(true),
        );
    Ok(Schema::new().block(
        "configuration",
        BlockSchema::new(root).occurrences(Cardinality::exactly(1)),
    ))
}

pub(super) fn parse_configuration(
    root: &Block,
) -> Result<WorkflowVariableAggregateConfiguration, String> {
    let mut groups = root
        .blocks
        .iter()
        .filter(|block| block.name == "group")
        .map(|group| {
            let mut candidates = group
                .blocks
                .iter()
                .filter(|block| block.name == "candidate")
                .map(|candidate| {
                    Ok(WorkflowVariableAggregateCandidate {
                        input_port: required_label(
                            candidate,
                            "Workflow Variable Aggregator candidate",
                        )?,
                        ordinal: u32::try_from(non_negative_integer(required_number(
                            candidate, "ordinal",
                        )?)?)
                        .map_err(|_| {
                            "Workflow Variable Aggregator candidate ordinal exceeds u32".to_owned()
                        })?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            candidates.sort_by(|left, right| {
                left.ordinal
                    .cmp(&right.ordinal)
                    .then_with(|| left.input_port.cmp(&right.input_port))
            });
            Ok(WorkflowVariableAggregateGroup {
                output_port: required_label(group, "Workflow Variable Aggregator group")?,
                output_type: WorkflowDataType::parse(&required_string(group, "output_type")?)?,
                candidates,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    groups.sort_by(|left, right| left.output_port.cmp(&right.output_port));
    let value = WorkflowVariableAggregateConfiguration {
        group_enabled: required_bool(root, "group_enabled")?,
        groups,
    };
    value.validate()?;
    Ok(value)
}

pub(super) fn validate_transform_kind(step_kind: WorkflowStepKind) -> Result<(), String> {
    if step_kind == WorkflowStepKind::Transform {
        Ok(())
    } else {
        Err("Workflow Variable Aggregator configuration requires transform step kind".into())
    }
}
