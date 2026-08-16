use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_acl::builder::{boolean, integer, list, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document, Value};
use serde::{Deserialize, Serialize};

const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_CELL_NAME_BYTES: u64 = 4 * 1024;
const MAX_REQUEST_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RESPONSE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_WEBSOCKET_MESSAGE_BYTES: u64 = 16 * 1024 * 1024;
const PROFILE_BLOCK: &str = "durable_cell_service_profile";
const PROFILE_ATTRIBUTES: [&str; 19] = [
    "dedicated_application_fleet",
    "epoch_fencing",
    "health_path",
    "hibernatable_websockets",
    "idle_eviction",
    "internal_runtime_port",
    "max_cell_name_bytes",
    "max_request_bytes",
    "max_response_bytes",
    "max_websocket_message_bytes",
    "provider_protocol",
    "public_runtime_port",
    "replicate_before_acknowledgement",
    "required_handlers",
    "required_storage_guarantees",
    "schema",
    "single_threaded_event_turns",
    "single_writer",
    "sqlite_per_cell",
];
const REQUIRED_HANDLERS: [&str; 3] = ["alarm", "fetch", "websocket"];
const REQUIRED_STORAGE_GUARANTEES: [&str; 3] = [
    "conditional_create",
    "conditional_overwrite",
    "read_after_write",
];

pub const DURABLE_CELL_PROFILE_SCHEMA: &str = "cloud.durable-cell.service.v1";
pub const DURABLE_CELL_PROVIDER_PROTOCOL: &str = "a3s.durable-cell-provider.v1";
pub const DURABLE_CELL_SERVICE_PROFILE_MAX_ACL_BYTES: usize = 64 * 1024;

/// Bounded fields that may vary between immutable Durable Cell service
/// profiles. The state, ownership, durability, and isolation guarantees are
/// fixed by the v1 schema and cannot be weakened by callers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableCellServiceProfileSpec {
    pub public_runtime_port: String,
    pub internal_runtime_port: String,
    pub health_path: String,
    pub max_cell_name_bytes: u64,
    pub max_request_bytes: u64,
    pub max_response_bytes: u64,
    pub max_websocket_message_bytes: u64,
}

/// Immutable product semantics for a named, stateful Durable Cell service.
///
/// This contract does not own placement, Runtime lifecycle, object-store
/// credentials, per-cell ownership records, SQLite bytes, routing, or retry
/// scheduling. Those remain with Workloads/Fleet, Runtime/Box, S0/Secrets, the
/// selected Cell provider, Edge/Gateway, and their existing authorities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellServiceProfile {
    spec: DurableCellServiceProfileSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl DurableCellServiceProfile {
    pub fn from_spec(spec: DurableCellServiceProfileSpec) -> Result<Self, String> {
        spec.validate()?;
        let document = profile_document(&spec)?;
        let canonical_acl = generate_acl(&document);
        if canonical_acl.len() > DURABLE_CELL_SERVICE_PROFILE_MAX_ACL_BYTES {
            return Err("Durable Cell Service profile ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl).map_err(|error| {
            format!("generated Durable Cell Service profile ACL is invalid: {error}")
        })?;
        let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
            format!("Durable Cell Service profile is not canonicalizable: {error}")
        })?)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(acl: &str) -> Result<Self, String> {
        if acl.is_empty() || acl.len() > DURABLE_CELL_SERVICE_PROFILE_MAX_ACL_BYTES {
            return Err("Durable Cell Service profile ACL size is invalid".into());
        }
        let document = parse_acl(acl)
            .map_err(|error| format!("Durable Cell Service profile ACL is invalid: {error}"))?;
        let block = exact_profile_block(&document)?;
        require_exact_string(block, "schema", DURABLE_CELL_PROFILE_SCHEMA)?;
        require_exact_string(block, "provider_protocol", DURABLE_CELL_PROVIDER_PROTOCOL)?;
        require_exact_strings(block, "required_handlers", &REQUIRED_HANDLERS)?;
        require_exact_strings(
            block,
            "required_storage_guarantees",
            &REQUIRED_STORAGE_GUARANTEES,
        )?;
        for field in [
            "dedicated_application_fleet",
            "sqlite_per_cell",
            "single_threaded_event_turns",
            "single_writer",
            "epoch_fencing",
            "replicate_before_acknowledgement",
            "idle_eviction",
            "hibernatable_websockets",
        ] {
            if !required_bool(block, field)? {
                return Err(format!(
                    "Durable Cell Service profile guarantee {field:?} cannot be disabled"
                ));
            }
        }
        Self::from_spec(DurableCellServiceProfileSpec {
            public_runtime_port: required_string(block, "public_runtime_port")?,
            internal_runtime_port: required_string(block, "internal_runtime_port")?,
            health_path: required_string(block, "health_path")?,
            max_cell_name_bytes: required_u64(block, "max_cell_name_bytes")?,
            max_request_bytes: required_u64(block, "max_request_bytes")?,
            max_response_bytes: required_u64(block, "max_response_bytes")?,
            max_websocket_message_bytes: required_u64(block, "max_websocket_message_bytes")?,
        })
    }

    pub fn restore(acl: &str, stored_digest: &str) -> Result<Self, String> {
        let profile = Self::parse_acl(acl)?;
        if profile.canonical_acl != acl || profile.digest.as_str() != stored_digest {
            return Err("stored Durable Cell Service profile ACL and digest do not match".into());
        }
        Ok(profile)
    }

    pub const fn spec(&self) -> &DurableCellServiceProfileSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

impl DurableCellServiceProfileSpec {
    pub fn validate(&self) -> Result<(), String> {
        validate_runtime_port(&self.public_runtime_port)?;
        validate_runtime_port(&self.internal_runtime_port)?;
        if self.public_runtime_port == self.internal_runtime_port {
            return Err("Durable Cell public and internal Runtime ports must be distinct".into());
        }
        validate_literal_path(&self.health_path)?;
        if self.max_cell_name_bytes == 0
            || self.max_cell_name_bytes > MAX_CELL_NAME_BYTES
            || self.max_request_bytes == 0
            || self.max_request_bytes > MAX_REQUEST_BYTES
            || self.max_response_bytes == 0
            || self.max_response_bytes > MAX_RESPONSE_BYTES
            || self.max_websocket_message_bytes == 0
            || self.max_websocket_message_bytes > MAX_WEBSOCKET_MESSAGE_BYTES
        {
            return Err("Durable Cell Service profile bounds are invalid".into());
        }
        Ok(())
    }
}

fn profile_document(spec: &DurableCellServiceProfileSpec) -> Result<Document, String> {
    Ok(Document {
        blocks: vec![BlockBuilder::new(PROFILE_BLOCK)
            .attr("schema", string(DURABLE_CELL_PROFILE_SCHEMA))
            .attr("provider_protocol", string(DURABLE_CELL_PROVIDER_PROTOCOL))
            .attr(
                "required_handlers",
                list(
                    REQUIRED_HANDLERS
                        .iter()
                        .map(|value| string(value))
                        .collect(),
                ),
            )
            .attr(
                "required_storage_guarantees",
                list(
                    REQUIRED_STORAGE_GUARANTEES
                        .iter()
                        .map(|value| string(value))
                        .collect(),
                ),
            )
            .attr("dedicated_application_fleet", boolean(true))
            .attr("sqlite_per_cell", boolean(true))
            .attr("single_threaded_event_turns", boolean(true))
            .attr("single_writer", boolean(true))
            .attr("epoch_fencing", boolean(true))
            .attr("replicate_before_acknowledgement", boolean(true))
            .attr("idle_eviction", boolean(true))
            .attr("hibernatable_websockets", boolean(true))
            .attr("public_runtime_port", string(&spec.public_runtime_port))
            .attr("internal_runtime_port", string(&spec.internal_runtime_port))
            .attr("health_path", string(&spec.health_path))
            .attr(
                "max_cell_name_bytes",
                acl_integer("maximum Cell-name bytes", spec.max_cell_name_bytes)?,
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
                "max_websocket_message_bytes",
                acl_integer(
                    "maximum WebSocket message bytes",
                    spec.max_websocket_message_bytes,
                )?,
            )
            .build()],
    })
}

fn exact_profile_block(document: &Document) -> Result<&a3s_acl::Block, String> {
    if document.blocks.len() != 1 {
        return Err("Durable Cell Service profile must contain exactly one top-level block".into());
    }
    let block = &document.blocks[0];
    if block.name != PROFILE_BLOCK || !block.labels.is_empty() || !block.blocks.is_empty() {
        return Err("Durable Cell Service profile block shape is invalid".into());
    }
    if block.attributes.len() != PROFILE_ATTRIBUTES.len()
        || block
            .attributes
            .keys()
            .any(|key| !PROFILE_ATTRIBUTES.contains(&key.as_str()))
    {
        return Err("Durable Cell Service profile contains missing or unknown fields".into());
    }
    Ok(block)
}

fn required_value<'a>(block: &'a a3s_acl::Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("Durable Cell Service profile field {name:?} is required"))
}

fn required_string(block: &a3s_acl::Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("Durable Cell Service profile field {name:?} must be a string"))
}

fn required_bool(block: &a3s_acl::Block, name: &str) -> Result<bool, String> {
    required_value(block, name)?
        .as_bool()
        .ok_or_else(|| format!("Durable Cell Service profile field {name:?} must be a boolean"))
}

fn required_u64(block: &a3s_acl::Block, name: &str) -> Result<u64, String> {
    let number = required_value(block, name)?
        .as_number()
        .ok_or_else(|| format!("Durable Cell Service profile field {name:?} must be an integer"))?;
    if !number.is_finite()
        || number.fract() != 0.0
        || number <= 0.0
        || number > MAX_SAFE_ACL_INTEGER as f64
    {
        return Err(format!(
            "Durable Cell Service profile field {name:?} must be a positive exactly representable integer"
        ));
    }
    Ok(number as u64)
}

fn strings(block: &a3s_acl::Block, name: &str) -> Result<Vec<String>, String> {
    let Value::List(values) = required_value(block, name)? else {
        return Err(format!(
            "Durable Cell Service profile field {name:?} must be a string list"
        ));
    };
    values
        .iter()
        .map(|value| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                format!("Durable Cell Service profile field {name:?} must be a string list")
            })
        })
        .collect()
}

fn require_exact_string(block: &a3s_acl::Block, name: &str, expected: &str) -> Result<(), String> {
    if required_string(block, name)? != expected {
        return Err(format!(
            "Durable Cell Service profile field {name:?} must be exactly {expected:?}"
        ));
    }
    Ok(())
}

fn require_exact_strings(
    block: &a3s_acl::Block,
    name: &str,
    expected: &[&str],
) -> Result<(), String> {
    let actual = strings(block, name)?;
    if actual.iter().map(String::as_str).collect::<Vec<_>>() != expected {
        return Err(format!(
            "Durable Cell Service profile field {name:?} does not match the v1 contract"
        ));
    }
    Ok(())
}

fn acl_integer(label: &str, value: u64) -> Result<Value, String> {
    if value == 0 || value > MAX_SAFE_ACL_INTEGER {
        return Err(format!(
            "Durable Cell Service profile {label} is not representable by ACL"
        ));
    }
    Ok(integer(value as i64))
}

fn validate_runtime_port(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 63
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
    {
        return Err("Durable Cell Service profile Runtime port is invalid".into());
    }
    Ok(())
}

fn validate_literal_path(path: &str) -> Result<(), String> {
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
        return Err(
            "Durable Cell Service profile health path must be one safe literal path".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVICE_PROFILE_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/cell0.1/service-profile.acl"
    ));
    const CELLD_V021_SERVICE_PROFILE_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/cell0.3/celld-v0.2.1-service-profile.acl"
    ));
    const SERVICE_PROFILE_FIXTURE_DIGEST: &str =
        "sha256:55422ee8bc0028a10e09aef7487e321511cbcc05545d693338b5cc086d43b303";
    const CELLD_V021_SERVICE_PROFILE_FIXTURE_DIGEST: &str =
        "sha256:0389b20dc544c0441b611df52e8dadf8b94a48db41454777f2f140acc007df15";

    fn fixture_spec() -> DurableCellServiceProfileSpec {
        DurableCellServiceProfileSpec {
            public_runtime_port: "cell-public".into(),
            internal_runtime_port: "cell-internal".into(),
            health_path: "/__a3s/cell/health".into(),
            max_cell_name_bytes: 512,
            max_request_bytes: 16 * 1024 * 1024,
            max_response_bytes: 64 * 1024 * 1024,
            max_websocket_message_bytes: 1024 * 1024,
        }
    }

    #[test]
    fn canonical_acl_round_trips_with_the_required_durability_contract() {
        let profile = DurableCellServiceProfile::from_spec(fixture_spec()).expect("profile");
        let restored =
            DurableCellServiceProfile::parse_acl(profile.canonical_acl()).expect("restored");
        assert_eq!(restored, profile);
        assert!(profile.digest().as_str().starts_with("sha256:"));
        assert!(profile
            .canonical_acl()
            .contains("replicate_before_acknowledgement = true"));
        assert!(profile.canonical_acl().contains("conditional_overwrite"));
    }

    #[test]
    fn shared_cell0_1_service_profile_fixture_is_canonical_and_digest_locked() {
        let profile =
            DurableCellServiceProfile::parse_acl(SERVICE_PROFILE_FIXTURE).expect("fixture");
        assert_eq!(
            format!("{}\n", profile.canonical_acl()),
            SERVICE_PROFILE_FIXTURE.replace("\r\n", "\n")
        );
        assert_eq!(profile.digest().as_str(), SERVICE_PROFILE_FIXTURE_DIGEST);
    }

    #[test]
    fn celld_v021_adapter_profile_is_canonical_and_digest_locked() {
        let profile = DurableCellServiceProfile::parse_acl(CELLD_V021_SERVICE_PROFILE_FIXTURE)
            .expect("celld adapter profile");
        assert_eq!(
            format!("{}\n", profile.canonical_acl()),
            CELLD_V021_SERVICE_PROFILE_FIXTURE.replace("\r\n", "\n")
        );
        assert_eq!(
            profile.digest().as_str(),
            CELLD_V021_SERVICE_PROFILE_FIXTURE_DIGEST
        );
        assert_eq!(profile.spec().health_path, "/__celld/health");
    }

    #[test]
    fn parser_rejects_weakened_unknown_or_reordered_semantics() {
        let canonical = DurableCellServiceProfile::from_spec(fixture_spec())
            .expect("profile")
            .canonical_acl()
            .to_owned();
        assert!(DurableCellServiceProfile::parse_acl(&canonical.replace(
            "replicate_before_acknowledgement = true",
            "replicate_before_acknowledgement = false"
        ))
        .is_err());
        assert!(DurableCellServiceProfile::parse_acl(&canonical.replace(
            "[\"alarm\", \"fetch\", \"websocket\"]",
            "[\"fetch\", \"alarm\", \"websocket\"]"
        ))
        .is_err());
        assert!(DurableCellServiceProfile::parse_acl(&canonical.replace(
            "single_writer = true",
            "single_writer = true\n  consensus = \"raft\""
        ))
        .is_err());
        assert!(DurableCellServiceProfile::parse_acl(
            &canonical.replace(DURABLE_CELL_PROVIDER_PROTOCOL, "celld.internal.alpha")
        )
        .is_err());
    }

    #[test]
    fn profile_rejects_shared_ports_and_unbounded_inputs() {
        let mut shared_port = fixture_spec();
        shared_port.internal_runtime_port = shared_port.public_runtime_port.clone();
        assert!(DurableCellServiceProfile::from_spec(shared_port).is_err());

        let mut unbounded = fixture_spec();
        unbounded.max_cell_name_bytes = MAX_CELL_NAME_BYTES + 1;
        assert!(DurableCellServiceProfile::from_spec(unbounded).is_err());
    }

    #[test]
    fn restore_rejects_a_digest_mismatch() {
        let profile = DurableCellServiceProfile::from_spec(fixture_spec()).expect("profile");
        assert!(DurableCellServiceProfile::restore(
            profile.canonical_acl(),
            &format!("sha256:{}", "f".repeat(64))
        )
        .is_err());
        assert!(DurableCellServiceProfile::restore(
            &format!("\n{}", profile.canonical_acl()),
            profile.digest().as_str()
        )
        .is_err());
    }
}
