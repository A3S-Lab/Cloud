use super::validation::{validate_dotted_identifier, validate_exact_semver, validate_identifier};
use super::WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION;
use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_acl::builder::{integer, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const WORKFLOW_STEP_DESCRIPTOR_BINDINGS_SCHEMA: &str =
    "cloud.workflow.step-descriptor-bindings.v1";
pub const WORKFLOW_STEP_DESCRIPTOR_BINDINGS_MAX_ACL_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStepDescriptorBinding {
    pub step_id: String,
    pub descriptor_id: String,
    pub descriptor_revision: String,
    pub semantic_digest: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStepDescriptorBindingsSpec {
    pub id: String,
    pub revision: String,
    pub compiler_schema_version: u32,
    pub bindings: Vec<WorkflowStepDescriptorBinding>,
}

/// Exact descriptor semantics selected for every step in one Workflow graph.
///
/// Registry presentation and admission metadata stay outside this digest. The
/// binding digest changes only when executable step semantics change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowStepDescriptorBindings {
    spec: WorkflowStepDescriptorBindingsSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl WorkflowStepDescriptorBindings {
    pub fn from_spec(mut spec: WorkflowStepDescriptorBindingsSpec) -> Result<Self, String> {
        validate_dotted_identifier("Workflow descriptor bindings ID", &spec.id)?;
        validate_exact_semver("Workflow descriptor bindings revision", &spec.revision)?;
        if spec.compiler_schema_version != WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION
            || spec.bindings.len() < 2
            || spec.bindings.len() > 10_000
        {
            return Err("Workflow descriptor binding bounds are invalid".into());
        }
        for binding in &spec.bindings {
            validate_identifier("Workflow descriptor binding step", &binding.step_id)?;
            validate_dotted_identifier("Workflow descriptor binding ID", &binding.descriptor_id)?;
            validate_exact_semver(
                "Workflow descriptor binding revision",
                &binding.descriptor_revision,
            )?;
        }
        spec.bindings
            .sort_by(|left, right| left.step_id.cmp(&right.step_id));
        if spec
            .bindings
            .windows(2)
            .any(|pair| pair[0].step_id == pair[1].step_id)
        {
            return Err("Workflow descriptor bindings contain a duplicate step".into());
        }

        let document = binding_document(&spec);
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > WORKFLOW_STEP_DESCRIPTOR_BINDINGS_MAX_ACL_BYTES {
            return Err("Workflow descriptor bindings ACL exceeds its storage bound".into());
        }
        let digest = Sha256Digest::parse(canonical_digest(&document).map_err(|error| {
            format!("Workflow descriptor bindings are not canonicalizable: {error}")
        })?)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > WORKFLOW_STEP_DESCRIPTOR_BINDINGS_MAX_ACL_BYTES {
            return Err("Workflow descriptor bindings ACL size is invalid".into());
        }
        if source.replace("\r\n", "").contains('\r') {
            return Err("Workflow descriptor bindings contain a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let document = parse_acl(&normalized)
            .map_err(|error| format!("Workflow descriptor bindings ACL is invalid: {error}"))?;
        let bindings = Self::from_spec(parse_binding_spec(&document)?)?;
        if bindings.canonical_acl != normalized {
            return Err("Workflow descriptor bindings ACL is not canonical".into());
        }
        Ok(bindings)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let bindings = Self::parse_acl(source)?;
        if bindings.digest.as_str() != stored_digest {
            return Err("stored Workflow descriptor bindings and digest do not match".into());
        }
        Ok(bindings)
    }

    pub fn id(&self) -> &str {
        &self.spec.id
    }

    pub fn revision(&self) -> &str {
        &self.spec.revision
    }

    pub const fn compiler_schema_version(&self) -> u32 {
        self.spec.compiler_schema_version
    }

    pub fn bindings(&self) -> &[WorkflowStepDescriptorBinding] {
        &self.spec.bindings
    }

    pub fn resolve(&self, step_id: &str) -> Option<&WorkflowStepDescriptorBinding> {
        self.spec
            .bindings
            .binary_search_by(|binding| binding.step_id.as_str().cmp(step_id))
            .ok()
            .map(|index| &self.spec.bindings[index])
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn binding_document(spec: &WorkflowStepDescriptorBindingsSpec) -> Document {
    let mut root = BlockBuilder::new("descriptor_bindings")
        .label(&spec.id)
        .attr("schema", string(WORKFLOW_STEP_DESCRIPTOR_BINDINGS_SCHEMA))
        .attr("revision", string(&spec.revision))
        .attr(
            "compiler_schema_version",
            integer(i64::from(spec.compiler_schema_version)),
        );
    for binding in &spec.bindings {
        root = root.nested_block(
            BlockBuilder::new("binding")
                .label(&binding.step_id)
                .attr("descriptor_id", string(&binding.descriptor_id))
                .attr("descriptor_revision", string(&binding.descriptor_revision))
                .attr("semantic_digest", string(binding.semantic_digest.as_str()))
                .build(),
        );
    }
    Document {
        blocks: vec![root.build()],
    }
}

fn parse_binding_spec(document: &Document) -> Result<WorkflowStepDescriptorBindingsSpec, String> {
    if document.blocks.len() != 1 {
        return Err("Workflow descriptor bindings require exactly one root block".into());
    }
    let root = &document.blocks[0];
    exact_block(
        root,
        "descriptor_bindings",
        &["compiler_schema_version", "revision", "schema"],
        1,
        true,
    )?;
    if required_string(root, "schema")? != WORKFLOW_STEP_DESCRIPTOR_BINDINGS_SCHEMA
        || root.blocks.iter().any(|block| block.name != "binding")
    {
        return Err("Workflow descriptor bindings schema is unsupported".into());
    }
    Ok(WorkflowStepDescriptorBindingsSpec {
        id: root.labels[0].clone(),
        revision: required_string(root, "revision")?,
        compiler_schema_version: required_u32(root, "compiler_schema_version")?,
        bindings: root
            .blocks
            .iter()
            .map(parse_binding)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn parse_binding(block: &Block) -> Result<WorkflowStepDescriptorBinding, String> {
    exact_block(
        block,
        "binding",
        &["descriptor_id", "descriptor_revision", "semantic_digest"],
        1,
        false,
    )?;
    Ok(WorkflowStepDescriptorBinding {
        step_id: block.labels[0].clone(),
        descriptor_id: required_string(block, "descriptor_id")?,
        descriptor_revision: required_string(block, "descriptor_revision")?,
        semantic_digest: Sha256Digest::parse(required_string(block, "semantic_digest")?)?,
    })
}

fn exact_block(
    block: &Block,
    name: &str,
    attributes: &[&str],
    labels: usize,
    allow_nested: bool,
) -> Result<(), String> {
    let allowed = attributes.iter().copied().collect::<BTreeSet<_>>();
    if block.name != name
        || block.labels.len() != labels
        || block.attributes.len() != attributes.len()
        || attributes
            .iter()
            .any(|attribute| !block.attributes.contains_key(*attribute))
        || block
            .attributes
            .keys()
            .any(|attribute| !allowed.contains(attribute.as_str()))
        || (!allow_nested && !block.blocks.is_empty())
    {
        return Err(format!(
            "Workflow descriptor bindings {name} block shape is invalid"
        ));
    }
    Ok(())
}

fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("Workflow descriptor bindings field {name:?} is required"))
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("Workflow descriptor bindings field {name:?} must be a string"))
}

fn required_u32(block: &Block, name: &str) -> Result<u32, String> {
    let value = required_value(block, name)?
        .as_number()
        .ok_or_else(|| format!("Workflow descriptor bindings field {name:?} must be an integer"))?;
    if !value.is_finite() || value.fract() != 0.0 || value <= 0.0 || value > f64::from(u32::MAX) {
        return Err(format!(
            "Workflow descriptor bindings field {name:?} must be a positive u32"
        ));
    }
    Ok(value as u32)
}
