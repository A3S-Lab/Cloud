use super::validation::{
    required_string, required_strings, required_value, validate_dotted_identifier,
    validate_exact_semver, validate_identifier,
};
use super::{
    CapabilityReference, CapabilityType, WorkflowPlan, WorkflowStepDescriptorBindings,
    WorkflowStepKind, WorkflowVariableContract, WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
};
use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_acl::builder::{integer, list, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const WORKFLOW_COMPOSITE_REGIONS_SCHEMA: &str = "cloud.workflow.composite-regions.v1";
pub const WORKFLOW_COMPOSITE_REGIONS_MAX_ACL_BYTES: usize = 512 * 1024;
pub const WORKFLOW_COMPOSITE_REGION_MAX_COUNT: usize = 512;
pub const WORKFLOW_ITERATION_MAX_ITEMS: u32 = 10_000;
pub const WORKFLOW_ITERATION_MAX_CONCURRENCY: u32 = 10;
pub const WORKFLOW_LOOP_MAX_ITERATIONS: u32 = 10_000;
pub const WORKFLOW_COMPOSITE_REGION_MAX_TIME_BUDGET_SECONDS: u64 = 30 * 24 * 60 * 60;
const WORKFLOW_LOOP_MAX_TERMINATION_PATH_SEGMENTS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowIterationFailureMode {
    Terminate,
    ContinueNull,
    RemoveFailed,
}

impl WorkflowIterationFailureMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Terminate => "terminate",
            Self::ContinueNull => "continue_null",
            Self::RemoveFailed => "remove_failed",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "terminate" => Ok(Self::Terminate),
            "continue_null" => Ok(Self::ContinueNull),
            "remove_failed" => Ok(Self::RemoveFailed),
            _ => Err(format!(
                "unsupported Workflow iteration failure mode {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowIterationRegionPolicy {
    pub step_id: String,
    pub maximum_items: u32,
    pub maximum_concurrency: u32,
    pub failure_mode: WorkflowIterationFailureMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowLoopRegionPolicy {
    pub step_id: String,
    pub maximum_iterations: u32,
    pub time_budget_seconds: u64,
    pub termination_path: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum WorkflowCompositeRegionPolicy {
    Iteration(WorkflowIterationRegionPolicy),
    Loop(WorkflowLoopRegionPolicy),
}

impl WorkflowCompositeRegionPolicy {
    pub fn step_id(&self) -> &str {
        match self {
            Self::Iteration(value) => &value.step_id,
            Self::Loop(value) => &value.step_id,
        }
    }

    pub const fn semantic_profile(&self) -> &'static str {
        match self {
            Self::Iteration(_) => "workflow.iteration",
            Self::Loop(_) => "workflow.loop",
        }
    }

    fn validate(&self) -> Result<(), String> {
        validate_identifier("Workflow composite region step", self.step_id())?;
        match self {
            Self::Iteration(value) => {
                if value.maximum_items == 0
                    || value.maximum_items > WORKFLOW_ITERATION_MAX_ITEMS
                    || value.maximum_concurrency == 0
                    || value.maximum_concurrency > WORKFLOW_ITERATION_MAX_CONCURRENCY
                    || value.maximum_concurrency > value.maximum_items
                {
                    return Err("Workflow iteration bounds are invalid".into());
                }
            }
            Self::Loop(value) => {
                if value.maximum_iterations == 0
                    || value.maximum_iterations > WORKFLOW_LOOP_MAX_ITERATIONS
                    || value.time_budget_seconds == 0
                    || value.time_budget_seconds > WORKFLOW_COMPOSITE_REGION_MAX_TIME_BUDGET_SECONDS
                    || value.termination_path.len() > WORKFLOW_LOOP_MAX_TERMINATION_PATH_SEGMENTS
                {
                    return Err("Workflow loop bounds are invalid".into());
                }
                for segment in &value.termination_path {
                    validate_identifier("Workflow loop termination path", segment)?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowCompositeRegionsSpec {
    pub id: String,
    pub revision: String,
    pub compiler_schema_version: u32,
    pub regions: Vec<WorkflowCompositeRegionPolicy>,
}

/// Immutable execution policy for Workflow-owned Iteration and Loop regions.
///
/// Exact child Workflow identity remains in each graph step's capability
/// reference. This contract freezes only region scheduling, failure, and
/// termination semantics; durable execution and replay remain owned by Flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowCompositeRegions {
    spec: WorkflowCompositeRegionsSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl WorkflowCompositeRegions {
    pub fn from_spec(mut spec: WorkflowCompositeRegionsSpec) -> Result<Self, String> {
        validate_dotted_identifier("Workflow composite regions ID", &spec.id)?;
        validate_exact_semver("Workflow composite regions revision", &spec.revision)?;
        if spec.compiler_schema_version != WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION {
            return Err("Workflow composite regions compiler schema is unsupported".into());
        }
        if spec.regions.is_empty() || spec.regions.len() > WORKFLOW_COMPOSITE_REGION_MAX_COUNT {
            return Err("Workflow composite region count is invalid".into());
        }
        for region in &spec.regions {
            region.validate()?;
        }
        spec.regions
            .sort_by(|left, right| left.step_id().cmp(right.step_id()));
        let unique = spec
            .regions
            .iter()
            .map(WorkflowCompositeRegionPolicy::step_id)
            .collect::<BTreeSet<_>>();
        if unique.len() != spec.regions.len() {
            return Err("Workflow composite regions contain duplicate steps".into());
        }

        let document = regions_document(&spec)?;
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > WORKFLOW_COMPOSITE_REGIONS_MAX_ACL_BYTES {
            return Err("Workflow composite regions ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl).map_err(|error| {
            format!("generated Workflow composite regions ACL is invalid: {error}")
        })?;
        let digest = digest_document(&reparsed)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > WORKFLOW_COMPOSITE_REGIONS_MAX_ACL_BYTES {
            return Err("Workflow composite regions ACL size is invalid".into());
        }
        if source.replace("\r\n", "").contains('\r') {
            return Err("Workflow composite regions contain a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let value = Self::from_spec(parse_regions_spec(&normalized)?)?;
        if value.canonical_acl != normalized {
            return Err("Workflow composite regions ACL is not canonical".into());
        }
        Ok(value)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let value = Self::parse_acl(source)?;
        if value.digest.as_str() != stored_digest {
            return Err("stored Workflow composite regions and digest do not match".into());
        }
        Ok(value)
    }

    pub fn validate_identity(
        &self,
        bindings: &WorkflowStepDescriptorBindings,
        variables: &WorkflowVariableContract,
    ) -> Result<(), String> {
        if self.spec.id != bindings.id()
            || self.spec.id != variables.id()
            || self.spec.revision != bindings.revision()
            || self.spec.revision != variables.revision()
            || self.spec.compiler_schema_version != bindings.compiler_schema_version()
            || self.spec.compiler_schema_version != variables.compiler_schema_version()
        {
            return Err(
                "Workflow composite regions identity does not match its semantic contracts".into(),
            );
        }
        Ok(())
    }

    pub fn validate_plan(&self, plan: &WorkflowPlan) -> Result<(), String> {
        if plan.composite_regions_digest.as_ref() != Some(&self.digest) {
            return Err("Workflow composite regions drifted from the PlanRevision".into());
        }
        let composite_steps = plan
            .steps
            .iter()
            .filter(|step| step.kind == WorkflowStepKind::Subworkflow)
            .collect::<Vec<_>>();
        if composite_steps.len() != self.spec.regions.len() {
            return Err("Workflow composite regions do not cover the PlanRevision".into());
        }
        for step in composite_steps {
            let region = self.resolve(&step.id).ok_or_else(|| {
                format!(
                    "Workflow composite step {:?} has no immutable region policy",
                    step.id
                )
            })?;
            let descriptor = step.descriptor.as_ref().ok_or_else(|| {
                format!(
                    "Workflow composite step {:?} has no semantic descriptor",
                    step.id
                )
            })?;
            if descriptor.descriptor_id != region.semantic_profile() {
                return Err(format!(
                    "Workflow composite step {:?} descriptor {:?} does not match its immutable {} region policy",
                    step.id,
                    descriptor.descriptor_id,
                    region.semantic_profile()
                ));
            }
            let capability = step.capability.as_ref().ok_or_else(|| {
                format!(
                    "Workflow composite step {:?} has no child Workflow revision",
                    step.id
                )
            })?;
            if !is_exact_child_workflow_revision(capability) {
                return Err(format!(
                    "Workflow composite step {:?} does not pin one exact workflow.run revision",
                    step.id
                ));
            }
        }
        Ok(())
    }

    pub fn resolve(&self, step_id: &str) -> Option<&WorkflowCompositeRegionPolicy> {
        self.spec
            .regions
            .binary_search_by(|region| region.step_id().cmp(step_id))
            .ok()
            .map(|index| &self.spec.regions[index])
    }

    pub const fn spec(&self) -> &WorkflowCompositeRegionsSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

pub(super) fn is_exact_child_workflow_revision(capability: &CapabilityReference) -> bool {
    capability.capability_type == CapabilityType::WorkflowRevision
        && capability.capability == "workflow.run"
        && uuid::Uuid::parse_str(&capability.revision).is_ok_and(|revision| !revision.is_nil())
}

fn regions_document(spec: &WorkflowCompositeRegionsSpec) -> Result<Document, String> {
    let mut root = BlockBuilder::new("composite_regions")
        .label(&spec.id)
        .attr("schema", string(WORKFLOW_COMPOSITE_REGIONS_SCHEMA))
        .attr("revision", string(&spec.revision))
        .attr(
            "compiler_schema_version",
            integer(i64::from(spec.compiler_schema_version)),
        );
    for region in &spec.regions {
        let block = match region {
            WorkflowCompositeRegionPolicy::Iteration(value) => BlockBuilder::new("iteration")
                .label(&value.step_id)
                .attr("maximum_items", integer(i64::from(value.maximum_items)))
                .attr(
                    "maximum_concurrency",
                    integer(i64::from(value.maximum_concurrency)),
                )
                .attr("failure_mode", string(value.failure_mode.as_str()))
                .build(),
            WorkflowCompositeRegionPolicy::Loop(value) => {
                let time_budget_seconds =
                    i64::try_from(value.time_budget_seconds).map_err(|_| {
                        "Workflow loop time budget exceeds the ACL integer range".to_owned()
                    })?;
                BlockBuilder::new("loop")
                    .label(&value.step_id)
                    .attr(
                        "maximum_iterations",
                        integer(i64::from(value.maximum_iterations)),
                    )
                    .attr("time_budget_seconds", integer(time_budget_seconds))
                    .attr(
                        "termination_path",
                        list(
                            value
                                .termination_path
                                .iter()
                                .map(|segment| string(segment))
                                .collect(),
                        ),
                    )
                    .build()
            }
        };
        root = root.nested_block(block);
    }
    Ok(Document {
        blocks: vec![root.build()],
    })
}

fn parse_regions_spec(source: &str) -> Result<WorkflowCompositeRegionsSpec, String> {
    let document = parse_acl(source)
        .map_err(|error| format!("Workflow composite regions ACL is invalid: {error}"))?;
    if document.blocks.len() != 1 {
        return Err("Workflow composite regions require exactly one root block".into());
    }
    let root = &document.blocks[0];
    exact_block(
        root,
        "composite_regions",
        &["compiler_schema_version", "revision", "schema"],
        1,
        true,
    )?;
    if required_string(root, "schema")? != WORKFLOW_COMPOSITE_REGIONS_SCHEMA {
        return Err("Workflow composite regions schema is unsupported".into());
    }
    if root
        .blocks
        .iter()
        .any(|block| !matches!(block.name.as_str(), "iteration" | "loop"))
    {
        return Err("Workflow composite regions contain an unknown block".into());
    }
    Ok(WorkflowCompositeRegionsSpec {
        id: root.labels[0].clone(),
        revision: required_string(root, "revision")?,
        compiler_schema_version: required_u32(root, "compiler_schema_version")?,
        regions: root
            .blocks
            .iter()
            .map(parse_region)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn parse_region(block: &Block) -> Result<WorkflowCompositeRegionPolicy, String> {
    match block.name.as_str() {
        "iteration" => {
            exact_block(
                block,
                "iteration",
                &["failure_mode", "maximum_concurrency", "maximum_items"],
                1,
                false,
            )?;
            Ok(WorkflowCompositeRegionPolicy::Iteration(
                WorkflowIterationRegionPolicy {
                    step_id: block.labels[0].clone(),
                    maximum_items: required_u32(block, "maximum_items")?,
                    maximum_concurrency: required_u32(block, "maximum_concurrency")?,
                    failure_mode: WorkflowIterationFailureMode::parse(&required_string(
                        block,
                        "failure_mode",
                    )?)?,
                },
            ))
        }
        "loop" => {
            exact_block(
                block,
                "loop",
                &[
                    "maximum_iterations",
                    "termination_path",
                    "time_budget_seconds",
                ],
                1,
                false,
            )?;
            Ok(WorkflowCompositeRegionPolicy::Loop(
                WorkflowLoopRegionPolicy {
                    step_id: block.labels[0].clone(),
                    maximum_iterations: required_u32(block, "maximum_iterations")?,
                    time_budget_seconds: required_u64(block, "time_budget_seconds")?,
                    termination_path: required_strings(block, "termination_path")?,
                },
            ))
        }
        _ => Err("Workflow composite region kind is unsupported".into()),
    }
}

fn exact_block(
    block: &Block,
    name: &str,
    required: &[&str],
    labels: usize,
    allow_nested: bool,
) -> Result<(), String> {
    if block.name != name
        || block.labels.len() != labels
        || block.attributes.len() != required.len()
        || required
            .iter()
            .any(|name| !block.attributes.contains_key(*name))
        || block
            .attributes
            .keys()
            .any(|key| !required.contains(&key.as_str()))
        || (!allow_nested && !block.blocks.is_empty())
    {
        return Err(format!(
            "Workflow composite regions {name} block shape is invalid"
        ));
    }
    Ok(())
}

fn required_u32(block: &Block, name: &str) -> Result<u32, String> {
    let value = required_number(block, name)?;
    if value == 0 || value > u64::from(u32::MAX) {
        return Err(format!(
            "Workflow composite regions field {name:?} must be a positive u32"
        ));
    }
    Ok(value as u32)
}

fn required_u64(block: &Block, name: &str) -> Result<u64, String> {
    let value = required_number(block, name)?;
    if value == 0 {
        return Err(format!(
            "Workflow composite regions field {name:?} must be a positive u64"
        ));
    }
    Ok(value)
}

fn required_number(block: &Block, name: &str) -> Result<u64, String> {
    let Value::Number(value) = required_value(block, name)? else {
        return Err(format!(
            "Workflow composite regions field {name:?} must be an integer"
        ));
    };
    if !value.is_finite() || value.fract() != 0.0 || *value <= 0.0 || *value > u64::MAX as f64 {
        return Err(format!(
            "Workflow composite regions field {name:?} must be a positive integer"
        ));
    }
    Ok(*value as u64)
}

fn digest_document(document: &Document) -> Result<Sha256Digest, String> {
    Sha256Digest::parse(
        canonical_digest(document).map_err(|error| {
            format!("Workflow composite regions are not canonicalizable: {error}")
        })?,
    )
}
