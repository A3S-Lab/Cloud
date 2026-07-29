//! Cross-product contract for hosted modern stateless MCP projections.

use argon2::password_hash::PasswordHash;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::net::IpAddr;
use std::time::Duration;
use url::Url;
use uuid::Uuid;

pub const MCP_PROTOCOL_VERSION: &str = "2026-07-28";
pub const MCP_CREDENTIAL_AUDIENCE: &str = "cloud-mcp";
pub const MCP_GATEWAY_PROJECTION_SCHEMA: &str = "a3s.cloud.mcp-gateway-projection.v1";

const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_HEADER_BYTES: u64 = 128 * 1024;
const MAX_REQUEST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_VERIFIER_BYTES: usize = 512;
const MIN_ARGON2_MEMORY_KIB: u32 = 19_456;
const MAX_ARGON2_MEMORY_KIB: u32 = 262_144;
const MIN_ARGON2_ITERATIONS: u32 = 2;
const MAX_ARGON2_ITERATIONS: u32 = 10;
const MAX_ARGON2_LANES: u32 = 4;
const MIN_ARGON2_SALT_ENCODED_LEN: usize = 22;
const MAX_ARGON2_SALT_ENCODED_LEN: usize = 86;
const MIN_ARGON2_OUTPUT_ENCODED_LEN: usize = 43;
const MAX_ARGON2_OUTPUT_ENCODED_LEN: usize = 86;

/// Complete immutable-profile and mutable-route input for one Gateway snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpGatewayProjection {
    pub schema: String,
    pub expires_at: DateTime<Utc>,
    pub profiles: Vec<McpServiceProfileProjection>,
    pub credentials: Vec<McpCredentialProjection>,
    pub routes: Vec<McpRoutePolicyProjection>,
}

impl McpGatewayProjection {
    pub fn validate(&self, now: DateTime<Utc>) -> Result<(), String> {
        if self.schema != MCP_GATEWAY_PROJECTION_SCHEMA {
            return Err(format!(
                "unsupported MCP Gateway projection schema {:?}",
                self.schema
            ));
        }
        if self.expires_at <= now {
            return Err("MCP Gateway projection has expired".into());
        }
        if self.profiles.is_empty() || self.profiles.len() > 1_000 {
            return Err("MCP Gateway projection must contain 1 to 1000 profiles".into());
        }
        if self.routes.is_empty() || self.routes.len() > 1_000 {
            return Err("MCP Gateway projection must contain 1 to 1000 routes".into());
        }
        if self.credentials.len() > 10_000 {
            return Err("MCP Gateway projection exceeds the credential limit".into());
        }

        let mut profiles = HashMap::new();
        for profile in &self.profiles {
            profile.validate()?;
            if profiles
                .insert(profile.profile_digest.as_str(), profile)
                .is_some()
            {
                return Err("MCP Gateway projection contains duplicate profile digests".into());
            }
        }

        let mut credential_ids = HashSet::new();
        let mut prefixes = HashSet::new();
        for credential in &self.credentials {
            credential.validate()?;
            if !credential_ids.insert(credential.credential_id)
                || !prefixes.insert(credential.prefix.as_str())
            {
                return Err(
                    "MCP Gateway projection contains duplicate credentials or prefixes".into(),
                );
            }
        }
        let mut prefixes = prefixes.into_iter().collect::<Vec<_>>();
        prefixes.sort_unstable();
        if prefixes.windows(2).any(|pair| pair[1].starts_with(pair[0])) {
            return Err("MCP credential prefixes must not overlap".into());
        }

        let mut route_ids = HashSet::new();
        let mut routers = HashSet::new();
        let mut target_ids = HashSet::new();
        let mut runtime_targets = HashSet::new();
        for route in &self.routes {
            if !route_ids.insert(route.route_id) || !routers.insert(route.router.as_str()) {
                return Err("MCP Gateway projection contains duplicate routes or routers".into());
            }
            route.validate(
                &profiles,
                &self.credentials,
                &mut target_ids,
                &mut runtime_targets,
            )?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServiceProfileProjection {
    pub profile_digest: String,
    pub protocol_versions: Vec<String>,
    pub path: String,
    pub request_sse: bool,
    pub subscriptions: bool,
    pub max_request_bytes: u64,
    pub max_response_bytes: u64,
}

impl McpServiceProfileProjection {
    fn validate(&self) -> Result<(), String> {
        validate_digest("MCP service profile", &self.profile_digest)?;
        if self.protocol_versions != [MCP_PROTOCOL_VERSION] {
            return Err(format!(
                "MCP service profile must support exactly {MCP_PROTOCOL_VERSION}"
            ));
        }
        validate_literal_path(&self.path)?;
        if self.subscriptions && !self.request_sse {
            return Err("MCP subscriptions require request-scoped SSE".into());
        }
        if self.max_request_bytes == 0
            || self.max_request_bytes > MAX_REQUEST_BYTES
            || self.max_response_bytes == 0
            || self.max_response_bytes > MAX_RESPONSE_BYTES
        {
            return Err("MCP service profile byte bounds are invalid".into());
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpCredentialProjection {
    pub credential_id: Uuid,
    pub environment_id: Uuid,
    pub audience: String,
    pub prefix: String,
    #[serde(skip_serializing)]
    verifier_hash: String,
    pub generation: u64,
    pub expires_at: DateTime<Utc>,
    pub revoked: bool,
}

impl McpCredentialProjection {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        credential_id: Uuid,
        environment_id: Uuid,
        audience: impl Into<String>,
        prefix: impl Into<String>,
        verifier_hash: impl Into<String>,
        generation: u64,
        expires_at: DateTime<Utc>,
        revoked: bool,
    ) -> Self {
        Self {
            credential_id,
            environment_id,
            audience: audience.into(),
            prefix: prefix.into(),
            verifier_hash: verifier_hash.into(),
            generation,
            expires_at,
            revoked,
        }
    }

    pub fn verifier_hash(&self) -> &str {
        &self.verifier_hash
    }

    fn validate(&self) -> Result<(), String> {
        if self.credential_id.is_nil() || self.environment_id.is_nil() {
            return Err("MCP credential and environment IDs must not be nil".into());
        }
        if self.audience != MCP_CREDENTIAL_AUDIENCE {
            return Err("MCP credential audience is invalid".into());
        }
        validate_acl_integer("MCP credential generation", self.generation)?;
        let Some(suffix) = self.prefix.strip_prefix("a3s_mcp_") else {
            return Err("MCP credential prefix must start with a3s_mcp_".into());
        };
        if suffix.len() < 8
            || suffix.len() > 32
            || !suffix
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        {
            return Err("MCP credential prefix suffix is invalid".into());
        }
        validate_argon2id_verifier(&self.verifier_hash)
    }
}

impl fmt::Debug for McpCredentialProjection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("McpCredentialProjection")
            .field("credential_id", &self.credential_id)
            .field("environment_id", &self.environment_id)
            .field("audience", &self.audience)
            .field("prefix", &self.prefix)
            .field("verifier_hash", &"<redacted>")
            .field("generation", &self.generation)
            .field("expires_at", &self.expires_at)
            .field("revoked", &self.revoked)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpRoutePolicyProjection {
    pub route_id: Uuid,
    pub router: String,
    pub environment_id: Uuid,
    pub policy_revision: u64,
    pub profile_digest: String,
    pub allowed_origins: Vec<String>,
    pub max_header_bytes: u64,
    pub max_request_bytes: u64,
    pub max_response_bytes: u64,
    pub first_response_timeout: String,
    pub stream_idle_timeout: String,
    pub stream_total_timeout: String,
    pub drain_timeout: String,
    pub telemetry_names: Vec<String>,
    pub targets: Vec<McpTargetProjection>,
    pub grants: Vec<McpGrantProjection>,
}

impl McpRoutePolicyProjection {
    fn validate<'a>(
        &'a self,
        profiles: &HashMap<&str, &McpServiceProfileProjection>,
        credentials: &[McpCredentialProjection],
        target_ids: &mut HashSet<Uuid>,
        runtime_targets: &mut HashSet<(Uuid, &'a str, u64)>,
    ) -> Result<(), String> {
        if self.route_id.is_nil() || self.environment_id.is_nil() {
            return Err("MCP route and environment IDs must not be nil".into());
        }
        validate_acl_integer("MCP route policy revision", self.policy_revision)?;
        validate_name("MCP router", &self.router, 255)?;
        validate_digest("MCP route profile", &self.profile_digest)?;
        let profile = profiles
            .get(self.profile_digest.as_str())
            .ok_or_else(|| "MCP route references an unknown service profile".to_string())?;
        if self.max_header_bytes == 0
            || self.max_header_bytes > MAX_HEADER_BYTES
            || self.max_request_bytes == 0
            || self.max_request_bytes > profile.max_request_bytes
            || self.max_response_bytes == 0
            || self.max_response_bytes > profile.max_response_bytes
        {
            return Err("MCP route byte bounds are invalid or exceed its service profile".into());
        }
        let first = validate_duration(&self.first_response_timeout, Duration::from_secs(5 * 60))?;
        let idle = validate_duration(&self.stream_idle_timeout, Duration::from_secs(60 * 60))?;
        let total = validate_duration(
            &self.stream_total_timeout,
            Duration::from_secs(24 * 60 * 60),
        )?;
        validate_duration(&self.drain_timeout, Duration::from_secs(10 * 60))?;
        if first > total || idle > total {
            return Err(
                "MCP stream total timeout must cover first-response and idle timeouts".into(),
            );
        }
        validate_origins(&self.allowed_origins)?;
        validate_names("MCP telemetry name", &self.telemetry_names, 256)?;
        if self.targets.is_empty() || self.targets.len() > 64 {
            return Err("MCP route must contain 1 to 64 targets".into());
        }
        if self.grants.is_empty() || self.grants.len() > 10_000 {
            return Err("MCP route must contain 1 to 10000 grants".into());
        }

        let mut services = HashSet::new();
        let mut priorities = BTreeSet::new();
        let mut weights = HashMap::<u32, u64>::new();
        for target in &self.targets {
            target.validate(self, target_ids, runtime_targets)?;
            if !services.insert(target.service.as_str()) {
                return Err("MCP route target services must be unique".into());
            }
            priorities.insert(target.priority);
            let total = weights.entry(target.priority).or_default();
            *total = total
                .checked_add(u64::from(target.weight))
                .ok_or_else(|| "MCP target weight total overflowed".to_string())?;
            if *total > u64::from(u32::MAX) {
                return Err("MCP target weight total exceeds u32".into());
            }
        }
        if priorities
            .iter()
            .copied()
            .enumerate()
            .any(|(expected, actual)| usize::try_from(actual) != Ok(expected))
        {
            return Err("MCP target priorities must be contiguous from zero".into());
        }

        let mut grant_ids = HashSet::new();
        for grant in &self.grants {
            if !grant_ids.insert(grant.credential_id) {
                return Err("MCP route contains duplicate credential grants".into());
            }
            grant.validate(self, credentials)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpTargetProjection {
    pub target_id: Uuid,
    pub node_id: Uuid,
    pub asset_release_id: Uuid,
    pub unit_id: String,
    pub generation: u64,
    pub service: String,
    pub endpoint: String,
    pub profile_digest: String,
    pub priority: u32,
    pub weight: u32,
}

impl McpTargetProjection {
    fn validate<'a>(
        &'a self,
        route: &McpRoutePolicyProjection,
        target_ids: &mut HashSet<Uuid>,
        runtime_targets: &mut HashSet<(Uuid, &'a str, u64)>,
    ) -> Result<(), String> {
        if self.target_id.is_nil()
            || self.node_id.is_nil()
            || self.asset_release_id.is_nil()
            || !target_ids.insert(self.target_id)
        {
            return Err("MCP target IDs must be non-nil and globally unique".into());
        }
        validate_name("MCP Runtime unit ID", &self.unit_id, 512)?;
        validate_acl_integer("MCP Runtime generation", self.generation)?;
        validate_name("MCP target service", &self.service, 255)?;
        if self.profile_digest != route.profile_digest {
            return Err("MCP route contains mixed profile digests".into());
        }
        validate_loopback_endpoint(&self.endpoint)?;
        if self.weight == 0 {
            return Err("MCP target weight must be positive".into());
        }
        if !runtime_targets.insert((self.node_id, self.unit_id.as_str(), self.generation)) {
            return Err("MCP route contains a duplicate Runtime observation".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpGrantProjection {
    pub credential_id: Uuid,
    pub credential_generation: u64,
    pub methods: Vec<String>,
    pub names: Vec<String>,
    pub limits: McpLimitsProjection,
}

impl McpGrantProjection {
    fn validate(
        &self,
        route: &McpRoutePolicyProjection,
        credentials: &[McpCredentialProjection],
    ) -> Result<(), String> {
        let credential = credentials
            .iter()
            .find(|credential| credential.credential_id == self.credential_id)
            .ok_or_else(|| "MCP grant references an unknown credential".to_string())?;
        if credential.environment_id != route.environment_id
            || credential.revoked
            || self.credential_generation != credential.generation
        {
            return Err("MCP grant credential scope, state, or generation is invalid".into());
        }
        if self.methods.is_empty() || self.methods.len() > 256 {
            return Err("MCP grant must contain 1 to 256 methods".into());
        }
        let mut methods = HashSet::new();
        for method in &self.methods {
            if !methods.insert(method.as_str()) || !valid_method(method) {
                return Err("MCP grant contains an invalid or duplicate method".into());
            }
        }
        if !methods.contains("server/discover") {
            return Err("MCP grant must allow mandatory server/discover".into());
        }
        validate_names("MCP grant name", &self.names, 1_000)?;
        self.limits.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpLimitsProjection {
    pub max_concurrent_requests: u64,
    pub requests_per_minute: u64,
    pub request_burst: u64,
}

impl McpLimitsProjection {
    fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("max concurrent requests", self.max_concurrent_requests),
            ("requests per minute", self.requests_per_minute),
            ("request burst", self.request_burst),
        ] {
            validate_acl_integer(name, value)?;
        }
        if self.max_concurrent_requests > 100_000
            || self.requests_per_minute > 10_000_000
            || self.request_burst > self.requests_per_minute
        {
            return Err("MCP grant limits exceed their bounded maximum".into());
        }
        Ok(())
    }
}

fn validate_digest(context: &str, digest: &str) -> Result<(), String> {
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return Err(format!("{context} digest must use sha256:<hex>"));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "{context} digest must contain 64 lowercase hexadecimal digits"
        ));
    }
    Ok(())
}

fn validate_acl_integer(context: &str, value: u64) -> Result<(), String> {
    if value == 0 || value > MAX_SAFE_ACL_INTEGER {
        return Err(format!(
            "{context} must be a positive exactly representable ACL integer"
        ));
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
        return Err("MCP service profile path must be one safe literal path".into());
    }
    Ok(())
}

fn validate_duration(duration: &str, maximum: Duration) -> Result<Duration, String> {
    if duration.is_empty() || duration.chars().any(char::is_whitespace) {
        return Err("MCP duration is invalid".into());
    }
    let (value, unit) = if let Some(value) = duration.strip_suffix("ms") {
        (value, "ms")
    } else if let Some(value) = duration.strip_suffix('s') {
        (value, "s")
    } else if let Some(value) = duration.strip_suffix('m') {
        (value, "m")
    } else {
        (duration, "s")
    };
    let value = value
        .parse::<u64>()
        .map_err(|_| "MCP duration is invalid".to_string())?;
    if value == 0 {
        return Err("MCP duration is invalid".into());
    }
    let parsed = match unit {
        "ms" => Duration::from_millis(value),
        "s" => Duration::from_secs(value),
        "m" => Duration::from_secs(
            value
                .checked_mul(60)
                .ok_or_else(|| "MCP duration is too large".to_string())?,
        ),
        _ => unreachable!("duration unit is closed"),
    };
    if parsed > maximum {
        return Err("MCP duration exceeds its bounded maximum".into());
    }
    Ok(parsed)
}

fn validate_origins(origins: &[String]) -> Result<(), String> {
    if origins.len() > 256 {
        return Err("MCP route exceeds the allowed-origin limit".into());
    }
    let mut unique = HashSet::new();
    for origin in origins {
        let parsed = Url::parse(origin).map_err(|_| "MCP allowed origin is invalid".to_string())?;
        let loopback = parsed
            .host_str()
            .and_then(|host| host.parse::<IpAddr>().ok())
            .is_some_and(|address| address.is_loopback());
        if !matches!(parsed.scheme(), "http" | "https")
            || (parsed.scheme() == "http" && !loopback)
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.path() != "/"
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err("MCP allowed origin must be unique and safe".into());
        }
        let normalized = format!(
            "{}://{}{}",
            parsed.scheme(),
            parsed.host_str().unwrap_or_default(),
            parsed
                .port()
                .map(|port| format!(":{port}"))
                .unwrap_or_default()
        );
        if !unique.insert(normalized) {
            return Err("MCP allowed origin must be unique and safe".into());
        }
    }
    Ok(())
}

fn validate_loopback_endpoint(endpoint: &str) -> Result<(), String> {
    let parsed = Url::parse(endpoint).map_err(|_| "MCP target endpoint is invalid".to_string())?;
    let loopback = parsed
        .host_str()
        .and_then(|host| host.parse::<IpAddr>().ok())
        .is_some_and(|address| address.is_loopback());
    if parsed.scheme() != "http"
        || !loopback
        || parsed.port().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("MCP target must be an explicit loopback HTTP endpoint".into());
    }
    Ok(())
}

fn validate_names(context: &str, names: &[String], maximum: usize) -> Result<(), String> {
    if names.len() > maximum {
        return Err(format!("{context} list exceeds its limit"));
    }
    let mut unique = HashSet::new();
    for name in names {
        if !unique.insert(name.as_str())
            || validate_name(context, name, 255).is_err()
            || name.contains("://")
            || name.starts_with('/')
        {
            return Err(format!(
                "{context} must be unique, non-resource, and bounded"
            ));
        }
    }
    Ok(())
}

fn validate_name(context: &str, value: &str, maximum: usize) -> Result<(), String> {
    if value.is_empty()
        || value.len() > maximum
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(format!("{context} is invalid"));
    }
    Ok(())
}

fn valid_method(method: &str) -> bool {
    !method.is_empty()
        && method.len() <= 255
        && !legacy_or_wrong_direction_method(method)
        && method.split('/').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        })
}

fn legacy_or_wrong_direction_method(method: &str) -> bool {
    matches!(
        method,
        "initialize"
            | "notifications/initialized"
            | "notifications/cancelled"
            | "ping"
            | "logging/setLevel"
            | "resources/subscribe"
            | "resources/unsubscribe"
            | "roots/list"
            | "sampling/createMessage"
            | "elicitation/create"
    )
}

fn validate_argon2id_verifier(verifier_hash: &str) -> Result<(), String> {
    if verifier_hash.is_empty() || verifier_hash.len() > MAX_VERIFIER_BYTES {
        return Err("MCP credential verifier_hash has an invalid byte length".into());
    }
    PasswordHash::new(verifier_hash)
        .map_err(|_| "MCP credential verifier_hash is not a valid PHC string".to_string())?;
    let parts = verifier_hash.split('$').collect::<Vec<_>>();
    if parts.len() != 6 || parts[1] != "argon2id" || parts[2] != "v=19" {
        return Err("MCP credential verifier_hash must use Argon2id PHC version 19".into());
    }
    let mut memory = None;
    let mut iterations = None;
    let mut lanes = None;
    for parameter in parts[3].split(',') {
        let (name, value) = parameter
            .split_once('=')
            .ok_or_else(|| "MCP credential verifier has invalid parameters".to_string())?;
        let value = value
            .parse::<u32>()
            .map_err(|_| "MCP credential verifier has invalid parameters".to_string())?;
        match name {
            "m" if memory.replace(value).is_none() => {}
            "t" if iterations.replace(value).is_none() => {}
            "p" if lanes.replace(value).is_none() => {}
            _ => {
                return Err(
                    "MCP credential verifier must contain exactly m, t, and p parameters".into(),
                );
            }
        }
    }
    let (Some(memory), Some(iterations), Some(lanes)) = (memory, iterations, lanes) else {
        return Err("MCP credential verifier is missing Argon2id parameters".into());
    };
    if !(MIN_ARGON2_MEMORY_KIB..=MAX_ARGON2_MEMORY_KIB).contains(&memory)
        || !(MIN_ARGON2_ITERATIONS..=MAX_ARGON2_ITERATIONS).contains(&iterations)
        || !(1..=MAX_ARGON2_LANES).contains(&lanes)
        || !(MIN_ARGON2_SALT_ENCODED_LEN..=MAX_ARGON2_SALT_ENCODED_LEN).contains(&parts[4].len())
        || !(MIN_ARGON2_OUTPUT_ENCODED_LEN..=MAX_ARGON2_OUTPUT_ENCODED_LEN)
            .contains(&parts[5].len())
    {
        return Err("MCP credential verifier uses unsupported Argon2id bounds".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn projection() -> McpGatewayProjection {
        let environment_id = Uuid::new_v4();
        let credential_id = Uuid::new_v4();
        let profile_digest =
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        McpGatewayProjection {
            schema: MCP_GATEWAY_PROJECTION_SCHEMA.into(),
            expires_at: Utc::now() + Duration::hours(1),
            profiles: vec![McpServiceProfileProjection {
                profile_digest: profile_digest.into(),
                protocol_versions: vec![MCP_PROTOCOL_VERSION.into()],
                path: "/mcp".into(),
                request_sse: true,
                subscriptions: true,
                max_request_bytes: 1_048_576,
                max_response_bytes: 8_388_608,
            }],
            credentials: vec![McpCredentialProjection::new(
                credential_id,
                environment_id,
                MCP_CREDENTIAL_AUDIENCE,
                "a3s_mcp_abc12345",
                "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                7,
                Utc::now() + Duration::minutes(30),
                false,
            )],
            routes: vec![McpRoutePolicyProjection {
                route_id: Uuid::new_v4(),
                router: "mcp".into(),
                environment_id,
                policy_revision: 1,
                profile_digest: profile_digest.into(),
                allowed_origins: vec!["https://console.example.com".into()],
                max_header_bytes: 32_768,
                max_request_bytes: 524_288,
                max_response_bytes: 4_194_304,
                first_response_timeout: "30s".into(),
                stream_idle_timeout: "2m".into(),
                stream_total_timeout: "30m".into(),
                drain_timeout: "30s".into(),
                telemetry_names: vec!["weather".into()],
                targets: vec![McpTargetProjection {
                    target_id: Uuid::new_v4(),
                    node_id: Uuid::new_v4(),
                    asset_release_id: Uuid::new_v4(),
                    unit_id: "workload:mcp-weather:replica:1".into(),
                    generation: 3,
                    service: "mcp-target-1".into(),
                    endpoint: "http://127.0.0.1:8000/".into(),
                    profile_digest: profile_digest.into(),
                    priority: 0,
                    weight: 100,
                }],
                grants: vec![McpGrantProjection {
                    credential_id,
                    credential_generation: 7,
                    methods: vec![
                        "server/discover".into(),
                        "tools/list".into(),
                        "tools/call".into(),
                    ],
                    names: vec!["weather".into()],
                    limits: McpLimitsProjection {
                        max_concurrent_requests: 8,
                        requests_per_minute: 120,
                        request_burst: 16,
                    },
                }],
            }],
        }
    }

    #[test]
    fn validates_modern_stateless_projection_and_redacts_verifier() {
        let projection = projection();
        projection.validate(Utc::now()).expect("valid projection");
        let debug = format!("{projection:?}");
        let json = serde_json::to_string(&projection).expect("projection JSON");
        assert!(!debug.contains("$argon2id$"));
        assert!(!json.contains("$argon2id$"));
    }

    #[test]
    fn rejects_legacy_protocol_and_mixed_target_profile() {
        let mut legacy = projection();
        legacy.profiles[0].protocol_versions = vec!["2025-06-18".into()];
        assert!(legacy
            .validate(Utc::now())
            .expect_err("legacy must fail")
            .contains("exactly"));

        let mut mixed = projection();
        mixed.routes[0].targets[0].profile_digest =
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into();
        assert!(mixed
            .validate(Utc::now())
            .expect_err("mixed profiles must fail")
            .contains("mixed"));
    }

    #[test]
    fn rejects_legacy_and_server_to_client_methods_in_modern_grants() {
        for method in [
            "initialize",
            "notifications/initialized",
            "notifications/cancelled",
            "ping",
            "logging/setLevel",
            "resources/subscribe",
            "resources/unsubscribe",
            "roots/list",
            "sampling/createMessage",
            "elicitation/create",
        ] {
            let mut projection = projection();
            projection.routes[0].grants[0].methods.push(method.into());
            let error = projection
                .validate(Utc::now())
                .expect_err("legacy or wrong-direction method must fail");
            assert!(
                error.contains("invalid or duplicate method"),
                "expected {method:?} to be rejected, got {error}"
            );
        }
    }
}
