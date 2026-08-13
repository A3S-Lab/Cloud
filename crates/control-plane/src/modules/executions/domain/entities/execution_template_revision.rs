use super::{ExecutionArtifact, ExecutionProcess, ExecutionResources, ExecutionTemplate};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ExecutionTemplateId, ExecutionTemplateRevisionId, OrganizationId,
    PrincipalId, ProjectId, Sha256Digest,
};
use a3s_acl::builder::{integer, list, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const EXECUTION_TEMPLATE_SCHEMA: &str = "cloud.execution-template.v1";
pub const EXECUTION_TEMPLATE_CAPABILITY: &str = "execution.run";
pub const EXECUTION_TEMPLATE_MAX_ACL_BYTES: usize = 128 * 1024;
const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;

/// Reusable, immutable execution configuration. Invocation input is deliberately
/// absent: a Workflow supplies its schema-checked effective input when this
/// definition is materialized as a one-shot [`ExecutionTemplate`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionTemplateDefinitionSpec {
    pub name: String,
    pub description: String,
    pub artifact: ExecutionArtifact,
    pub process: ExecutionProcess,
    pub resources: ExecutionResources,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionTemplateDefinition {
    spec: ExecutionTemplateDefinitionSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl ExecutionTemplateDefinition {
    pub fn from_spec(spec: ExecutionTemplateDefinitionSpec) -> Result<Self, String> {
        validate_definition_spec(&spec)?;
        let document = definition_document(&spec)?;
        let canonical_acl = generate_acl(&document);
        if canonical_acl.len() > EXECUTION_TEMPLATE_MAX_ACL_BYTES {
            return Err("ExecutionTemplate ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated ExecutionTemplate ACL is invalid: {error}"))?;
        let digest = Sha256Digest::parse(
            canonical_digest(&reparsed)
                .map_err(|error| format!("ExecutionTemplate is not canonicalizable: {error}"))?,
        )?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(acl: &str) -> Result<Self, String> {
        if acl.is_empty() || acl.len() > EXECUTION_TEMPLATE_MAX_ACL_BYTES {
            return Err("ExecutionTemplate ACL size is invalid".into());
        }
        let document =
            parse_acl(acl).map_err(|error| format!("ExecutionTemplate ACL is invalid: {error}"))?;
        Self::from_spec(parse_definition(&document)?)
    }

    pub fn restore(acl: &str, stored_digest: &str) -> Result<Self, String> {
        let definition = Self::parse_acl(acl)?;
        if definition.canonical_acl != acl || definition.digest.as_str() != stored_digest {
            return Err("stored ExecutionTemplate ACL and digest do not match".into());
        }
        Ok(definition)
    }

    pub const fn spec(&self) -> &ExecutionTemplateDefinitionSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub fn materialize(&self, input: serde_json::Value) -> Result<ExecutionTemplate, String> {
        let template = ExecutionTemplate {
            artifact: self.spec.artifact.clone(),
            process: self.spec.process.clone(),
            input,
            resources: self.spec.resources.clone(),
        };
        template.validate()?;
        Ok(template)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionTemplateRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub template_id: ExecutionTemplateId,
    pub revision_id: ExecutionTemplateRevisionId,
    pub definition: ExecutionTemplateDefinition,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
}

impl ExecutionTemplateRevision {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        organization_id: OrganizationId,
        project_id: ProjectId,
        template_id: ExecutionTemplateId,
        revision_id: ExecutionTemplateRevisionId,
        definition: ExecutionTemplateDefinition,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            project_id,
            template_id,
            revision_id,
            definition,
            created_by,
            created_at: canonical_timestamp(created_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn restore(mut self) -> Result<Self, String> {
        self.definition = ExecutionTemplateDefinition::restore(
            self.definition.canonical_acl(),
            self.definition.digest().as_str(),
        )?;
        self.created_at = canonical_timestamp(self.created_at);
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.template_id.as_uuid().is_nil()
            || self.revision_id.as_uuid().is_nil()
            || self.created_by.as_uuid().is_nil()
        {
            return Err("ExecutionTemplate revision identity is invalid".into());
        }
        let restored = ExecutionTemplateDefinition::restore(
            self.definition.canonical_acl(),
            self.definition.digest().as_str(),
        )?;
        if restored != self.definition {
            return Err("ExecutionTemplate revision definition drifted".into());
        }
        Ok(())
    }
}

fn validate_definition_spec(spec: &ExecutionTemplateDefinitionSpec) -> Result<(), String> {
    if spec.name.is_empty()
        || spec.name.len() > 120
        || !spec.name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
        || spec.description.chars().count() > 4_096
        || spec.description.contains('\0')
    {
        return Err("ExecutionTemplate name or description is invalid".into());
    }
    ExecutionTemplate {
        artifact: spec.artifact.clone(),
        process: spec.process.clone(),
        input: serde_json::Value::Null,
        resources: spec.resources.clone(),
    }
    .validate()
}

fn definition_document(spec: &ExecutionTemplateDefinitionSpec) -> Result<Document, String> {
    let mut process = BlockBuilder::new("process")
        .attr(
            "command",
            list(
                spec.process
                    .command
                    .iter()
                    .map(|value| string(value))
                    .collect(),
            ),
        )
        .attr(
            "args",
            list(
                spec.process
                    .args
                    .iter()
                    .map(|value| string(value))
                    .collect(),
            ),
        );
    if let Some(directory) = &spec.process.working_directory {
        process = process.attr("working_directory", string(directory));
    }
    for (name, value) in &spec.process.environment {
        process = process.nested_block(
            BlockBuilder::new("environment")
                .label(name)
                .attr("value", string(value))
                .build(),
        );
    }
    let mut resources = BlockBuilder::new("resources")
        .attr(
            "cpu_millis",
            acl_integer("cpu_millis", spec.resources.cpu_millis)?,
        )
        .attr(
            "memory_bytes",
            acl_integer("memory_bytes", spec.resources.memory_bytes)?,
        )
        .attr("pids", acl_integer("pids", u64::from(spec.resources.pids))?)
        .attr(
            "timeout_ms",
            acl_integer("timeout_ms", spec.resources.timeout_ms)?,
        );
    if let Some(bytes) = spec.resources.ephemeral_storage_bytes {
        resources = resources.attr(
            "ephemeral_storage_bytes",
            acl_integer("ephemeral_storage_bytes", bytes)?,
        );
    }
    Ok(Document {
        blocks: vec![BlockBuilder::new("execution_template")
            .label(&spec.name)
            .attr("schema", string(EXECUTION_TEMPLATE_SCHEMA))
            .attr("description", string(&spec.description))
            .nested_block(
                BlockBuilder::new("artifact")
                    .attr("uri", string(&spec.artifact.uri))
                    .attr("digest", string(&spec.artifact.digest))
                    .attr("media_type", string(&spec.artifact.media_type))
                    .build(),
            )
            .nested_block(process.build())
            .nested_block(resources.build())
            .build()],
    })
}

fn parse_definition(document: &Document) -> Result<ExecutionTemplateDefinitionSpec, String> {
    if document.blocks.len() != 1 {
        return Err("ExecutionTemplate must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    exact_block(root, "execution_template", &["schema", "description"], 1, 3)?;
    if root.labels.len() != 1 || required_string(root, "schema")? != EXECUTION_TEMPLATE_SCHEMA {
        return Err("ExecutionTemplate root identity or schema is invalid".into());
    }
    let artifact = exact_nested(root, "artifact")?;
    exact_block(artifact, "artifact", &["uri", "digest", "media_type"], 0, 0)?;
    let process = exact_nested(root, "process")?;
    if !process.labels.is_empty()
        || process.attributes.len() < 2
        || process.attributes.len() > 3
        || process
            .attributes
            .keys()
            .any(|key| !["command", "args", "working_directory"].contains(&key.as_str()))
        || process
            .blocks
            .iter()
            .any(|block| block.name != "environment")
    {
        return Err("ExecutionTemplate process block shape is invalid".into());
    }
    let mut environment = BTreeMap::new();
    for variable in &process.blocks {
        exact_block(variable, "environment", &["value"], 1, 0)?;
        if environment
            .insert(
                variable.labels[0].clone(),
                required_string(variable, "value")?,
            )
            .is_some()
        {
            return Err("ExecutionTemplate contains duplicate environment variables".into());
        }
    }
    let resources = exact_nested(root, "resources")?;
    let resource_fields = [
        "cpu_millis",
        "memory_bytes",
        "pids",
        "timeout_ms",
        "ephemeral_storage_bytes",
    ];
    if !resources.labels.is_empty()
        || !resources.blocks.is_empty()
        || resources.attributes.len() < 4
        || resources.attributes.len() > 5
        || resources
            .attributes
            .keys()
            .any(|key| !resource_fields.contains(&key.as_str()))
    {
        return Err("ExecutionTemplate resources block shape is invalid".into());
    }
    Ok(ExecutionTemplateDefinitionSpec {
        name: root.labels[0].clone(),
        description: required_string(root, "description")?,
        artifact: ExecutionArtifact {
            uri: required_string(artifact, "uri")?,
            digest: required_string(artifact, "digest")?,
            media_type: required_string(artifact, "media_type")?,
        },
        process: ExecutionProcess {
            command: required_strings(process, "command")?,
            args: required_strings(process, "args")?,
            working_directory: optional_string(process, "working_directory")?,
            environment,
        },
        resources: ExecutionResources {
            cpu_millis: required_u64(resources, "cpu_millis")?,
            memory_bytes: required_u64(resources, "memory_bytes")?,
            pids: u32::try_from(required_u64(resources, "pids")?)
                .map_err(|_| "ExecutionTemplate pids exceeds u32".to_owned())?,
            ephemeral_storage_bytes: optional_u64(resources, "ephemeral_storage_bytes")?,
            timeout_ms: required_u64(resources, "timeout_ms")?,
        },
    })
}

fn exact_nested<'a>(root: &'a Block, name: &str) -> Result<&'a Block, String> {
    let mut matching = root.blocks.iter().filter(|block| block.name == name);
    let value = matching
        .next()
        .ok_or_else(|| format!("ExecutionTemplate {name} block is required"))?;
    if matching.next().is_some() {
        return Err(format!("ExecutionTemplate {name} block must be unique"));
    }
    Ok(value)
}

fn exact_block(
    block: &Block,
    name: &str,
    attributes: &[&str],
    labels: usize,
    nested: usize,
) -> Result<(), String> {
    if block.name != name
        || block.labels.len() != labels
        || block.attributes.len() != attributes.len()
        || block
            .attributes
            .keys()
            .any(|key| !attributes.contains(&key.as_str()))
        || (nested != 0 && block.blocks.len() != nested)
        || (nested == 0 && name != "execution_template" && !block.blocks.is_empty())
    {
        return Err(format!("ExecutionTemplate {name} block shape is invalid"));
    }
    Ok(())
}

fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("ExecutionTemplate field {name:?} is required"))
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("ExecutionTemplate field {name:?} must be a string"))
}

fn optional_string(block: &Block, name: &str) -> Result<Option<String>, String> {
    block
        .attributes
        .get(name)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("ExecutionTemplate field {name:?} must be a string"))
        })
        .transpose()
}

fn required_strings(block: &Block, name: &str) -> Result<Vec<String>, String> {
    let Value::List(values) = required_value(block, name)? else {
        return Err(format!(
            "ExecutionTemplate field {name:?} must be a string list"
        ));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("ExecutionTemplate field {name:?} must be a string list"))
        })
        .collect()
}

fn required_u64(block: &Block, name: &str) -> Result<u64, String> {
    let value = required_value(block, name)?
        .as_number()
        .ok_or_else(|| format!("ExecutionTemplate field {name:?} must be an integer"))?;
    if !value.is_finite()
        || value.fract() != 0.0
        || value <= 0.0
        || value > MAX_SAFE_ACL_INTEGER as f64
    {
        return Err(format!(
            "ExecutionTemplate field {name:?} must be a positive exactly representable integer"
        ));
    }
    Ok(value as u64)
}

fn optional_u64(block: &Block, name: &str) -> Result<Option<u64>, String> {
    block
        .attributes
        .get(name)
        .map(|_| required_u64(block, name))
        .transpose()
}

fn acl_integer(name: &str, value: u64) -> Result<Value, String> {
    if value == 0 || value > MAX_SAFE_ACL_INTEGER {
        return Err(format!(
            "ExecutionTemplate field {name:?} is not representable by ACL"
        ));
    }
    Ok(integer(value as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ExecutionTemplateDefinitionSpec {
        let digest = format!("sha256:{}", "a".repeat(64));
        ExecutionTemplateDefinitionSpec {
            name: "echo-task".into(),
            description: "Runs one bounded echo task".into(),
            artifact: ExecutionArtifact {
                uri: format!("oci://registry.example/tasks/echo@{digest}"),
                digest,
                media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            },
            process: ExecutionProcess {
                command: vec!["/bin/echo".into()],
                args: vec!["invoke".into()],
                working_directory: Some("/workspace".into()),
                environment: BTreeMap::from([("MODE".into(), "workflow".into())]),
            },
            resources: ExecutionResources {
                cpu_millis: 100,
                memory_bytes: 64 * 1024 * 1024,
                pids: 32,
                ephemeral_storage_bytes: Some(1024 * 1024),
                timeout_ms: 30_000,
            },
        }
    }

    #[test]
    fn definition_is_acl_native_canonical_and_materializes_only_invocation_input() {
        let definition = ExecutionTemplateDefinition::from_spec(spec()).expect("definition");
        assert!(definition
            .canonical_acl()
            .contains("execution_template \"echo-task\""));
        assert_eq!(
            ExecutionTemplateDefinition::parse_acl(definition.canonical_acl()).expect("reparse"),
            definition
        );
        let invocation = definition
            .materialize(serde_json::json!({"ticket": "T-42"}))
            .expect("invocation");
        assert_eq!(invocation.input, serde_json::json!({"ticket": "T-42"}));
        assert_eq!(invocation.artifact, definition.spec().artifact);
    }

    #[test]
    fn shared_w0_3_execution_template_contract_uses_the_owner_parser() {
        let definition = ExecutionTemplateDefinition::parse_acl(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/w0.3/execution-template.acl"
        )))
        .expect("shared W0.3 ExecutionTemplate ACL");
        assert_eq!(definition.spec().name, "workflow-release-check");
        assert_eq!(definition.spec().resources.timeout_ms, 30_000);
        assert!(definition
            .canonical_acl()
            .contains("execution_template \"workflow-release-check\""));
        assert_eq!(
            ExecutionTemplateDefinition::parse_acl(definition.canonical_acl())
                .expect("canonical shared ExecutionTemplate ACL"),
            definition
        );
    }

    #[test]
    fn definition_rejects_unknown_non_acl_or_digest_drift() {
        let definition = ExecutionTemplateDefinition::from_spec(spec()).expect("definition");
        let unknown = definition
            .canonical_acl()
            .replace("description =", "unknown = \"x\"\n  description =");
        assert!(ExecutionTemplateDefinition::parse_acl(&unknown).is_err());
        assert!(ExecutionTemplateDefinition::restore(
            definition.canonical_acl(),
            &format!("sha256:{}", "f".repeat(64))
        )
        .is_err());
    }
}
