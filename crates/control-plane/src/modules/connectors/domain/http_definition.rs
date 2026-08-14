use super::{
    validate_connector_content_type, validate_connector_signature_metadata, ConnectorHttpMethod,
    ConnectorHttpStatusPolicy, MAXIMUM_CONNECTOR_BODY_BYTES,
};
use crate::modules::shared_kernel::domain::{SecretId, Sha256Digest};
use a3s_acl::builder::{integer, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;
use url::Url;
use uuid::Uuid;

pub const CONNECTOR_HTTP_DEFINITION_SCHEMA: &str = "cloud.connector.http.v1";
pub const CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES: usize = 64 * 1024;
const CONNECTOR_HTTP_BLOCK: &str = "connector_http";
const MAXIMUM_ENDPOINT_CHARACTERS: usize = 2_048;
const MAXIMUM_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const MAXIMUM_RETRY_AFTER: Duration = Duration::from_secs(86_400);
const MAXIMUM_SIGNING_SECRET_BYTES: usize = 4 * 1024;
pub(crate) const MINIMUM_SIGNING_SECRET_BYTES: usize = 32;
const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorSecretReference {
    pub secret_id: SecretId,
    pub version: u64,
}

impl ConnectorSecretReference {
    pub fn new(secret_id: SecretId, version: u64) -> Result<Self, String> {
        let value = Self { secret_id, version };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.secret_id.as_uuid().is_nil()
            || self.version == 0
            || self.version > MAX_SAFE_ACL_INTEGER
        {
            return Err("connector Secret reference is invalid".into());
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectorHttpDestination {
    LiteralHttps { endpoint: String },
    SecretHttpsUrl { reference: ConnectorSecretReference },
}

impl fmt::Debug for ConnectorHttpDestination {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LiteralHttps { .. } => formatter
                .debug_struct("LiteralHttps")
                .field("endpoint", &"redacted")
                .finish(),
            Self::SecretHttpsUrl { reference } => formatter
                .debug_struct("SecretHttpsUrl")
                .field("reference", reference)
                .finish(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectorHttpAuthentication {
    None,
    HmacSha256 {
        secret: ConnectorSecretReference,
        signature_header: String,
        value_prefix: String,
    },
}

impl fmt::Debug for ConnectorHttpAuthentication {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => formatter.write_str("None"),
            Self::HmacSha256 {
                secret,
                signature_header,
                ..
            } => formatter
                .debug_struct("HmacSha256")
                .field("secret", secret)
                .field("signature_header", signature_header)
                .field("value_prefix", &"redacted")
                .finish(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorHttpDefinitionSpec {
    pub destination: ConnectorHttpDestination,
    pub method: ConnectorHttpMethod,
    pub request_content_type: String,
    pub maximum_request_bytes: u64,
    pub maximum_response_bytes: u64,
    pub timeout_milliseconds: u64,
    pub status_policy: ConnectorHttpStatusPolicy,
    pub authentication: ConnectorHttpAuthentication,
}

impl fmt::Debug for ConnectorHttpDefinitionSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectorHttpDefinitionSpec")
            .field("destination", &self.destination)
            .field("method", &self.method)
            .field("request_content_type", &self.request_content_type)
            .field("maximum_request_bytes", &self.maximum_request_bytes)
            .field("maximum_response_bytes", &self.maximum_response_bytes)
            .field("timeout_milliseconds", &self.timeout_milliseconds)
            .field("status_policy", &self.status_policy)
            .field("authentication", &self.authentication)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorHttpDefinition {
    spec: ConnectorHttpDefinitionSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl ConnectorHttpDefinition {
    pub fn from_spec(spec: ConnectorHttpDefinitionSpec) -> Result<Self, String> {
        let spec = normalize_spec(spec)?;
        let document = definition_document(&spec)?;
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES {
            return Err("Connector HTTP definition ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated Connector HTTP ACL is invalid: {error}"))?;
        let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
            format!("Connector HTTP definition is not canonicalizable: {error}")
        })?)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > CONNECTOR_HTTP_DEFINITION_MAX_ACL_BYTES {
            return Err("Connector HTTP definition ACL size is invalid".into());
        }
        if source.replace("\r\n", "").contains('\r') {
            return Err("Connector HTTP definition contains a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let document = parse_acl(&normalized)
            .map_err(|error| format!("Connector HTTP definition ACL is invalid: {error}"))?;
        let definition = Self::from_spec(parse_definition(&document)?)?;
        if definition.canonical_acl != normalized {
            return Err("Connector HTTP definition ACL is not canonical".into());
        }
        Ok(definition)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let definition = Self::parse_acl(source)?;
        if definition.digest.as_str() != stored_digest {
            return Err("stored Connector HTTP definition and digest do not match".into());
        }
        Ok(definition)
    }

    pub const fn spec(&self) -> &ConnectorHttpDefinitionSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub fn secret_bindings(&self) -> Vec<ConnectorSecretBinding> {
        let mut bindings = Vec::with_capacity(2);
        if let ConnectorHttpDestination::SecretHttpsUrl { reference } = &self.spec.destination {
            bindings.push(ConnectorSecretBinding {
                purpose: ConnectorSecretBindingPurpose::Destination,
                reference: *reference,
            });
        }
        if let ConnectorHttpAuthentication::HmacSha256 { secret, .. } = &self.spec.authentication {
            bindings.push(ConnectorSecretBinding {
                purpose: ConnectorSecretBindingPurpose::HmacSha256,
                reference: *secret,
            });
        }
        bindings
    }
}

impl fmt::Debug for ConnectorHttpDefinition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectorHttpDefinition")
            .field("spec", &self.spec)
            .field("canonical_acl_bytes", &self.canonical_acl.len())
            .field("digest", &self.digest)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorSecretBindingPurpose {
    Destination,
    HmacSha256,
}

impl ConnectorSecretBindingPurpose {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Destination => "destination",
            Self::HmacSha256 => "hmac_sha256",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "destination" => Ok(Self::Destination),
            "hmac_sha256" => Ok(Self::HmacSha256),
            _ => Err("unsupported Connector Secret binding purpose".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorSecretBinding {
    pub purpose: ConnectorSecretBindingPurpose,
    pub reference: ConnectorSecretReference,
}

fn normalize_spec(
    mut spec: ConnectorHttpDefinitionSpec,
) -> Result<ConnectorHttpDefinitionSpec, String> {
    spec.status_policy = ConnectorHttpStatusPolicy::new(
        spec.status_policy.accepted_ranges().to_vec(),
        spec.status_policy.retryable_ranges().to_vec(),
    )?;
    validate_connector_content_type(&spec.request_content_type)?;
    let maximum_request_bytes = usize::try_from(spec.maximum_request_bytes)
        .map_err(|_| "Connector HTTP request byte limit is not representable".to_owned())?;
    let maximum_response_bytes = usize::try_from(spec.maximum_response_bytes)
        .map_err(|_| "Connector HTTP response byte limit is not representable".to_owned())?;
    validate_connector_http_limits(
        maximum_request_bytes,
        maximum_response_bytes,
        Duration::from_millis(spec.timeout_milliseconds),
    )?;
    match &mut spec.destination {
        ConnectorHttpDestination::LiteralHttps { endpoint } => {
            let parsed = Url::parse(endpoint)
                .map_err(|_| "Connector literal HTTPS destination is invalid".to_owned())?;
            validate_resolved_connector_endpoint(&parsed, false, false)?;
            *endpoint = parsed.to_string();
        }
        ConnectorHttpDestination::SecretHttpsUrl { reference } => reference.validate()?,
    }
    match &spec.authentication {
        ConnectorHttpAuthentication::None => {}
        ConnectorHttpAuthentication::HmacSha256 {
            secret,
            signature_header,
            value_prefix,
        } => {
            secret.validate()?;
            validate_connector_signature_metadata(signature_header, value_prefix)?;
            if matches!(
                &spec.destination,
                ConnectorHttpDestination::SecretHttpsUrl { reference } if *reference == *secret
            ) {
                return Err(
                    "Connector destination and HMAC key must use distinct Secret references".into(),
                );
            }
        }
    }
    Ok(spec)
}

fn definition_document(spec: &ConnectorHttpDefinitionSpec) -> Result<Document, String> {
    let destination = match &spec.destination {
        ConnectorHttpDestination::LiteralHttps { endpoint } => BlockBuilder::new("destination")
            .label("literal_https")
            .attr("endpoint", string(endpoint))
            .build(),
        ConnectorHttpDestination::SecretHttpsUrl { reference } => BlockBuilder::new("destination")
            .label("secret_https_url")
            .attr("secret_id", string(&reference.secret_id.to_string()))
            .attr(
                "secret_version",
                acl_integer("Secret version", reference.version)?,
            )
            .build(),
    };
    let authentication = match &spec.authentication {
        ConnectorHttpAuthentication::None => {
            BlockBuilder::new("authentication").label("none").build()
        }
        ConnectorHttpAuthentication::HmacSha256 {
            secret,
            signature_header,
            value_prefix,
        } => BlockBuilder::new("authentication")
            .label("hmac_sha256")
            .attr("secret_id", string(&secret.secret_id.to_string()))
            .attr(
                "secret_version",
                acl_integer("Secret version", secret.version)?,
            )
            .attr("signature_header", string(signature_header))
            .attr("value_prefix", string(value_prefix))
            .build(),
    };
    let mut root = BlockBuilder::new(CONNECTOR_HTTP_BLOCK)
        .attr("schema", string(CONNECTOR_HTTP_DEFINITION_SCHEMA))
        .attr("method", string(spec.method.as_str()))
        .attr("request_content_type", string(&spec.request_content_type))
        .attr(
            "maximum_request_bytes",
            acl_integer("maximum request bytes", spec.maximum_request_bytes)?,
        )
        .attr(
            "maximum_response_bytes",
            acl_integer("maximum response bytes", spec.maximum_response_bytes)?,
        )
        .attr(
            "timeout_milliseconds",
            acl_integer("timeout milliseconds", spec.timeout_milliseconds)?,
        )
        .nested_block(destination)
        .nested_block(authentication);
    for (start, end) in spec.status_policy.accepted_ranges() {
        root = root.nested_block(status_block("accepted_status", *start, *end));
    }
    for (start, end) in spec.status_policy.retryable_ranges() {
        root = root.nested_block(status_block("retryable_status", *start, *end));
    }
    Ok(Document {
        blocks: vec![root.build()],
    })
}

fn status_block(name: &str, start: u16, end: u16) -> Block {
    BlockBuilder::new(name)
        .attr("start", integer(i64::from(start)))
        .attr("end", integer(i64::from(end)))
        .build()
}

fn parse_definition(document: &Document) -> Result<ConnectorHttpDefinitionSpec, String> {
    if document.blocks.len() != 1 {
        return Err("Connector HTTP definition must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    let attributes = [
        "schema",
        "method",
        "request_content_type",
        "maximum_request_bytes",
        "maximum_response_bytes",
        "timeout_milliseconds",
    ];
    if root.name != CONNECTOR_HTTP_BLOCK
        || !root.labels.is_empty()
        || root.attributes.len() != attributes.len()
        || root
            .attributes
            .keys()
            .any(|key| !attributes.contains(&key.as_str()))
        || root.blocks.iter().any(|block| {
            !matches!(
                block.name.as_str(),
                "destination" | "authentication" | "accepted_status" | "retryable_status"
            )
        })
    {
        return Err("Connector HTTP definition root shape is invalid".into());
    }
    if required_string(root, "schema")? != CONNECTOR_HTTP_DEFINITION_SCHEMA {
        return Err("Connector HTTP definition schema is unsupported".into());
    }
    let destination = exact_child(root, "destination")?;
    let destination = parse_destination(destination)?;
    let authentication = exact_child(root, "authentication")?;
    let authentication = parse_authentication(authentication)?;
    let accepted = root
        .blocks
        .iter()
        .filter(|block| block.name == "accepted_status")
        .map(parse_status_range)
        .collect::<Result<Vec<_>, _>>()?;
    let retryable = root
        .blocks
        .iter()
        .filter(|block| block.name == "retryable_status")
        .map(parse_status_range)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ConnectorHttpDefinitionSpec {
        destination,
        method: ConnectorHttpMethod::parse(&required_string(root, "method")?)?,
        request_content_type: required_string(root, "request_content_type")?,
        maximum_request_bytes: required_positive_u64(root, "maximum_request_bytes")?,
        maximum_response_bytes: required_positive_u64(root, "maximum_response_bytes")?,
        timeout_milliseconds: required_positive_u64(root, "timeout_milliseconds")?,
        status_policy: ConnectorHttpStatusPolicy::new(accepted, retryable)?,
        authentication,
    })
}

fn parse_destination(block: &Block) -> Result<ConnectorHttpDestination, String> {
    if block.labels.len() != 1 || !block.blocks.is_empty() {
        return Err("Connector HTTP destination block shape is invalid".into());
    }
    match block.labels[0].as_str() {
        "literal_https" => {
            exact_attributes(block, &["endpoint"])?;
            Ok(ConnectorHttpDestination::LiteralHttps {
                endpoint: required_string(block, "endpoint")?,
            })
        }
        "secret_https_url" => {
            exact_attributes(block, &["secret_id", "secret_version"])?;
            Ok(ConnectorHttpDestination::SecretHttpsUrl {
                reference: parse_secret_reference(block)?,
            })
        }
        _ => Err("Connector HTTP destination kind is unsupported".into()),
    }
}

fn parse_authentication(block: &Block) -> Result<ConnectorHttpAuthentication, String> {
    if block.labels.len() != 1 || !block.blocks.is_empty() {
        return Err("Connector HTTP authentication block shape is invalid".into());
    }
    match block.labels[0].as_str() {
        "none" => {
            exact_attributes(block, &[])?;
            Ok(ConnectorHttpAuthentication::None)
        }
        "hmac_sha256" => {
            exact_attributes(
                block,
                &[
                    "secret_id",
                    "secret_version",
                    "signature_header",
                    "value_prefix",
                ],
            )?;
            Ok(ConnectorHttpAuthentication::HmacSha256 {
                secret: parse_secret_reference(block)?,
                signature_header: required_string(block, "signature_header")?,
                value_prefix: required_string(block, "value_prefix")?,
            })
        }
        _ => Err("Connector HTTP authentication kind is unsupported".into()),
    }
}

fn parse_secret_reference(block: &Block) -> Result<ConnectorSecretReference, String> {
    let secret_id = Uuid::parse_str(&required_string(block, "secret_id")?)
        .map_err(|_| "Connector Secret ID is invalid".to_owned())?;
    ConnectorSecretReference::new(
        SecretId::from_uuid(secret_id),
        required_positive_u64(block, "secret_version")?,
    )
}

fn parse_status_range(block: &Block) -> Result<(u16, u16), String> {
    if !block.labels.is_empty() || !block.blocks.is_empty() {
        return Err("Connector HTTP status range block shape is invalid".into());
    }
    exact_attributes(block, &["start", "end"])?;
    let start = u16::try_from(required_positive_u64(block, "start")?)
        .map_err(|_| "Connector HTTP status range start is invalid".to_owned())?;
    let end = u16::try_from(required_positive_u64(block, "end")?)
        .map_err(|_| "Connector HTTP status range end is invalid".to_owned())?;
    Ok((start, end))
}

fn exact_child<'a>(root: &'a Block, name: &str) -> Result<&'a Block, String> {
    let mut matching = root.blocks.iter().filter(|block| block.name == name);
    let child = matching
        .next()
        .ok_or_else(|| format!("Connector HTTP {name} block is required"))?;
    if matching.next().is_some() {
        return Err(format!("Connector HTTP {name} block must be unique"));
    }
    Ok(child)
}

fn exact_attributes(block: &Block, expected: &[&str]) -> Result<(), String> {
    if block.attributes.len() != expected.len()
        || block
            .attributes
            .keys()
            .any(|key| !expected.contains(&key.as_str()))
    {
        return Err(format!(
            "Connector HTTP {:?} block contains missing or unknown fields",
            block.name
        ));
    }
    Ok(())
}

fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("Connector HTTP field {name:?} is required"))
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("Connector HTTP field {name:?} must be a string"))
}

fn required_positive_u64(block: &Block, name: &str) -> Result<u64, String> {
    let value = required_value(block, name)?
        .as_number()
        .ok_or_else(|| format!("Connector HTTP field {name:?} must be an integer"))?;
    if !value.is_finite()
        || value.fract() != 0.0
        || value <= 0.0
        || value > MAX_SAFE_ACL_INTEGER as f64
    {
        return Err(format!(
            "Connector HTTP field {name:?} must be a positive exactly representable integer"
        ));
    }
    Ok(value as u64)
}

fn acl_integer(label: &str, value: u64) -> Result<Value, String> {
    if value == 0 || value > MAX_SAFE_ACL_INTEGER {
        return Err(format!(
            "Connector HTTP {label} is not representable by ACL"
        ));
    }
    Ok(integer(value as i64))
}

pub(crate) fn validate_resolved_connector_endpoint(
    endpoint: &Url,
    allow_http: bool,
    allow_query: bool,
) -> Result<(), String> {
    let accepted_scheme =
        endpoint.scheme() == "https" || (allow_http && endpoint.scheme() == "http");
    if !accepted_scheme
        || endpoint.as_str().chars().count() > MAXIMUM_ENDPOINT_CHARACTERS
        || endpoint.host_str().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.fragment().is_some()
        || (!allow_query && endpoint.query().is_some())
    {
        return Err("resolved Connector HTTP destination is invalid".into());
    }
    Ok(())
}

pub(crate) fn validate_connector_http_limits(
    maximum_request_bytes: usize,
    maximum_response_bytes: usize,
    timeout: Duration,
) -> Result<(), String> {
    if maximum_request_bytes == 0
        || maximum_request_bytes > MAXIMUM_CONNECTOR_BODY_BYTES
        || maximum_response_bytes == 0
        || maximum_response_bytes > MAXIMUM_CONNECTOR_BODY_BYTES
        || timeout.is_zero()
        || timeout > MAXIMUM_REQUEST_TIMEOUT
    {
        return Err("resolved Connector HTTP limits are invalid".into());
    }
    Ok(())
}

pub(crate) fn validate_connector_signing_secret_length(length: usize) -> Result<(), String> {
    if !(MINIMUM_SIGNING_SECRET_BYTES..=MAXIMUM_SIGNING_SECRET_BYTES).contains(&length) {
        return Err("connector signing secret must contain between 32 and 4096 bytes".into());
    }
    Ok(())
}

pub(crate) const fn maximum_connector_retry_after() -> Duration {
    MAXIMUM_RETRY_AFTER
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(destination: ConnectorHttpDestination) -> ConnectorHttpDefinitionSpec {
        ConnectorHttpDefinitionSpec {
            destination,
            method: ConnectorHttpMethod::Post,
            request_content_type: "application/json; charset=utf-8".into(),
            maximum_request_bytes: 16 * 1024,
            maximum_response_bytes: 32 * 1024,
            timeout_milliseconds: 5_000,
            status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
            authentication: ConnectorHttpAuthentication::HmacSha256 {
                secret: ConnectorSecretReference::new(SecretId::new(), 3).expect("Secret"),
                signature_header: "x-a3s-signature".into(),
                value_prefix: "v1=".into(),
            },
        }
    }

    #[test]
    fn definition_is_canonical_acl_and_redacts_destination_and_authentication_metadata() {
        let definition =
            ConnectorHttpDefinition::from_spec(fixture(ConnectorHttpDestination::LiteralHttps {
                endpoint: "https://hooks.example.test/delivery".into(),
            }))
            .expect("definition");
        assert_eq!(
            ConnectorHttpDefinition::parse_acl(definition.canonical_acl()).expect("reparse"),
            definition
        );
        assert!(definition.canonical_acl().ends_with('\n'));
        assert!(definition.digest().as_str().starts_with("sha256:"));
        let debug = format!("{definition:?}");
        assert!(!debug.contains("hooks.example.test"));
        assert!(!debug.contains("v1="));
    }

    #[test]
    fn token_bearing_destination_must_be_a_secret_reference() {
        let literal = fixture(ConnectorHttpDestination::LiteralHttps {
            endpoint: "https://hooks.example.test/delivery?token=plaintext".into(),
        });
        assert!(ConnectorHttpDefinition::from_spec(literal).is_err());

        let destination = ConnectorSecretReference::new(SecretId::new(), 7).expect("destination");
        let definition =
            ConnectorHttpDefinition::from_spec(fixture(ConnectorHttpDestination::SecretHttpsUrl {
                reference: destination,
            }))
            .expect("secret destination");
        let bindings = definition.secret_bindings();
        assert_eq!(bindings.len(), 2);
        assert_eq!(
            bindings[0].purpose,
            ConnectorSecretBindingPurpose::Destination
        );
        assert_eq!(bindings[0].reference, destination);
    }

    #[test]
    fn parser_rejects_unknown_fields_noncanonical_bytes_and_digest_drift() {
        let definition =
            ConnectorHttpDefinition::from_spec(fixture(ConnectorHttpDestination::LiteralHttps {
                endpoint: "https://hooks.example.test/delivery".into(),
            }))
            .expect("definition");
        assert!(ConnectorHttpDefinition::parse_acl(
            &definition
                .canonical_acl()
                .replace("schema =", "unknown = \"x\"\n  schema =")
        )
        .is_err());
        assert!(ConnectorHttpDefinition::parse_acl(
            &definition.canonical_acl().replace("  method", "    method")
        )
        .is_err());
        assert!(ConnectorHttpDefinition::restore(
            definition.canonical_acl(),
            &format!("sha256:{}", "f".repeat(64))
        )
        .is_err());
    }

    #[test]
    fn shared_aut0_5_fixture_uses_the_owner_acl_parser() {
        let source = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/aut0.5/http-connector.acl"
        ));
        let definition =
            ConnectorHttpDefinition::parse_acl(source).expect("shared AUT0.5 Connector HTTP ACL");
        assert_eq!(definition.canonical_acl(), source);
        assert_eq!(definition.secret_bindings().len(), 2);
        assert!(matches!(
            &definition.spec().destination,
            ConnectorHttpDestination::SecretHttpsUrl { .. }
        ));
    }
}
