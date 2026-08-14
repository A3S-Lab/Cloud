use super::validation::{
    required_string, validate_dotted_identifier, validate_exact_semver, validate_identifier,
};
use super::WorkflowVariableContract;
use crate::modules::shared_kernel::domain::{canonical_json_bounded, sha256_digest, Sha256Digest};
use a3s_acl::builder::{string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const WORKFLOW_VARIABLE_DEFAULTS_SCHEMA: &str = "cloud.workflow.variable-defaults.v1";
pub const WORKFLOW_VARIABLE_DEFAULTS_MAX_ACL_BYTES: usize = 2 * 1024 * 1024;
pub const WORKFLOW_VARIABLE_DEFAULT_MAX_VALUE_BYTES: usize = 256 * 1024;
const MAX_DEFAULTS: usize = 1_024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowVariableDefault {
    pub name: String,
    pub value: Value,
    pub digest: Sha256Digest,
}

impl WorkflowVariableDefault {
    pub fn new(name: impl Into<String>, value: Value) -> Result<Self, String> {
        let canonical = canonical_default_bytes(&value)?;
        Ok(Self {
            name: name.into(),
            value,
            digest: Sha256Digest::parse(sha256_digest(&canonical))?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowVariableDefaultsSpec {
    pub id: String,
    pub revision: String,
    pub values: Vec<WorkflowVariableDefault>,
}

/// Immutable default material for one Workflow variable contract.
///
/// The variable contract retains only value digests. This separately persisted
/// revision child supplies the exact canonical JSON bytes needed for replay; it
/// is configuration material, not mutable run state or a variable store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowVariableDefaults {
    spec: WorkflowVariableDefaultsSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl WorkflowVariableDefaults {
    pub fn from_spec(mut spec: WorkflowVariableDefaultsSpec) -> Result<Self, String> {
        validate_dotted_identifier("Workflow variable defaults ID", &spec.id)?;
        validate_exact_semver("Workflow variable defaults revision", &spec.revision)?;
        if spec.values.is_empty() || spec.values.len() > MAX_DEFAULTS {
            return Err("Workflow variable defaults bounds are invalid".into());
        }
        for value in &spec.values {
            validate_identifier("Workflow variable default name", &value.name)?;
            let canonical = canonical_default_bytes(&value.value)?;
            if sha256_digest(&canonical) != value.digest.as_str() {
                return Err(format!(
                    "Workflow variable default {:?} digest does not match its canonical value",
                    value.name
                ));
            }
        }
        spec.values
            .sort_by(|left, right| left.name.cmp(&right.name));
        let unique = spec
            .values
            .iter()
            .map(|value| value.name.as_str())
            .collect::<BTreeSet<_>>();
        if unique.len() != spec.values.len() {
            return Err("Workflow variable defaults contain duplicate names".into());
        }

        let document = defaults_document(&spec)?;
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > WORKFLOW_VARIABLE_DEFAULTS_MAX_ACL_BYTES {
            return Err("Workflow variable defaults ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl).map_err(|error| {
            format!("generated Workflow variable defaults ACL is invalid: {error}")
        })?;
        let digest = digest_document(&reparsed)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > WORKFLOW_VARIABLE_DEFAULTS_MAX_ACL_BYTES {
            return Err("Workflow variable defaults ACL size is invalid".into());
        }
        if source.replace("\r\n", "").contains('\r') {
            return Err("Workflow variable defaults contain a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let value = Self::from_spec(parse_defaults_spec(&normalized)?)?;
        if value.canonical_acl != normalized {
            return Err("Workflow variable defaults ACL is not canonical".into());
        }
        Ok(value)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let value = Self::parse_acl(source)?;
        if value.digest.as_str() != stored_digest {
            return Err("stored Workflow variable defaults and digest do not match".into());
        }
        Ok(value)
    }

    pub fn validate_contract(&self, contract: &WorkflowVariableContract) -> Result<(), String> {
        if self.spec.id != contract.id() || self.spec.revision != contract.revision() {
            return Err("Workflow variable defaults identity does not match its contract".into());
        }
        let declarations = contract
            .spec()
            .declarations
            .iter()
            .filter_map(|declaration| {
                declaration
                    .default_value_digest
                    .as_ref()
                    .map(|digest| (declaration.name.as_str(), (declaration, digest)))
            })
            .collect::<BTreeMap<_, _>>();
        if declarations.len() != self.spec.values.len() {
            return Err(
                "Workflow variable defaults must exactly cover digest-backed declarations".into(),
            );
        }
        for value in &self.spec.values {
            let (declaration, expected_digest) =
                declarations.get(value.name.as_str()).ok_or_else(|| {
                    format!(
                        "Workflow variable default {:?} has no digest-backed declaration",
                        value.name
                    )
                })?;
            if value.digest.as_str() != expected_digest.as_str() {
                return Err(format!(
                    "Workflow variable default {:?} does not match its declared digest",
                    value.name
                ));
            }
            if !declaration.value_type.matches_json_value(&value.value) {
                return Err(format!(
                    "Workflow variable default {:?} does not match {}",
                    value.name,
                    declaration.value_type.as_str()
                ));
            }
        }
        Ok(())
    }

    pub fn value(&self, name: &str) -> Option<&Value> {
        self.spec
            .values
            .binary_search_by(|value| value.name.as_str().cmp(name))
            .ok()
            .map(|index| &self.spec.values[index].value)
    }

    pub const fn spec(&self) -> &WorkflowVariableDefaultsSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn canonical_default_bytes(value: &Value) -> Result<Vec<u8>, String> {
    canonical_json_bounded(
        value,
        WORKFLOW_VARIABLE_DEFAULT_MAX_VALUE_BYTES,
        "Workflow variable default",
    )
}

fn defaults_document(spec: &WorkflowVariableDefaultsSpec) -> Result<Document, String> {
    let mut root = BlockBuilder::new("variable_defaults")
        .label(&spec.id)
        .attr("schema", string(WORKFLOW_VARIABLE_DEFAULTS_SCHEMA))
        .attr("revision", string(&spec.revision));
    for value in &spec.values {
        let canonical_json = String::from_utf8(canonical_default_bytes(&value.value)?)
            .map_err(|_| "Workflow variable default JSON is not UTF-8".to_owned())?;
        root = root.nested_block(
            BlockBuilder::new("default")
                .label(&value.name)
                .attr("canonical_json", string(&canonical_json))
                .attr("digest", string(value.digest.as_str()))
                .build(),
        );
    }
    Ok(Document {
        blocks: vec![root.build()],
    })
}

fn parse_defaults_spec(source: &str) -> Result<WorkflowVariableDefaultsSpec, String> {
    let document = parse_acl(source)
        .map_err(|error| format!("Workflow variable defaults ACL is invalid: {error}"))?;
    if document.blocks.len() != 1 {
        return Err("Workflow variable defaults require exactly one root block".into());
    }
    let root = &document.blocks[0];
    exact_block(root, "variable_defaults", &["revision", "schema"], 1, true)?;
    if required_string(root, "schema")? != WORKFLOW_VARIABLE_DEFAULTS_SCHEMA {
        return Err("Workflow variable defaults schema is unsupported".into());
    }
    if root.blocks.iter().any(|block| block.name != "default") {
        return Err("Workflow variable defaults contain an unknown block".into());
    }
    Ok(WorkflowVariableDefaultsSpec {
        id: root.labels[0].clone(),
        revision: required_string(root, "revision")?,
        values: root
            .blocks
            .iter()
            .map(parse_default)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn parse_default(block: &Block) -> Result<WorkflowVariableDefault, String> {
    exact_block(block, "default", &["canonical_json", "digest"], 1, false)?;
    let canonical_json = required_string(block, "canonical_json")?;
    let value = serde_json::from_str::<Value>(&canonical_json)
        .map_err(|error| format!("Workflow variable default JSON is invalid: {error}"))?;
    let parsed = WorkflowVariableDefault {
        name: block.labels[0].clone(),
        value,
        digest: Sha256Digest::parse(required_string(block, "digest")?)?,
    };
    let encoded = String::from_utf8(canonical_default_bytes(&parsed.value)?)
        .map_err(|_| "Workflow variable default JSON is not UTF-8".to_owned())?;
    if encoded != canonical_json {
        return Err("Workflow variable default JSON is not canonical".into());
    }
    Ok(parsed)
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
            "Workflow variable defaults {name} block shape is invalid"
        ));
    }
    Ok(())
}

fn digest_document(document: &Document) -> Result<Sha256Digest, String> {
    Sha256Digest::parse(
        canonical_digest(document).map_err(|error| {
            format!("Workflow variable defaults are not canonicalizable: {error}")
        })?,
    )
}
