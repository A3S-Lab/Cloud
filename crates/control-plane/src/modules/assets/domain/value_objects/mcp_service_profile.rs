use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_acl::builder::{boolean, integer, list, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document, Value};
use a3s_cloud_contracts::{McpServiceProfileProjection, MCP_PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_REQUEST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_STREAM_SECONDS: u64 = 24 * 60 * 60;
pub const MCP_SERVICE_PROFILE_MAX_ACL_BYTES: usize = 64 * 1024;
const PROFILE_BLOCK: &str = "mcp_service_profile";
const PROFILE_ATTRIBUTES: [&str; 11] = [
    "endpoint_path",
    "expected_capabilities",
    "health_path",
    "max_request_bytes",
    "max_response_bytes",
    "max_stream_seconds",
    "protocol_versions",
    "request_sse",
    "runtime_port",
    "server_discover",
    "subscriptions",
];

/// Immutable, product-level MCP behavior that is bound to one published
/// `AssetRelease`. Deployment placement, credentials, origins, and rate limits
/// intentionally do not belong here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServiceProfileSpec {
    pub protocol_versions: Vec<String>,
    pub endpoint_path: String,
    pub runtime_port: String,
    pub health_path: String,
    pub request_sse: bool,
    pub subscriptions: bool,
    pub server_discover: bool,
    pub expected_capabilities: Vec<String>,
    pub max_request_bytes: u64,
    pub max_response_bytes: u64,
    pub max_stream_seconds: u64,
}

/// Canonical A3S ACL and its semantic digest. Construction always reparses the
/// generated ACL, so callers cannot manufacture a digest/bytes mismatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServiceProfile {
    spec: McpServiceProfileSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl McpServiceProfile {
    pub fn from_spec(mut spec: McpServiceProfileSpec) -> Result<Self, String> {
        spec.expected_capabilities.sort_unstable();
        spec.validate()?;
        let document = profile_document(&spec)?;
        let canonical_acl = generate_acl(&document);
        if canonical_acl.len() > MCP_SERVICE_PROFILE_MAX_ACL_BYTES {
            return Err("MCP Service profile ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated MCP Service profile ACL is invalid: {error}"))?;
        let digest =
            Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
                format!("MCP Service profile is not canonicalizable: {error}")
            })?)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(acl: &str) -> Result<Self, String> {
        if acl.is_empty() || acl.len() > MCP_SERVICE_PROFILE_MAX_ACL_BYTES {
            return Err("MCP Service profile ACL size is invalid".into());
        }
        let document = parse_acl(acl)
            .map_err(|error| format!("MCP Service profile ACL is invalid: {error}"))?;
        let block = exact_profile_block(&document)?;
        let spec = McpServiceProfileSpec {
            protocol_versions: strings(block, "protocol_versions")?,
            endpoint_path: required_string(block, "endpoint_path")?,
            runtime_port: required_string(block, "runtime_port")?,
            health_path: required_string(block, "health_path")?,
            request_sse: required_bool(block, "request_sse")?,
            subscriptions: required_bool(block, "subscriptions")?,
            server_discover: required_bool(block, "server_discover")?,
            expected_capabilities: strings(block, "expected_capabilities")?,
            max_request_bytes: required_u64(block, "max_request_bytes")?,
            max_response_bytes: required_u64(block, "max_response_bytes")?,
            max_stream_seconds: required_u64(block, "max_stream_seconds")?,
        };
        Self::from_spec(spec)
    }

    pub fn restore(acl: &str, stored_digest: &str) -> Result<Self, String> {
        let profile = Self::parse_acl(acl)?;
        if profile.digest.as_str() != stored_digest {
            return Err("stored MCP Service profile ACL and digest do not match".into());
        }
        Ok(profile)
    }

    pub const fn spec(&self) -> &McpServiceProfileSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub fn gateway_projection(&self) -> McpServiceProfileProjection {
        McpServiceProfileProjection {
            profile_digest: self.digest.to_string(),
            protocol_versions: self.spec.protocol_versions.clone(),
            path: self.spec.endpoint_path.clone(),
            request_sse: self.spec.request_sse,
            subscriptions: self.spec.subscriptions,
            max_request_bytes: self.spec.max_request_bytes,
            max_response_bytes: self.spec.max_response_bytes,
        }
    }
}

impl McpServiceProfileSpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.protocol_versions != [MCP_PROTOCOL_VERSION] {
            return Err(format!(
                "MCP Service profile must support exactly {MCP_PROTOCOL_VERSION}"
            ));
        }
        validate_literal_path("endpoint", &self.endpoint_path)?;
        validate_literal_path("health", &self.health_path)?;
        if self.endpoint_path == self.health_path {
            return Err("MCP endpoint and health paths must be distinct".into());
        }
        validate_runtime_port(&self.runtime_port)?;
        if !self.server_discover {
            return Err("MCP Service profile must require server/discover".into());
        }
        if self.subscriptions && !self.request_sse {
            return Err("MCP subscriptions require request-scoped SSE".into());
        }
        if self.expected_capabilities.len() > 64 {
            return Err("MCP Service profile exceeds the capability limit".into());
        }
        let mut capabilities = HashSet::new();
        for capability in &self.expected_capabilities {
            validate_identifier("expected capability", capability, 255)?;
            if !capabilities.insert(capability.as_str()) {
                return Err("MCP Service profile contains duplicate capabilities".into());
            }
        }
        if self.subscriptions && !capabilities.contains("subscriptions") {
            return Err("MCP subscriptions must be declared as an expected capability".into());
        }
        if self.max_request_bytes == 0
            || self.max_request_bytes > MAX_REQUEST_BYTES
            || self.max_response_bytes == 0
            || self.max_response_bytes > MAX_RESPONSE_BYTES
            || self.max_stream_seconds == 0
            || self.max_stream_seconds > MAX_STREAM_SECONDS
        {
            return Err("MCP Service profile bounds are invalid".into());
        }
        Ok(())
    }
}

fn profile_document(spec: &McpServiceProfileSpec) -> Result<Document, String> {
    Ok(Document {
        blocks: vec![BlockBuilder::new(PROFILE_BLOCK)
            .attr(
                "protocol_versions",
                list(
                    spec.protocol_versions
                        .iter()
                        .map(|value| string(value))
                        .collect(),
                ),
            )
            .attr("endpoint_path", string(&spec.endpoint_path))
            .attr("runtime_port", string(&spec.runtime_port))
            .attr("health_path", string(&spec.health_path))
            .attr("request_sse", boolean(spec.request_sse))
            .attr("subscriptions", boolean(spec.subscriptions))
            .attr("server_discover", boolean(spec.server_discover))
            .attr(
                "expected_capabilities",
                list(
                    spec.expected_capabilities
                        .iter()
                        .map(|value| string(value))
                        .collect(),
                ),
            )
            .attr(
                "max_request_bytes",
                acl_integer("maximum request bytes", spec.max_request_bytes)?,
            )
            .attr(
                "max_response_bytes",
                acl_integer("maximum response bytes", spec.max_response_bytes)?,
            )
            .attr(
                "max_stream_seconds",
                acl_integer("maximum stream seconds", spec.max_stream_seconds)?,
            )
            .build()],
    })
}

fn exact_profile_block(document: &Document) -> Result<&a3s_acl::Block, String> {
    if document.blocks.len() != 1 {
        return Err("MCP Service profile must contain exactly one top-level block".into());
    }
    let block = &document.blocks[0];
    if block.name != PROFILE_BLOCK || !block.labels.is_empty() || !block.blocks.is_empty() {
        return Err("MCP Service profile block shape is invalid".into());
    }
    if block.attributes.len() != PROFILE_ATTRIBUTES.len()
        || block
            .attributes
            .keys()
            .any(|key| !PROFILE_ATTRIBUTES.contains(&key.as_str()))
    {
        return Err("MCP Service profile contains missing or unknown fields".into());
    }
    Ok(block)
}

fn required_value<'a>(block: &'a a3s_acl::Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("MCP Service profile field {name:?} is required"))
}

fn required_string(block: &a3s_acl::Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("MCP Service profile field {name:?} must be a string"))
}

fn required_bool(block: &a3s_acl::Block, name: &str) -> Result<bool, String> {
    required_value(block, name)?
        .as_bool()
        .ok_or_else(|| format!("MCP Service profile field {name:?} must be a boolean"))
}

fn required_u64(block: &a3s_acl::Block, name: &str) -> Result<u64, String> {
    let number = required_value(block, name)?
        .as_number()
        .ok_or_else(|| format!("MCP Service profile field {name:?} must be an integer"))?;
    if !number.is_finite()
        || number.fract() != 0.0
        || number <= 0.0
        || number > MAX_SAFE_ACL_INTEGER as f64
    {
        return Err(format!(
            "MCP Service profile field {name:?} must be a positive exactly representable integer"
        ));
    }
    Ok(number as u64)
}

fn strings(block: &a3s_acl::Block, name: &str) -> Result<Vec<String>, String> {
    let Value::List(values) = required_value(block, name)? else {
        return Err(format!(
            "MCP Service profile field {name:?} must be a string list"
        ));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("MCP Service profile field {name:?} must be a string list"))
        })
        .collect()
}

fn acl_integer(label: &str, value: u64) -> Result<Value, String> {
    if value == 0 || value > MAX_SAFE_ACL_INTEGER {
        return Err(format!(
            "MCP Service profile {label} is not representable by ACL"
        ));
    }
    Ok(integer(value as i64))
}

fn validate_literal_path(label: &str, path: &str) -> Result<(), String> {
    if path.is_empty()
        || path.len() > 1_024
        || !path.starts_with('/')
        || path.starts_with("//")
        || path.contains(['?', '#', '%', '*', '{', '}', '`'])
        || path.split('/').any(|segment| matches!(segment, "." | ".."))
        || path
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(format!(
            "MCP Service profile {label} path must be one safe literal path"
        ));
    }
    Ok(())
}

fn validate_identifier(label: &str, value: &str, maximum_length: usize) -> Result<(), String> {
    if value.is_empty()
        || value.len() > maximum_length
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'_' | b'.' | b'/')
        })
    {
        return Err(format!("MCP Service profile {label} is invalid"));
    }
    Ok(())
}

fn validate_runtime_port(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 63
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
    {
        return Err("MCP Service profile Runtime port is invalid".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_spec() -> McpServiceProfileSpec {
        McpServiceProfileSpec {
            protocol_versions: vec![MCP_PROTOCOL_VERSION.into()],
            endpoint_path: "/mcp".into(),
            runtime_port: "mcp".into(),
            health_path: "/health".into(),
            request_sse: true,
            subscriptions: true,
            server_discover: true,
            expected_capabilities: vec!["tools".into(), "subscriptions".into()],
            max_request_bytes: 1_048_576,
            max_response_bytes: 8_388_608,
            max_stream_seconds: 3_600,
        }
    }

    #[test]
    fn canonical_acl_round_trips_and_projects_only_gateway_fields() {
        let profile = McpServiceProfile::from_spec(fixture_spec()).expect("profile");
        let restored = McpServiceProfile::parse_acl(profile.canonical_acl()).expect("restored");
        assert_eq!(restored, profile);
        assert!(profile.digest().as_str().starts_with("sha256:"));

        let projection = profile.gateway_projection();
        assert_eq!(projection.profile_digest, profile.digest().as_str());
        assert_eq!(projection.path, "/mcp");
        assert!(projection.request_sse);
        assert!(projection.subscriptions);
    }

    #[test]
    fn set_order_does_not_change_canonical_acl_or_digest() {
        let mut reordered = fixture_spec();
        reordered.expected_capabilities.reverse();
        assert_eq!(
            McpServiceProfile::from_spec(fixture_spec()).expect("first"),
            McpServiceProfile::from_spec(reordered).expect("second")
        );
    }

    #[test]
    fn parser_rejects_unknown_legacy_and_unbounded_profile_fields() {
        let canonical = McpServiceProfile::from_spec(fixture_spec())
            .expect("profile")
            .canonical_acl()
            .to_owned();
        assert!(McpServiceProfile::parse_acl(&canonical.replace(
            "subscriptions = true",
            "subscriptions = true\n  session_support = true"
        ))
        .is_err());
        assert!(McpServiceProfile::parse_acl(
            &canonical.replace(MCP_PROTOCOL_VERSION, "2025-06-18")
        )
        .is_err());
        assert!(McpServiceProfile::parse_acl(&canonical.replace("/mcp", "/mcp/*")).is_err());
        assert!(McpServiceProfile::parse_acl(
            &canonical.replace("max_request_bytes = 1048576", "max_request_bytes = 0")
        )
        .is_err());
        assert!(McpServiceProfile::parse_acl(
            &canonical.replace("server_discover = true", "server_discover = false")
        )
        .is_err());
    }

    #[test]
    fn restore_rejects_a_digest_mismatch() {
        let profile = McpServiceProfile::from_spec(fixture_spec()).expect("profile");
        assert!(McpServiceProfile::restore(
            profile.canonical_acl(),
            &format!("sha256:{}", "f".repeat(64))
        )
        .is_err());
    }
}
