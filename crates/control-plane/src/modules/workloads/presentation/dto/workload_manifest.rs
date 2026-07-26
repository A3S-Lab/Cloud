use super::service_template::{
    HttpHealthCheckDto, OciArtifactReferenceDto, SecretBindingDto, SecretBindingTargetDto,
    ServicePortDto, ServiceProcessDto, ServiceResourcesDto, ServiceTemplateDto,
    SourceWorkloadTemplateDto,
};
use a3s_acl::{
    parse_with_limits, validate_document_with_limits, AttributeSchema, Block, BlockSchema,
    Cardinality, Document, ParseLimits, Schema, Value, ValueSchema,
};
use a3s_boot::{BootError, Result};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub(crate) const A3S_ACL_MEDIA_TYPE: &str = "application/vnd.a3s.acl";

const MANIFEST_VERSION: u64 = 1;
const MANIFEST_MAX_BYTES: usize = 64 * 1024;
const MANIFEST_MAX_NESTING_DEPTH: usize = 16;
const MANIFEST_MAX_COLLECTION_ITEMS: usize = 512;
const MANIFEST_MAX_TOKEN_BYTES: usize = 16 * 1024;
const MANIFEST_MAX_DIAGNOSTICS: usize = 20;
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkloadManifest {
    pub name: String,
    pub template: ServiceTemplateDto,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceWorkloadManifest {
    pub name: String,
    pub template: SourceWorkloadTemplateDto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArtifactPolicy {
    Required,
    Forbidden,
}

struct ParsedManifest {
    name: String,
    artifact: Option<OciArtifactReferenceDto>,
    process: ServiceProcessDto,
    secrets: Vec<SecretBindingDto>,
    resources: ServiceResourcesDto,
    ports: Vec<ServicePortDto>,
    health: HttpHealthCheckDto,
}

pub(crate) fn parse_workload_manifest(source: &[u8]) -> Result<WorkloadManifest> {
    let manifest = parse_manifest(source, ArtifactPolicy::Required)?;
    let artifact = manifest.artifact.ok_or_else(|| {
        BootError::BadRequest("workload ACL must declare one artifact block".into())
    })?;
    Ok(WorkloadManifest {
        name: manifest.name,
        template: ServiceTemplateDto {
            artifact,
            process: manifest.process,
            secrets: manifest.secrets,
            resources: manifest.resources,
            ports: manifest.ports,
            health: manifest.health,
        },
    })
}

pub(crate) fn parse_source_workload_manifest(source: &[u8]) -> Result<SourceWorkloadManifest> {
    let manifest = parse_manifest(source, ArtifactPolicy::Forbidden)?;
    Ok(SourceWorkloadManifest {
        name: manifest.name,
        template: SourceWorkloadTemplateDto {
            process: manifest.process,
            secrets: manifest.secrets,
            resources: manifest.resources,
            ports: manifest.ports,
            health: manifest.health,
        },
    })
}

fn parse_manifest(source: &[u8], artifact_policy: ArtifactPolicy) -> Result<ParsedManifest> {
    let source = std::str::from_utf8(source)
        .map_err(|_| BootError::BadRequest("workload ACL must be valid UTF-8".into()))?;
    let limits = manifest_limits();
    let document = parse_with_limits(source, limits).map_err(|error| {
        BootError::BadRequest(format!(
            "invalid workload ACL: {} at line {}, column {}",
            error.code.as_str(),
            error.line,
            error.column
        ))
    })?;
    validate_manifest_schema(&document, artifact_policy, limits)?;
    let version = document_attribute(&document, "version")
        .and_then(|value| unsigned_integer(value, "version"))?;
    if version != MANIFEST_VERSION {
        return Err(BootError::BadRequest(format!(
            "workload ACL version must be {MANIFEST_VERSION}"
        )));
    }

    let workload = one_document_block(&document, "workload")?;
    let name = workload.labels.first().cloned().ok_or_else(|| {
        BootError::BadRequest("workload ACL must declare one workload name".into())
    })?;
    let artifact = match artifact_policy {
        ArtifactPolicy::Required => Some(parse_artifact(one_block(workload, "artifact")?)?),
        ArtifactPolicy::Forbidden => None,
    };
    let process = optional_block(workload, "process")?
        .map(parse_process)
        .transpose()?
        .unwrap_or_default();
    let secrets = parse_secrets(workload)?;
    let resources = parse_resources(one_block(workload, "resources")?)?;
    let ports = parse_ports(workload)?;
    let health = parse_health(one_block(workload, "health")?)?;
    Ok(ParsedManifest {
        name,
        artifact,
        process,
        secrets,
        resources,
        ports,
        health,
    })
}

fn manifest_limits() -> ParseLimits {
    ParseLimits {
        max_document_bytes: MANIFEST_MAX_BYTES,
        max_nesting_depth: MANIFEST_MAX_NESTING_DEPTH,
        max_collection_items: MANIFEST_MAX_COLLECTION_ITEMS,
        max_token_bytes: MANIFEST_MAX_TOKEN_BYTES,
        max_diagnostics: MANIFEST_MAX_DIAGNOSTICS,
    }
}

fn validate_manifest_schema(
    document: &Document,
    artifact_policy: ArtifactPolicy,
    limits: ParseLimits,
) -> Result<()> {
    let report = validate_document_with_limits(document, &manifest_schema(artifact_policy), limits);
    if report.is_empty() {
        return Ok(());
    }
    let first = report.diagnostics.first().ok_or_else(|| {
        BootError::BadRequest("invalid workload ACL: diagnostic budget exhausted".into())
    })?;
    let remaining = report.diagnostics.len().saturating_sub(1);
    let suffix = if remaining == 0 && !report.truncated {
        String::new()
    } else {
        format!("; {remaining} additional diagnostics")
    };
    Err(BootError::BadRequest(format!(
        "invalid workload ACL: {} at {}{suffix}",
        first.code.as_str(),
        first.path
    )))
}

fn manifest_schema(artifact_policy: ArtifactPolicy) -> Schema {
    let process = Schema::new()
        .attribute(
            "command",
            AttributeSchema::optional(ValueSchema::list(ValueSchema::string())),
        )
        .attribute(
            "args",
            AttributeSchema::optional(ValueSchema::list(ValueSchema::string())),
        )
        .attribute(
            "working_directory",
            AttributeSchema::optional(ValueSchema::string()),
        )
        .block(
            "environment",
            BlockSchema::new(
                Schema::new().attribute("value", AttributeSchema::required(ValueSchema::string())),
            )
            .labels(Cardinality::exactly(1)),
        );
    let resources = Schema::new()
        .attribute(
            "cpu_millis",
            AttributeSchema::required(ValueSchema::number()),
        )
        .attribute(
            "memory_bytes",
            AttributeSchema::required(ValueSchema::number()),
        )
        .attribute("pids", AttributeSchema::required(ValueSchema::number()))
        .attribute(
            "ephemeral_storage_bytes",
            AttributeSchema::optional(ValueSchema::number()),
        );
    let health = Schema::new()
        .attribute(
            "port_name",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute("path", AttributeSchema::required(ValueSchema::string()))
        .attribute(
            "interval_ms",
            AttributeSchema::required(ValueSchema::number()),
        )
        .attribute(
            "timeout_ms",
            AttributeSchema::required(ValueSchema::number()),
        )
        .attribute(
            "healthy_threshold",
            AttributeSchema::required(ValueSchema::number()),
        )
        .attribute(
            "unhealthy_threshold",
            AttributeSchema::required(ValueSchema::number()),
        )
        .attribute(
            "stabilization_window_ms",
            AttributeSchema::required(ValueSchema::number()),
        );
    let secret = Schema::new()
        .attribute(
            "secret_id",
            AttributeSchema::required(ValueSchema::string()),
        )
        .attribute("version", AttributeSchema::required(ValueSchema::number()))
        .block(
            "environment",
            optional_one(
                Schema::new()
                    .attribute("variable", AttributeSchema::required(ValueSchema::string())),
            ),
        )
        .block(
            "file",
            optional_one(
                Schema::new()
                    .attribute("path", AttributeSchema::required(ValueSchema::string()))
                    .attribute("mode", AttributeSchema::required(ValueSchema::number())),
            ),
        )
        .block("registry_credential", optional_one(Schema::new()));
    let mut workload = Schema::new()
        .block("process", optional_one(process))
        .block("resources", required_one(resources))
        .block(
            "port",
            BlockSchema::new(Schema::new().attribute(
                "container_port",
                AttributeSchema::required(ValueSchema::number()),
            ))
            .occurrences(Cardinality::at_least(1))
            .labels(Cardinality::exactly(1)),
        )
        .block("health", required_one(health))
        .block(
            "secret",
            BlockSchema::new(secret).labels(Cardinality::exactly(1)),
        );
    if artifact_policy == ArtifactPolicy::Required {
        workload = workload.block(
            "artifact",
            required_one(
                Schema::new()
                    .attribute("uri", AttributeSchema::required(ValueSchema::string()))
                    .attribute(
                        "expected_digest",
                        AttributeSchema::optional(ValueSchema::string()),
                    ),
            ),
        );
    }
    Schema::new()
        .attribute("version", AttributeSchema::required(ValueSchema::number()))
        .block(
            "workload",
            required_one(workload).labels(Cardinality::exactly(1)),
        )
}

fn required_one(schema: Schema) -> BlockSchema {
    BlockSchema::new(schema).occurrences(Cardinality::exactly(1))
}

fn optional_one(schema: Schema) -> BlockSchema {
    BlockSchema::new(schema)
        .occurrences(Cardinality::new(0, Some(1)).expect("zero-or-one cardinality is valid"))
}

fn parse_artifact(block: &Block) -> Result<OciArtifactReferenceDto> {
    Ok(OciArtifactReferenceDto {
        uri: required_string(block, "uri")?,
        expected_digest: optional_string(block, "expected_digest")?,
    })
}

fn parse_process(block: &Block) -> Result<ServiceProcessDto> {
    let mut environment = BTreeMap::new();
    for variable in named_blocks(block, "environment") {
        let name = one_label(variable, "process environment")?.to_owned();
        if environment
            .insert(name, required_string(variable, "value")?)
            .is_some()
        {
            return Err(BootError::BadRequest(
                "workload ACL process environment names must be unique".into(),
            ));
        }
    }
    Ok(ServiceProcessDto {
        command: optional_string_list(block, "command")?.unwrap_or_default(),
        args: optional_string_list(block, "args")?.unwrap_or_default(),
        working_directory: optional_string(block, "working_directory")?,
        environment,
    })
}

fn parse_secrets(workload: &Block) -> Result<Vec<SecretBindingDto>> {
    let mut names = BTreeSet::new();
    named_blocks(workload, "secret")
        .map(|block| {
            let name = one_label(block, "secret")?.to_owned();
            if !names.insert(name.clone()) {
                return Err(BootError::BadRequest(
                    "workload ACL secret names must be unique".into(),
                ));
            }
            let secret_id =
                Uuid::parse_str(&required_string(block, "secret_id")?).map_err(|_| {
                    BootError::BadRequest("workload ACL secret_id must be a UUID".into())
                })?;
            let version = required_u64(block, "version")?;
            let environment_target = optional_block(block, "environment")?;
            let file_target = optional_block(block, "file")?;
            let registry_target = optional_block(block, "registry_credential")?;
            let targets = [environment_target, file_target, registry_target];
            if targets.iter().flatten().count() != 1 {
                return Err(BootError::BadRequest(
                    "workload ACL secret must declare exactly one target block".into(),
                ));
            }
            let target = if let Some(target) = targets[0] {
                SecretBindingTargetDto::Environment {
                    variable: required_string(target, "variable")?,
                }
            } else if let Some(target) = targets[1] {
                SecretBindingTargetDto::File {
                    path: required_string(target, "path")?,
                    mode: required_u32(target, "mode")?,
                }
            } else {
                SecretBindingTargetDto::RegistryCredential
            };
            Ok(SecretBindingDto {
                name,
                secret_id,
                version,
                target,
            })
        })
        .collect()
}

fn parse_resources(block: &Block) -> Result<ServiceResourcesDto> {
    Ok(ServiceResourcesDto {
        cpu_millis: required_u64(block, "cpu_millis")?,
        memory_bytes: required_u64(block, "memory_bytes")?,
        pids: required_u32(block, "pids")?,
        ephemeral_storage_bytes: optional_u64(block, "ephemeral_storage_bytes")?,
    })
}

fn parse_ports(workload: &Block) -> Result<Vec<ServicePortDto>> {
    let mut names = BTreeSet::new();
    named_blocks(workload, "port")
        .map(|block| {
            let name = one_label(block, "port")?.to_owned();
            if !names.insert(name.clone()) {
                return Err(BootError::BadRequest(
                    "workload ACL port names must be unique".into(),
                ));
            }
            Ok(ServicePortDto {
                name,
                container_port: required_u16(block, "container_port")?,
            })
        })
        .collect()
}

fn parse_health(block: &Block) -> Result<HttpHealthCheckDto> {
    Ok(HttpHealthCheckDto {
        port_name: required_string(block, "port_name")?,
        path: required_string(block, "path")?,
        interval_ms: required_u64(block, "interval_ms")?,
        timeout_ms: required_u64(block, "timeout_ms")?,
        healthy_threshold: required_u16(block, "healthy_threshold")?,
        unhealthy_threshold: required_u16(block, "unhealthy_threshold")?,
        stabilization_window_ms: required_u64(block, "stabilization_window_ms")?,
    })
}

fn document_attribute<'a>(document: &'a Document, name: &str) -> Result<&'a Value> {
    document
        .blocks
        .iter()
        .find_map(|block| bare_attribute(block, name))
        .ok_or_else(|| BootError::BadRequest(format!("workload ACL attribute {name} is required")))
}

fn bare_attribute<'a>(block: &'a Block, name: &str) -> Option<&'a Value> {
    (block.name == name
        && block.labels.is_empty()
        && block.blocks.is_empty()
        && block.attributes.len() == 1)
        .then(|| block.attributes.get(name))
        .flatten()
}

fn one_document_block<'a>(document: &'a Document, name: &str) -> Result<&'a Block> {
    document
        .blocks
        .iter()
        .find(|block| block.name == name && bare_attribute(block, name).is_none())
        .ok_or_else(|| BootError::BadRequest(format!("workload ACL block {name} is required")))
}

fn one_block<'a>(parent: &'a Block, name: &str) -> Result<&'a Block> {
    optional_block(parent, name)?
        .ok_or_else(|| BootError::BadRequest(format!("workload ACL block {name} is required")))
}

fn optional_block<'a>(parent: &'a Block, name: &str) -> Result<Option<&'a Block>> {
    let mut matches = named_blocks(parent, name);
    let first = matches.next();
    if matches.next().is_some() {
        return Err(BootError::BadRequest(format!(
            "workload ACL block {name} may appear only once"
        )));
    }
    Ok(first)
}

fn named_blocks<'a>(parent: &'a Block, name: &str) -> impl Iterator<Item = &'a Block> {
    let name = name.to_owned();
    parent.blocks.iter().filter(move |block| block.name == name)
}

fn one_label<'a>(block: &'a Block, context: &str) -> Result<&'a str> {
    block
        .labels
        .first()
        .map(String::as_str)
        .ok_or_else(|| BootError::BadRequest(format!("workload ACL {context} name is required")))
}

fn required_string(block: &Block, name: &str) -> Result<String> {
    optional_string(block, name)?
        .ok_or_else(|| BootError::BadRequest(format!("workload ACL attribute {name} is required")))
}

fn optional_string(block: &Block, name: &str) -> Result<Option<String>> {
    block
        .attributes
        .get(name)
        .map(|value| match value {
            Value::String(value) => Ok(value.clone()),
            _ => Err(BootError::BadRequest(format!(
                "workload ACL attribute {name} must be a string"
            ))),
        })
        .transpose()
}

fn optional_string_list(block: &Block, name: &str) -> Result<Option<Vec<String>>> {
    block
        .attributes
        .get(name)
        .map(|value| match value {
            Value::List(values) => values
                .iter()
                .map(|value| match value {
                    Value::String(value) => Ok(value.clone()),
                    _ => Err(BootError::BadRequest(format!(
                        "workload ACL attribute {name} must contain only strings"
                    ))),
                })
                .collect(),
            _ => Err(BootError::BadRequest(format!(
                "workload ACL attribute {name} must be a list"
            ))),
        })
        .transpose()
}

fn required_u64(block: &Block, name: &str) -> Result<u64> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| BootError::BadRequest(format!("workload ACL attribute {name} is required")))
        .and_then(|value| unsigned_integer(value, name))
}

fn optional_u64(block: &Block, name: &str) -> Result<Option<u64>> {
    block
        .attributes
        .get(name)
        .map(|value| unsigned_integer(value, name))
        .transpose()
}

fn required_u32(block: &Block, name: &str) -> Result<u32> {
    let value = required_u64(block, name)?;
    u32::try_from(value).map_err(|_| {
        BootError::BadRequest(format!(
            "workload ACL attribute {name} exceeds the u32 range"
        ))
    })
}

fn required_u16(block: &Block, name: &str) -> Result<u16> {
    let value = required_u64(block, name)?;
    u16::try_from(value).map_err(|_| {
        BootError::BadRequest(format!(
            "workload ACL attribute {name} exceeds the u16 range"
        ))
    })
}

fn unsigned_integer(value: &Value, name: &str) -> Result<u64> {
    let Value::Number(value) = value else {
        return Err(BootError::BadRequest(format!(
            "workload ACL attribute {name} must be a number"
        )));
    };
    if !value.is_finite() || *value < 0.0 || value.fract() != 0.0 || *value > MAX_SAFE_INTEGER {
        return Err(BootError::BadRequest(format!(
            "workload ACL attribute {name} must be a non-negative safe integer"
        )));
    }
    Ok(*value as u64)
}

#[cfg(test)]
#[path = "workload_manifest_tests.rs"]
mod tests;
