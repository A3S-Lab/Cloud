use crate::modules::assets::domain::McpServiceProfile;
use crate::modules::edge::domain::RouteHostname;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AssetId, AssetReleaseId, DomainClaimId, EnvironmentId, GatewayScopeId,
    OrganizationId, ProjectId, RouteId, Sha256Digest, WorkloadId,
};
use a3s_acl::builder::{boolean, integer, list, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use a3s_cloud_contracts::{
    validate_mcp_allowed_origins, validate_mcp_telemetry_names, McpGrantProjection,
    McpLimitsProjection, McpRoutePolicyProjection, McpTargetProjection,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use url::Url;
use uuid::Uuid;

const POLICY_BLOCK: &str = "mcp_route_policy";
const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;
pub const MCP_ROUTE_POLICY_MAX_ACL_BYTES: usize = 512 * 1024;
const MAX_HEADER_BYTES: u64 = 128 * 1024;
const MAX_TELEMETRY_EVENTS_PER_MINUTE: u64 = 10_000_000;
const MAX_POLICY_LIFETIME_HOURS: i64 = 24;
const POLICY_ATTRIBUTES: [&str; 25] = [
    "allowed_origins",
    "asset_id",
    "asset_release_id",
    "audit_required",
    "domain_claim_id",
    "drain_timeout_seconds",
    "environment_id",
    "expires_at",
    "first_response_timeout_seconds",
    "gateway_scope_id",
    "hostname",
    "max_header_bytes",
    "max_request_bytes",
    "max_response_bytes",
    "organization_id",
    "path",
    "policy_revision",
    "profile_digest",
    "project_id",
    "stream_idle_timeout_seconds",
    "stream_total_timeout_seconds",
    "telemetry_events_per_minute",
    "telemetry_names",
    "tls_required",
    "workload_id",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpRoutePolicySpec {
    pub route_id: RouteId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub gateway_scope_id: GatewayScopeId,
    pub domain_claim_id: DomainClaimId,
    pub workload_id: WorkloadId,
    pub asset_id: AssetId,
    pub asset_release_id: AssetReleaseId,
    pub profile_digest: Sha256Digest,
    pub hostname: RouteHostname,
    pub path: String,
    pub tls_required: bool,
    pub allowed_origins: Vec<String>,
    pub max_header_bytes: u64,
    pub max_request_bytes: u64,
    pub max_response_bytes: u64,
    pub first_response_timeout_seconds: u64,
    pub stream_idle_timeout_seconds: u64,
    pub stream_total_timeout_seconds: u64,
    pub drain_timeout_seconds: u64,
    pub telemetry_names: Vec<String>,
    pub telemetry_events_per_minute: u64,
    pub audit_required: bool,
    pub expires_at: DateTime<Utc>,
    pub grants: Vec<McpGrantProjection>,
}

/// Parsed, canonical desired-state input for one MCP route policy mutation.
///
/// Parsing and canonicalization do not depend on persistence timestamps or on
/// the referenced immutable Service profile. The repository can therefore
/// resolve an idempotent replay before revalidating an expired historical
/// request against the current clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpRoutePolicyDocument {
    spec: McpRoutePolicySpec,
    policy_revision: u64,
    canonical_acl: String,
    policy_digest: Sha256Digest,
}

impl McpRoutePolicyDocument {
    pub const fn spec(&self) -> &McpRoutePolicySpec {
        &self.spec
    }

    pub const fn policy_revision(&self) -> u64 {
        self.policy_revision
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn policy_digest(&self) -> &Sha256Digest {
        &self.policy_digest
    }

    pub fn materialize(
        &self,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        profile: &McpServiceProfile,
    ) -> Result<McpRoutePolicy, String> {
        let policy = McpRoutePolicy::build(
            self.spec.clone(),
            self.policy_revision,
            canonical_timestamp(created_at),
            canonical_timestamp(updated_at),
            profile,
        )?;
        if policy.canonical_acl != self.canonical_acl || policy.policy_digest != self.policy_digest
        {
            return Err("MCP route policy document changed during admission".into());
        }
        Ok(policy)
    }
}

/// Mutable Edge desired state. Each accepted mutation advances the policy
/// revision and therefore its canonical digest. Runtime targets and verifier
/// hashes are deliberately absent and are resolved only during reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpRoutePolicy {
    spec: McpRoutePolicySpec,
    policy_revision: u64,
    canonical_acl: String,
    policy_digest: Sha256Digest,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl McpRoutePolicy {
    pub fn parse_acl(acl: &str) -> Result<McpRoutePolicyDocument, String> {
        if acl.is_empty() || acl.len() > MCP_ROUTE_POLICY_MAX_ACL_BYTES {
            return Err("MCP route policy ACL size is invalid".into());
        }
        let document =
            parse_acl(acl).map_err(|error| format!("MCP route policy ACL is invalid: {error}"))?;
        let (mut spec, policy_revision) = parse_policy_document(&document)?;
        normalize_spec(&mut spec)?;
        if policy_revision == 0 || policy_revision > MAX_SAFE_ACL_INTEGER {
            return Err("MCP route policy revision is invalid".into());
        }
        let canonical_document = policy_document(&spec, policy_revision)?;
        let canonical_acl = generate_acl(&canonical_document);
        if canonical_acl.len() > MCP_ROUTE_POLICY_MAX_ACL_BYTES {
            return Err("MCP route policy ACL exceeds its storage bound".into());
        }
        let canonical_document = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated MCP route policy ACL is invalid: {error}"))?;
        let policy_digest = Sha256Digest::parse(
            canonical_digest(&canonical_document)
                .map_err(|error| format!("MCP route policy is not canonicalizable: {error}"))?,
        )?;
        Ok(McpRoutePolicyDocument {
            spec,
            policy_revision,
            canonical_acl,
            policy_digest,
        })
    }

    pub fn create(
        spec: McpRoutePolicySpec,
        profile: &McpServiceProfile,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let created_at = canonical_timestamp(created_at);
        Self::build(spec, 1, created_at, created_at, profile)
    }

    pub fn revise(
        &mut self,
        mut spec: McpRoutePolicySpec,
        profile: &McpServiceProfile,
        updated_at: DateTime<Utc>,
    ) -> Result<bool, String> {
        normalize_spec(&mut spec)?;
        if spec == self.spec {
            return Ok(false);
        }
        if spec.route_id != self.spec.route_id
            || spec.organization_id != self.spec.organization_id
            || spec.project_id != self.spec.project_id
            || spec.environment_id != self.spec.environment_id
            || spec.gateway_scope_id != self.spec.gateway_scope_id
            || spec.workload_id != self.spec.workload_id
            || spec.asset_id != self.spec.asset_id
        {
            return Err("MCP route policy ownership is immutable".into());
        }
        let updated_at = canonical_timestamp(updated_at);
        if updated_at < self.updated_at {
            return Err("MCP route policy update time regressed".into());
        }
        let policy_revision = self
            .policy_revision
            .checked_add(1)
            .ok_or_else(|| "MCP route policy revision space is exhausted".to_owned())?;
        let updated = Self::build(spec, policy_revision, self.created_at, updated_at, profile)?;
        *self = updated;
        Ok(true)
    }

    pub fn restore(
        acl: &str,
        stored_digest: &str,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        profile: &McpServiceProfile,
    ) -> Result<Self, String> {
        if acl.is_empty() || acl.len() > MCP_ROUTE_POLICY_MAX_ACL_BYTES {
            return Err("stored MCP route policy ACL size is invalid".into());
        }
        let document =
            parse_acl(acl).map_err(|error| format!("MCP route policy ACL is invalid: {error}"))?;
        let (spec, policy_revision) = parse_policy_document(&document)?;
        let policy = Self::build(
            spec,
            policy_revision,
            canonical_timestamp(created_at),
            canonical_timestamp(updated_at),
            profile,
        )?;
        if policy.policy_digest.as_str() != stored_digest {
            return Err("stored MCP route policy ACL and digest do not match".into());
        }
        Ok(policy)
    }

    fn build(
        mut spec: McpRoutePolicySpec,
        policy_revision: u64,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        profile: &McpServiceProfile,
    ) -> Result<Self, String> {
        normalize_spec(&mut spec)?;
        validate_spec(&spec, policy_revision, created_at, updated_at, profile)?;
        let document = policy_document(&spec, policy_revision)?;
        let canonical_acl = generate_acl(&document);
        if canonical_acl.len() > MCP_ROUTE_POLICY_MAX_ACL_BYTES {
            return Err("MCP route policy ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated MCP route policy ACL is invalid: {error}"))?;
        let policy_digest = Sha256Digest::parse(
            canonical_digest(&reparsed)
                .map_err(|error| format!("MCP route policy is not canonicalizable: {error}"))?,
        )?;
        Ok(Self {
            spec,
            policy_revision,
            canonical_acl,
            policy_digest,
            created_at,
            updated_at,
        })
    }

    pub const fn spec(&self) -> &McpRoutePolicySpec {
        &self.spec
    }

    pub const fn policy_revision(&self) -> u64 {
        self.policy_revision
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn policy_digest(&self) -> &Sha256Digest {
        &self.policy_digest
    }

    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    pub(crate) fn gateway_projection(
        &self,
        router: impl Into<String>,
        targets: Vec<McpTargetProjection>,
    ) -> McpRoutePolicyProjection {
        McpRoutePolicyProjection {
            route_id: self.spec.route_id.as_uuid(),
            router: router.into(),
            environment_id: self.spec.environment_id.as_uuid(),
            policy_revision: self.policy_revision,
            profile_digest: self.spec.profile_digest.to_string(),
            allowed_origins: self.spec.allowed_origins.clone(),
            max_header_bytes: self.spec.max_header_bytes,
            max_request_bytes: self.spec.max_request_bytes,
            max_response_bytes: self.spec.max_response_bytes,
            first_response_timeout: format!("{}s", self.spec.first_response_timeout_seconds),
            stream_idle_timeout: format!("{}s", self.spec.stream_idle_timeout_seconds),
            stream_total_timeout: format!("{}s", self.spec.stream_total_timeout_seconds),
            drain_timeout: format!("{}s", self.spec.drain_timeout_seconds),
            telemetry_names: self.spec.telemetry_names.clone(),
            targets,
            grants: self.spec.grants.clone(),
        }
    }
}

fn normalize_spec(spec: &mut McpRoutePolicySpec) -> Result<(), String> {
    spec.expires_at = canonical_timestamp(spec.expires_at);
    validate_mcp_allowed_origins(&spec.allowed_origins)?;
    spec.allowed_origins = spec
        .allowed_origins
        .iter()
        .map(|origin| {
            Url::parse(origin)
                .map(|parsed| parsed.origin().ascii_serialization())
                .map_err(|_| "MCP allowed origin is invalid".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    spec.allowed_origins.sort_unstable();
    spec.telemetry_names.sort_unstable();
    for grant in &mut spec.grants {
        grant.methods.sort_unstable();
        grant.names.sort_unstable();
    }
    spec.grants.sort_by_key(|grant| grant.credential_id);
    Ok(())
}

fn validate_spec(
    spec: &McpRoutePolicySpec,
    policy_revision: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    profile: &McpServiceProfile,
) -> Result<(), String> {
    if spec.route_id.as_uuid().is_nil()
        || spec.organization_id.as_uuid().is_nil()
        || spec.project_id.as_uuid().is_nil()
        || spec.environment_id.as_uuid().is_nil()
        || spec.gateway_scope_id.as_uuid().is_nil()
        || spec.domain_claim_id.as_uuid().is_nil()
        || spec.workload_id.as_uuid().is_nil()
        || spec.asset_id.as_uuid().is_nil()
        || spec.asset_release_id.as_uuid().is_nil()
        || policy_revision == 0
        || policy_revision > MAX_SAFE_ACL_INTEGER
    {
        return Err("MCP route policy identity or revision is invalid".into());
    }
    if spec.profile_digest != *profile.digest() {
        return Err("MCP route policy references a different Service profile".into());
    }
    if !spec.tls_required {
        return Err("hosted MCP routes require TLS".into());
    }
    validate_literal_path(&spec.path)?;
    if spec.path != profile.spec().endpoint_path {
        return Err("MCP public route path must match the immutable Service endpoint".into());
    }
    validate_mcp_allowed_origins(&spec.allowed_origins)?;
    if spec.max_header_bytes == 0
        || spec.max_header_bytes > MAX_HEADER_BYTES
        || spec.max_request_bytes == 0
        || spec.max_request_bytes > profile.spec().max_request_bytes
        || spec.max_response_bytes == 0
        || spec.max_response_bytes > profile.spec().max_response_bytes
    {
        return Err("MCP route byte bounds are invalid or exceed its Service profile".into());
    }
    for (label, value, maximum) in [
        (
            "first-response timeout",
            spec.first_response_timeout_seconds,
            5 * 60,
        ),
        (
            "stream idle timeout",
            spec.stream_idle_timeout_seconds,
            60 * 60,
        ),
        (
            "stream total timeout",
            spec.stream_total_timeout_seconds,
            profile.spec().max_stream_seconds,
        ),
        ("drain timeout", spec.drain_timeout_seconds, 10 * 60),
    ] {
        if value == 0 || value > maximum {
            return Err(format!("MCP route {label} is invalid"));
        }
    }
    if spec.first_response_timeout_seconds > spec.stream_total_timeout_seconds
        || spec.stream_idle_timeout_seconds > spec.stream_total_timeout_seconds
    {
        return Err("MCP stream total timeout must cover first-response and idle timeouts".into());
    }
    validate_mcp_telemetry_names(&spec.telemetry_names)?;
    if spec.telemetry_events_per_minute == 0
        || spec.telemetry_events_per_minute > MAX_TELEMETRY_EVENTS_PER_MINUTE
    {
        return Err("MCP route telemetry budget is invalid".into());
    }
    if spec.grants.is_empty() || spec.grants.len() > 10_000 {
        return Err("MCP route must contain 1 to 10000 credential grants".into());
    }
    let mut credential_ids = HashSet::new();
    for grant in &spec.grants {
        if !credential_ids.insert(grant.credential_id) {
            return Err("MCP route contains duplicate credential grants".into());
        }
        grant.validate_reference()?;
    }
    if created_at != canonical_timestamp(created_at)
        || updated_at != canonical_timestamp(updated_at)
        || updated_at < created_at
        || spec.expires_at <= updated_at
        || spec.expires_at > updated_at + Duration::hours(MAX_POLICY_LIFETIME_HOURS)
    {
        return Err("MCP route policy timestamps or expiry are invalid".into());
    }
    Ok(())
}

fn policy_document(spec: &McpRoutePolicySpec, policy_revision: u64) -> Result<Document, String> {
    let mut block = BlockBuilder::new(POLICY_BLOCK)
        .label(&spec.route_id.to_string())
        .attr("organization_id", string(&spec.organization_id.to_string()))
        .attr("project_id", string(&spec.project_id.to_string()))
        .attr("environment_id", string(&spec.environment_id.to_string()))
        .attr(
            "gateway_scope_id",
            string(&spec.gateway_scope_id.to_string()),
        )
        .attr("domain_claim_id", string(&spec.domain_claim_id.to_string()))
        .attr("workload_id", string(&spec.workload_id.to_string()))
        .attr("asset_id", string(&spec.asset_id.to_string()))
        .attr(
            "asset_release_id",
            string(&spec.asset_release_id.to_string()),
        )
        .attr("profile_digest", string(spec.profile_digest.as_str()))
        .attr("hostname", string(spec.hostname.as_str()))
        .attr("path", string(&spec.path))
        .attr("tls_required", boolean(spec.tls_required))
        .attr("allowed_origins", string_list(&spec.allowed_origins))
        .attr(
            "max_header_bytes",
            acl_integer("maximum header bytes", spec.max_header_bytes)?,
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
            "first_response_timeout_seconds",
            acl_integer(
                "first-response timeout",
                spec.first_response_timeout_seconds,
            )?,
        )
        .attr(
            "stream_idle_timeout_seconds",
            acl_integer("stream idle timeout", spec.stream_idle_timeout_seconds)?,
        )
        .attr(
            "stream_total_timeout_seconds",
            acl_integer("stream total timeout", spec.stream_total_timeout_seconds)?,
        )
        .attr(
            "drain_timeout_seconds",
            acl_integer("drain timeout", spec.drain_timeout_seconds)?,
        )
        .attr("telemetry_names", string_list(&spec.telemetry_names))
        .attr(
            "telemetry_events_per_minute",
            acl_integer(
                "telemetry events per minute",
                spec.telemetry_events_per_minute,
            )?,
        )
        .attr("audit_required", boolean(spec.audit_required))
        .attr("expires_at", string(&spec.expires_at.to_rfc3339()))
        .attr(
            "policy_revision",
            acl_integer("policy revision", policy_revision)?,
        );
    for grant in &spec.grants {
        block = block.nested_block(grant_block(grant)?);
    }
    Ok(Document {
        blocks: vec![block.build()],
    })
}

fn grant_block(grant: &McpGrantProjection) -> Result<Block, String> {
    Ok(BlockBuilder::new("grants")
        .label(&grant.credential_id.to_string())
        .attr(
            "credential_generation",
            acl_integer("credential generation", grant.credential_generation)?,
        )
        .attr("methods", string_list(&grant.methods))
        .attr("names", string_list(&grant.names))
        .nested_block(
            BlockBuilder::new("limits")
                .attr(
                    "max_concurrent_requests",
                    acl_integer(
                        "maximum concurrent requests",
                        grant.limits.max_concurrent_requests,
                    )?,
                )
                .attr(
                    "requests_per_minute",
                    acl_integer("requests per minute", grant.limits.requests_per_minute)?,
                )
                .attr(
                    "request_burst",
                    acl_integer("request burst", grant.limits.request_burst)?,
                )
                .build(),
        )
        .build())
}

fn parse_policy_document(document: &Document) -> Result<(McpRoutePolicySpec, u64), String> {
    if document.blocks.len() != 1 {
        return Err("MCP route policy must contain exactly one top-level block".into());
    }
    let block = &document.blocks[0];
    if block.name != POLICY_BLOCK || block.labels.len() != 1 {
        return Err("MCP route policy block shape is invalid".into());
    }
    if block.attributes.len() != POLICY_ATTRIBUTES.len()
        || block
            .attributes
            .keys()
            .any(|key| !POLICY_ATTRIBUTES.contains(&key.as_str()))
    {
        return Err("MCP route policy contains missing or unknown fields".into());
    }
    if block.blocks.is_empty()
        || block.blocks.len() > 10_000
        || block.blocks.iter().any(|nested| nested.name != "grants")
    {
        return Err("MCP route policy grant blocks are invalid".into());
    }
    let route_id = RouteId::from_uuid(parse_uuid(&block.labels[0], "route label")?);
    let spec = McpRoutePolicySpec {
        route_id,
        organization_id: OrganizationId::from_uuid(required_uuid(block, "organization_id")?),
        project_id: ProjectId::from_uuid(required_uuid(block, "project_id")?),
        environment_id: EnvironmentId::from_uuid(required_uuid(block, "environment_id")?),
        gateway_scope_id: GatewayScopeId::from_uuid(required_uuid(block, "gateway_scope_id")?),
        domain_claim_id: DomainClaimId::from_uuid(required_uuid(block, "domain_claim_id")?),
        workload_id: WorkloadId::from_uuid(required_uuid(block, "workload_id")?),
        asset_id: AssetId::from_uuid(required_uuid(block, "asset_id")?),
        asset_release_id: AssetReleaseId::from_uuid(required_uuid(block, "asset_release_id")?),
        profile_digest: Sha256Digest::parse(required_string(block, "profile_digest")?)?,
        hostname: RouteHostname::parse(required_string(block, "hostname")?)?,
        path: required_string(block, "path")?,
        tls_required: required_bool(block, "tls_required")?,
        allowed_origins: required_strings(block, "allowed_origins")?,
        max_header_bytes: required_u64(block, "max_header_bytes")?,
        max_request_bytes: required_u64(block, "max_request_bytes")?,
        max_response_bytes: required_u64(block, "max_response_bytes")?,
        first_response_timeout_seconds: required_u64(block, "first_response_timeout_seconds")?,
        stream_idle_timeout_seconds: required_u64(block, "stream_idle_timeout_seconds")?,
        stream_total_timeout_seconds: required_u64(block, "stream_total_timeout_seconds")?,
        drain_timeout_seconds: required_u64(block, "drain_timeout_seconds")?,
        telemetry_names: required_strings(block, "telemetry_names")?,
        telemetry_events_per_minute: required_u64(block, "telemetry_events_per_minute")?,
        audit_required: required_bool(block, "audit_required")?,
        expires_at: required_timestamp(block, "expires_at")?,
        grants: block
            .blocks
            .iter()
            .map(parse_grant)
            .collect::<Result<Vec<_>, _>>()?,
    };
    let policy_revision = required_u64(block, "policy_revision")?;
    Ok((spec, policy_revision))
}

fn parse_grant(block: &Block) -> Result<McpGrantProjection, String> {
    const ATTRIBUTES: [&str; 3] = ["credential_generation", "methods", "names"];
    if block.name != "grants"
        || block.labels.len() != 1
        || block.attributes.len() != ATTRIBUTES.len()
        || block
            .attributes
            .keys()
            .any(|key| !ATTRIBUTES.contains(&key.as_str()))
        || block.blocks.len() != 1
    {
        return Err("MCP route grant block shape is invalid".into());
    }
    let limits = &block.blocks[0];
    const LIMIT_ATTRIBUTES: [&str; 3] = [
        "max_concurrent_requests",
        "request_burst",
        "requests_per_minute",
    ];
    if limits.name != "limits"
        || !limits.labels.is_empty()
        || !limits.blocks.is_empty()
        || limits.attributes.len() != LIMIT_ATTRIBUTES.len()
        || limits
            .attributes
            .keys()
            .any(|key| !LIMIT_ATTRIBUTES.contains(&key.as_str()))
    {
        return Err("MCP route grant limits block shape is invalid".into());
    }
    Ok(McpGrantProjection {
        credential_id: parse_uuid(&block.labels[0], "credential label")?,
        credential_generation: required_u64(block, "credential_generation")?,
        methods: required_strings(block, "methods")?,
        names: required_strings(block, "names")?,
        limits: McpLimitsProjection {
            max_concurrent_requests: required_u64(limits, "max_concurrent_requests")?,
            requests_per_minute: required_u64(limits, "requests_per_minute")?,
            request_burst: required_u64(limits, "request_burst")?,
        },
    })
}

fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("MCP route policy field {name:?} is required"))
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("MCP route policy field {name:?} must be a string"))
}

fn required_uuid(block: &Block, name: &str) -> Result<Uuid, String> {
    parse_uuid(&required_string(block, name)?, name)
}

fn parse_uuid(value: &str, label: &str) -> Result<Uuid, String> {
    Uuid::parse_str(value).map_err(|_| format!("MCP route policy {label} must be a UUID"))
}

fn required_bool(block: &Block, name: &str) -> Result<bool, String> {
    required_value(block, name)?
        .as_bool()
        .ok_or_else(|| format!("MCP route policy field {name:?} must be a boolean"))
}

fn required_u64(block: &Block, name: &str) -> Result<u64, String> {
    let value = required_value(block, name)?
        .as_number()
        .ok_or_else(|| format!("MCP route policy field {name:?} must be an integer"))?;
    if !value.is_finite()
        || value.fract() != 0.0
        || value <= 0.0
        || value > MAX_SAFE_ACL_INTEGER as f64
    {
        return Err(format!(
            "MCP route policy field {name:?} must be a positive exactly representable integer"
        ));
    }
    Ok(value as u64)
}

fn required_strings(block: &Block, name: &str) -> Result<Vec<String>, String> {
    let Value::List(values) = required_value(block, name)? else {
        return Err(format!(
            "MCP route policy field {name:?} must be a string list"
        ));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("MCP route policy field {name:?} must be a string list"))
        })
        .collect()
}

fn required_timestamp(block: &Block, name: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(&required_string(block, name)?)
        .map(|value| canonical_timestamp(value.with_timezone(&Utc)))
        .map_err(|_| format!("MCP route policy field {name:?} must be an RFC 3339 timestamp"))
}

fn string_list(values: &[String]) -> Value {
    list(values.iter().map(|value| string(value)).collect())
}

fn acl_integer(label: &str, value: u64) -> Result<Value, String> {
    if value == 0 || value > MAX_SAFE_ACL_INTEGER {
        return Err(format!(
            "MCP route policy {label} is not representable by ACL"
        ));
    }
    Ok(integer(value as i64))
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
        return Err("MCP route path must be one safe literal path".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::assets::domain::{McpServiceProfile, McpServiceProfileSpec};
    use a3s_cloud_contracts::MCP_PROTOCOL_VERSION;
    use chrono::TimeZone;

    fn profile() -> McpServiceProfile {
        McpServiceProfile::from_spec(McpServiceProfileSpec {
            protocol_versions: vec![MCP_PROTOCOL_VERSION.into()],
            endpoint_path: "/mcp".into(),
            runtime_port: "mcp".into(),
            health_path: "/health".into(),
            request_sse: true,
            subscriptions: true,
            server_discover: true,
            expected_capabilities: vec!["subscriptions".into(), "tools".into()],
            max_request_bytes: 1_048_576,
            max_response_bytes: 8_388_608,
            max_stream_seconds: 3_600,
        })
        .expect("profile")
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 30, 12, 0, 0)
            .single()
            .expect("time")
    }

    fn spec(profile: &McpServiceProfile) -> McpRoutePolicySpec {
        McpRoutePolicySpec {
            route_id: RouteId::from_uuid(
                Uuid::parse_str("11111111-1111-4111-8111-111111111111").expect("UUID"),
            ),
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            gateway_scope_id: GatewayScopeId::new(),
            domain_claim_id: DomainClaimId::new(),
            workload_id: WorkloadId::new(),
            asset_id: AssetId::new(),
            asset_release_id: AssetReleaseId::new(),
            profile_digest: profile.digest().clone(),
            hostname: RouteHostname::parse("mcp.example.com").expect("hostname"),
            path: "/mcp".into(),
            tls_required: true,
            allowed_origins: vec!["https://console.example.com".into()],
            max_header_bytes: 32_768,
            max_request_bytes: 524_288,
            max_response_bytes: 4_194_304,
            first_response_timeout_seconds: 30,
            stream_idle_timeout_seconds: 120,
            stream_total_timeout_seconds: 1_800,
            drain_timeout_seconds: 30,
            telemetry_names: vec!["weather".into()],
            telemetry_events_per_minute: 10_000,
            audit_required: true,
            expires_at: now() + Duration::hours(1),
            grants: vec![McpGrantProjection {
                credential_id: Uuid::parse_str("22222222-2222-4222-8222-222222222222")
                    .expect("UUID"),
                credential_generation: 3,
                methods: vec![
                    "tools/call".into(),
                    "server/discover".into(),
                    "tools/list".into(),
                ],
                names: vec!["weather".into()],
                limits: McpLimitsProjection {
                    max_concurrent_requests: 8,
                    requests_per_minute: 120,
                    request_burst: 16,
                },
            }],
        }
    }

    #[test]
    fn canonical_policy_round_trips_and_projects_without_credentials_or_targets() {
        let profile = profile();
        let policy = McpRoutePolicy::create(spec(&profile), &profile, now()).expect("policy");
        let parsed = McpRoutePolicy::parse_acl(&format!("\n{}\n", policy.canonical_acl()))
            .expect("parsed document");
        assert_eq!(parsed.spec(), policy.spec());
        assert_eq!(parsed.policy_revision(), policy.policy_revision());
        assert_eq!(parsed.canonical_acl(), policy.canonical_acl());
        assert_eq!(parsed.policy_digest(), policy.policy_digest());
        assert_eq!(
            parsed
                .materialize(policy.created_at(), policy.updated_at(), &profile)
                .expect("materialized document"),
            policy
        );
        let restored = McpRoutePolicy::restore(
            policy.canonical_acl(),
            policy.policy_digest().as_str(),
            policy.created_at(),
            policy.updated_at(),
            &profile,
        )
        .expect("restored");
        assert_eq!(restored, policy);
        let projection = policy.gateway_projection("mcp", Vec::new());
        assert_eq!(projection.profile_digest, profile.digest().as_str());
        assert!(projection.targets.is_empty());
        assert_eq!(projection.grants.len(), 1);
    }

    #[test]
    fn historical_document_parses_before_current_time_admission() {
        let profile = profile();
        let policy = McpRoutePolicy::create(spec(&profile), &profile, now()).expect("policy");
        let parsed = McpRoutePolicy::parse_acl(policy.canonical_acl()).expect("parsed document");

        assert!(parsed
            .materialize(
                policy.created_at(),
                policy.spec().expires_at + Duration::seconds(1),
                &profile,
            )
            .is_err());
        assert_eq!(parsed.policy_digest(), policy.policy_digest());
    }

    #[test]
    fn revision_changes_digest_but_cannot_change_route_ownership() {
        let profile = profile();
        let mut policy = McpRoutePolicy::create(spec(&profile), &profile, now()).expect("policy");
        let first_digest = policy.policy_digest().clone();
        let mut updated = policy.spec().clone();
        updated.max_request_bytes /= 2;
        updated.expires_at += Duration::minutes(1);
        assert!(policy
            .revise(updated, &profile, now() + Duration::minutes(1))
            .expect("revision"));
        assert_eq!(policy.policy_revision(), 2);
        assert_ne!(policy.policy_digest(), &first_digest);

        let mut moved = policy.spec().clone();
        moved.organization_id = OrganizationId::new();
        assert!(policy
            .revise(moved, &profile, now() + Duration::minutes(2))
            .is_err());
    }

    #[test]
    fn semantically_reordered_policy_does_not_advance_revision() {
        let profile = profile();
        let mut policy = McpRoutePolicy::create(spec(&profile), &profile, now()).expect("policy");
        let digest = policy.policy_digest().clone();
        let updated_at = policy.updated_at();
        let mut reordered = policy.spec().clone();
        reordered.grants[0].methods.reverse();

        assert!(!policy
            .revise(reordered, &profile, now() + Duration::minutes(1))
            .expect("no-op revision"));
        assert_eq!(policy.policy_revision(), 1);
        assert_eq!(policy.policy_digest(), &digest);
        assert_eq!(policy.updated_at(), updated_at);
    }

    #[test]
    fn profile_bounds_exact_path_tls_origins_and_discovery_fail_closed() {
        let profile = profile();
        let mut candidate = spec(&profile);
        candidate.domain_claim_id = DomainClaimId::from_uuid(Uuid::nil());
        assert!(McpRoutePolicy::create(candidate, &profile, now()).is_err());

        let mut candidate = spec(&profile);
        candidate.max_request_bytes = profile.spec().max_request_bytes + 1;
        assert!(McpRoutePolicy::create(candidate, &profile, now()).is_err());

        let mut candidate = spec(&profile);
        candidate.path = "/other".into();
        assert!(McpRoutePolicy::create(candidate, &profile, now()).is_err());

        let mut candidate = spec(&profile);
        candidate.tls_required = false;
        assert!(McpRoutePolicy::create(candidate, &profile, now()).is_err());

        let mut candidate = spec(&profile);
        candidate.allowed_origins = vec!["http://example.com".into()];
        assert!(McpRoutePolicy::create(candidate, &profile, now()).is_err());

        let mut candidate = spec(&profile);
        candidate.grants[0].methods = vec!["tools/list".into()];
        assert!(McpRoutePolicy::create(candidate, &profile, now()).is_err());
    }

    #[test]
    fn acl_parser_rejects_unknown_session_fields_and_digest_mismatch() {
        let profile = profile();
        let policy = McpRoutePolicy::create(spec(&profile), &profile, now()).expect("policy");
        let with_session = policy.canonical_acl().replace(
            "tls_required = true",
            "tls_required = true\n  sticky_session = true",
        );
        assert!(McpRoutePolicy::restore(
            &with_session,
            policy.policy_digest().as_str(),
            policy.created_at(),
            policy.updated_at(),
            &profile,
        )
        .is_err());
        assert!(McpRoutePolicy::restore(
            policy.canonical_acl(),
            &format!("sha256:{}", "f".repeat(64)),
            policy.created_at(),
            policy.updated_at(),
            &profile,
        )
        .is_err());
    }
}
