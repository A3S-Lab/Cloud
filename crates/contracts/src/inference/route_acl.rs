//! Inference route / grant ACL projection for Cloud → Gateway policy.
//!
//! Gateway already parses this shape under managed `inference.routes`. Cloud
//! contracts own the byte form; Inference catalog/route authority must supply
//! the projections. Edge must not invent catalog facts.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use uuid::Uuid;

const MAX_SAFE_ACL_INTEGER: u64 = (1_u64 << 53) - 1;
const MAX_ALIAS_LEN: usize = 128;
const MAX_SERVICE_LEN: usize = 128;
const MAX_UPSTREAM_MODEL_LEN: usize = 256;
const MAX_ROUTER_LEN: usize = 128;

/// Closed native inference endpoint identifiers (Gateway ACL literals).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InferenceEndpointAcl {
    Models,
    ChatCompletions,
    Completions,
    Embeddings,
}

impl InferenceEndpointAcl {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Models => "models",
            Self::ChatCompletions => "chat-completions",
            Self::Completions => "completions",
            Self::Embeddings => "embeddings",
        }
    }
}

impl fmt::Display for InferenceEndpointAcl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Local admission limits projected into a grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceLimitsAclProjection {
    pub max_concurrent_requests: u64,
    pub requests_per_minute: u64,
    pub request_burst: u64,
    pub tokens_per_minute: u64,
}

impl InferenceLimitsAclProjection {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("max_concurrent_requests", self.max_concurrent_requests),
            ("requests_per_minute", self.requests_per_minute),
            ("request_burst", self.request_burst),
            ("tokens_per_minute", self.tokens_per_minute),
        ] {
            if value == 0 || value > MAX_SAFE_ACL_INTEGER {
                return Err(format!(
                    "inference grant limits.{name} must be in 1..={MAX_SAFE_ACL_INTEGER}"
                ));
            }
        }
        if self.request_burst > self.requests_per_minute {
            return Err(
                "inference grant limits.request_burst must not exceed requests_per_minute".into(),
            );
        }
        Ok(())
    }
}

/// One upstream target under a model alias.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceTargetAclProjection {
    pub target_id: Uuid,
    pub service: String,
    pub upstream_model: String,
    pub priority: u32,
    pub weight: u32,
}

impl InferenceTargetAclProjection {
    pub fn validate(&self) -> Result<(), String> {
        if self.target_id.is_nil() {
            return Err("inference target ID must not be nil".into());
        }
        validate_nonempty_ascii_token(&self.service, "service", MAX_SERVICE_LEN)?;
        validate_nonempty_token(
            &self.upstream_model,
            "upstream_model",
            MAX_UPSTREAM_MODEL_LEN,
        )?;
        if self.weight == 0 {
            return Err("inference target weight must be positive".into());
        }
        Ok(())
    }
}

/// One model alias with ordered targets (no scheduling in this brick).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceModelAclProjection {
    pub alias: String,
    pub model_id: Uuid,
    pub targets: Vec<InferenceTargetAclProjection>,
}

impl InferenceModelAclProjection {
    pub fn validate(&self) -> Result<(), String> {
        validate_model_alias(&self.alias)?;
        if self.model_id.is_nil() {
            return Err("inference model_id must not be nil".into());
        }
        if self.targets.is_empty() {
            return Err(format!(
                "inference model alias '{}' requires at least one target",
                self.alias
            ));
        }
        let mut seen = HashSet::new();
        for target in &self.targets {
            target.validate()?;
            if !seen.insert(target.target_id) {
                return Err(format!(
                    "inference model alias '{}' has duplicate target_id {}",
                    self.alias, target.target_id
                ));
            }
        }
        Ok(())
    }
}

/// Credential grant under one route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceGrantAclProjection {
    pub credential_id: Uuid,
    pub credential_generation: u64,
    pub models: Vec<String>,
    pub endpoints: Vec<InferenceEndpointAcl>,
    pub limits: InferenceLimitsAclProjection,
}

impl InferenceGrantAclProjection {
    pub fn validate(&self, route_aliases: &BTreeSet<&str>) -> Result<(), String> {
        if self.credential_id.is_nil() {
            return Err("inference grant credential_id must not be nil".into());
        }
        if self.credential_generation == 0 || self.credential_generation > MAX_SAFE_ACL_INTEGER {
            return Err(format!(
                "inference grant credential_generation must be in 1..={MAX_SAFE_ACL_INTEGER}"
            ));
        }
        if self.models.is_empty() {
            return Err("inference grant models must not be empty".into());
        }
        if self.endpoints.is_empty() {
            return Err("inference grant endpoints must not be empty".into());
        }
        let mut seen_models = HashSet::new();
        for alias in &self.models {
            validate_model_alias(alias)?;
            if !route_aliases.contains(alias.as_str()) {
                return Err(format!(
                    "inference grant references unknown model alias '{alias}'"
                ));
            }
            if !seen_models.insert(alias.as_str()) {
                return Err(format!(
                    "inference grant lists duplicate model alias '{alias}'"
                ));
            }
        }
        let mut seen_endpoints = HashSet::new();
        for endpoint in &self.endpoints {
            if !seen_endpoints.insert(*endpoint) {
                return Err(format!(
                    "inference grant lists duplicate endpoint '{}'",
                    endpoint.as_str()
                ));
            }
        }
        self.limits.validate()
    }
}

/// One InferenceRoute access-policy revision projected into Gateway ACL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceRouteAclProjection {
    pub route_id: Uuid,
    pub router: String,
    pub environment_id: Uuid,
    pub policy_revision: u64,
    pub models: Vec<InferenceModelAclProjection>,
    pub grants: Vec<InferenceGrantAclProjection>,
}

impl InferenceRouteAclProjection {
    pub fn validate(&self) -> Result<(), String> {
        if self.route_id.is_nil() || self.environment_id.is_nil() {
            return Err("inference route and environment IDs must not be nil".into());
        }
        validate_nonempty_ascii_token(&self.router, "router", MAX_ROUTER_LEN)?;
        if self.policy_revision == 0 || self.policy_revision > MAX_SAFE_ACL_INTEGER {
            return Err(format!(
                "inference route policy_revision must be in 1..={MAX_SAFE_ACL_INTEGER}"
            ));
        }
        if self.models.is_empty() {
            return Err("inference route requires at least one model".into());
        }
        let mut aliases = BTreeSet::new();
        for model in &self.models {
            model.validate()?;
            if !aliases.insert(model.alias.as_str()) {
                return Err(format!(
                    "inference route has duplicate model alias '{}'",
                    model.alias
                ));
            }
        }
        let mut seen_grants = HashSet::new();
        for grant in &self.grants {
            grant.validate(&aliases)?;
            if !seen_grants.insert(grant.credential_id) {
                return Err(format!(
                    "inference route has duplicate grant credential_id {}",
                    grant.credential_id
                ));
            }
        }
        Ok(())
    }
}

/// Render route blocks for insertion inside a managed `inference` policy.
///
/// Routes are sorted by `route_id`. Nested models/targets/grants/endpoints are
/// sorted for digest stability. Callers must still attach credentials via
/// [`super::render_inference_policy_acl_with_routes`].
pub fn render_inference_route_acl_blocks(
    routes: &[InferenceRouteAclProjection],
) -> Result<String, String> {
    let mut ordered = routes.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|route| route.route_id);
    let mut seen_ids = HashSet::new();
    let mut out = String::new();
    for route in ordered {
        route.validate()?;
        if !seen_ids.insert(route.route_id) {
            return Err("inference route IDs must be unique within one policy".into());
        }
        out.push_str(&render_one_route(route)?);
    }
    Ok(out)
}

fn render_one_route(route: &InferenceRouteAclProjection) -> Result<String, String> {
    let mut models: BTreeMap<&str, &InferenceModelAclProjection> = BTreeMap::new();
    for model in &route.models {
        models.insert(model.alias.as_str(), model);
    }
    let mut grants: BTreeMap<Uuid, &InferenceGrantAclProjection> = BTreeMap::new();
    for grant in &route.grants {
        grants.insert(grant.credential_id, grant);
    }

    let mut block = format!(
        "\n  routes \"{}\" {{\n    router = \"{}\"\n    environment_id = \"{}\"\n    policy_revision = {}\n",
        route.route_id,
        escape_acl_string(&route.router),
        route.environment_id,
        route.policy_revision,
    );
    for (alias, model) in models {
        block.push_str(&format!(
            "    models \"{}\" {{\n      model_id = \"{}\"\n",
            escape_acl_string(alias),
            model.model_id
        ));
        let mut targets = model.targets.clone();
        targets.sort_by_key(|target| target.target_id);
        for target in targets {
            block.push_str(&format!(
                "      targets \"{}\" {{\n        service = \"{}\"\n        upstream_model = \"{}\"\n        priority = {}\n        weight = {}\n      }}\n",
                target.target_id,
                escape_acl_string(&target.service),
                escape_acl_string(&target.upstream_model),
                target.priority,
                target.weight,
            ));
        }
        block.push_str("    }\n");
    }
    for (credential_id, grant) in grants {
        let mut models = grant.models.clone();
        models.sort();
        let models_list = models
            .iter()
            .map(|alias| format!("\"{}\"", escape_acl_string(alias)))
            .collect::<Vec<_>>()
            .join(", ");
        let mut endpoints = grant.endpoints.clone();
        endpoints.sort();
        let endpoints_list = endpoints
            .iter()
            .map(|endpoint| format!("\"{}\"", endpoint.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        block.push_str(&format!(
            "    grants \"{credential_id}\" {{\n      credential_generation = {}\n      models = [{models_list}]\n      endpoints = [{endpoints_list}]\n      limits {{\n        max_concurrent_requests = {}\n        requests_per_minute = {}\n        request_burst = {}\n        tokens_per_minute = {}\n      }}\n    }}\n",
            grant.credential_generation,
            grant.limits.max_concurrent_requests,
            grant.limits.requests_per_minute,
            grant.limits.request_burst,
            grant.limits.tokens_per_minute,
        ));
    }
    block.push_str("  }\n");
    Ok(block)
}

fn validate_model_alias(alias: &str) -> Result<(), String> {
    validate_nonempty_ascii_token(alias, "model alias", MAX_ALIAS_LEN)
}

fn validate_nonempty_ascii_token(value: &str, field: &str, max_len: usize) -> Result<(), String> {
    if value.is_empty() || value.len() > max_len {
        return Err(format!(
            "inference {field} must contain 1 to {max_len} characters"
        ));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' || byte == b'.')
    {
        return Err(format!(
            "inference {field} must use ASCII letters, digits, '-', '_', or '.'"
        ));
    }
    Ok(())
}

fn validate_nonempty_token(value: &str, field: &str, max_len: usize) -> Result<(), String> {
    if value.is_empty() || value.len() > max_len {
        return Err(format!(
            "inference {field} must contain 1 to {max_len} characters"
        ));
    }
    if value.contains('\n') || value.contains('\r') || value.contains('\0') {
        return Err(format!(
            "inference {field} must not contain control characters"
        ));
    }
    Ok(())
}

fn escape_acl_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> InferenceTargetAclProjection {
        InferenceTargetAclProjection {
            target_id: Uuid::parse_str("66666666-6666-4666-8666-666666666666").unwrap(),
            service: "model-service".into(),
            upstream_model: "internal/model-v1".into(),
            priority: 0,
            weight: 100,
        }
    }

    fn route() -> InferenceRouteAclProjection {
        InferenceRouteAclProjection {
            route_id: Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap(),
            router: "inference".into(),
            environment_id: Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap(),
            policy_revision: 11,
            models: vec![InferenceModelAclProjection {
                alias: "chat-model".into(),
                model_id: Uuid::parse_str("55555555-5555-4555-8555-555555555555").unwrap(),
                targets: vec![target()],
            }],
            grants: vec![InferenceGrantAclProjection {
                credential_id: Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
                credential_generation: 3,
                models: vec!["chat-model".into()],
                endpoints: vec![
                    InferenceEndpointAcl::Models,
                    InferenceEndpointAcl::ChatCompletions,
                ],
                limits: InferenceLimitsAclProjection {
                    max_concurrent_requests: 2,
                    requests_per_minute: 60,
                    request_burst: 2,
                    tokens_per_minute: 10_000,
                },
            }],
        }
    }

    #[test]
    fn renders_deterministic_route_grant_and_target_blocks() {
        let rendered = render_inference_route_acl_blocks(&[route()]).unwrap();
        assert!(rendered.contains("routes \"44444444-4444-4444-8444-444444444444\""));
        assert!(rendered.contains("router = \"inference\""));
        assert!(rendered.contains("models \"chat-model\""));
        assert!(rendered.contains("targets \"66666666-6666-4666-8666-666666666666\""));
        assert!(rendered.contains("grants \"33333333-3333-4333-8333-333333333333\""));
        assert!(rendered.contains("endpoints = [\"models\", \"chat-completions\"]"));
        assert!(!rendered.contains("scheduling"));
        assert!(!rendered.contains("workers "));
    }

    #[test]
    fn rejects_grant_for_unknown_alias_and_zero_weight_target() {
        let mut bad = route();
        bad.grants[0].models = vec!["missing".into()];
        assert!(render_inference_route_acl_blocks(&[bad])
            .unwrap_err()
            .contains("unknown model alias"));

        let mut zero = route();
        zero.models[0].targets[0].weight = 0;
        assert!(render_inference_route_acl_blocks(&[zero])
            .unwrap_err()
            .contains("weight"));
    }

    #[test]
    fn rejects_burst_above_rpm_and_duplicate_route_ids() {
        let mut burst = route();
        burst.grants[0].limits.request_burst = 120;
        assert!(render_inference_route_acl_blocks(&[burst])
            .unwrap_err()
            .contains("request_burst"));

        let left = route();
        let right = route();
        assert!(render_inference_route_acl_blocks(&[left, right])
            .unwrap_err()
            .contains("unique"));
    }
}
