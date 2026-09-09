//! Inference credential ACL projection for Cloud → Gateway policy.
//!
//! Gateway already enforces this shape on managed `inference` policy. Cloud
//! must emit the same bytes before catalog/route compilers land. Plaintext
//! secrets never belong in this projection.

use super::tokenizer_revision::{
    inference_tokenizer_revision_acl_attr, require_inference_tokenizer_revision,
    INFERENCE_TOKENIZER_REVISION_V1,
};
use argon2::password_hash::PasswordHash;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use uuid::Uuid;

/// Audience accepted by the native inference data plane.
pub const INFERENCE_CREDENTIAL_AUDIENCE: &str = "cloud-inference";

const MAX_VERIFIER_BYTES: usize = 512;
const MAX_SAFE_ACL_INTEGER: u64 = (1_u64 << 53) - 1;
const MIN_ARGON2_MEMORY_KIB: u32 = 19_456;
const MAX_ARGON2_MEMORY_KIB: u32 = 262_144;
const MIN_ARGON2_ITERATIONS: u32 = 2;
const MAX_ARGON2_ITERATIONS: u32 = 10;
const MAX_ARGON2_LANES: u32 = 4;
const MIN_ARGON2_SALT_ENCODED_LEN: usize = 22;
const MAX_ARGON2_SALT_ENCODED_LEN: usize = 86;
const MIN_ARGON2_OUTPUT_ENCODED_LEN: usize = 43;
const MAX_ARGON2_OUTPUT_ENCODED_LEN: usize = 86;

/// One inference-key verifier projection for managed Gateway ACL.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceCredentialAclProjection {
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

impl InferenceCredentialAclProjection {
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
    ) -> Result<Self, String> {
        let projection = Self {
            credential_id,
            environment_id,
            audience: audience.into(),
            prefix: prefix.into(),
            verifier_hash: verifier_hash.into(),
            generation,
            expires_at,
            revoked,
        };
        projection.validate()?;
        Ok(projection)
    }

    pub fn verifier_hash(&self) -> &str {
        &self.verifier_hash
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.credential_id.is_nil() || self.environment_id.is_nil() {
            return Err("inference credential and environment IDs must not be nil".into());
        }
        if self.audience != INFERENCE_CREDENTIAL_AUDIENCE {
            return Err(format!(
                "inference credential audience must be {INFERENCE_CREDENTIAL_AUDIENCE}"
            ));
        }
        validate_credential_prefix(&self.prefix)?;
        if self.generation == 0 || self.generation > MAX_SAFE_ACL_INTEGER {
            return Err("inference credential generation must be a positive ACL-safe integer".into());
        }
        if self.expires_at.timestamp_millis() <= 0 {
            return Err("inference credential expires_at must be a positive UTC timestamp".into());
        }
        validate_argon2id_verifier(&self.verifier_hash)
    }
}

impl fmt::Debug for InferenceCredentialAclProjection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InferenceCredentialAclProjection")
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

/// Render a managed `inference` ACL block with optional credential projections.
///
/// Credentials are sorted by `credential_id` for deterministic snapshot digests.
/// Routes and workers remain omitted until those compilers land.
pub fn render_inference_policy_acl(
    expires_at: DateTime<Utc>,
    credentials: &[InferenceCredentialAclProjection],
) -> Result<String, String> {
    if expires_at.timestamp_millis() <= 0 {
        return Err("inference policy expires_at must be a positive UTC timestamp".into());
    }
    let mut seen_ids = HashSet::new();
    let mut seen_prefixes = HashSet::new();
    let mut ordered = credentials.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|credential| credential.credential_id);
    for credential in &ordered {
        credential.validate()?;
        if !seen_ids.insert(credential.credential_id) {
            return Err("inference credential IDs must be unique within one policy".into());
        }
        if !seen_prefixes.insert(credential.prefix.as_str()) {
            return Err(format!(
                "inference credential prefix '{}' is not unique",
                credential.prefix
            ));
        }
    }
    let mut ordered_prefixes = seen_prefixes.into_iter().collect::<Vec<_>>();
    ordered_prefixes.sort_unstable();
    if ordered_prefixes
        .windows(2)
        .any(|window| window[1].starts_with(window[0]))
    {
        return Err("inference credential prefixes must not overlap".into());
    }

    let expires = expires_at.to_rfc3339_opts(SecondsFormat::Micros, true);
    let mut acl = format!(
        "inference {{\n  {}\n  expires_at = \"{expires}\"\n",
        inference_tokenizer_revision_acl_attr()
    );
    for credential in ordered {
        let credential_expires = credential
            .expires_at
            .to_rfc3339_opts(SecondsFormat::Micros, true);
        acl.push_str(&format!(
            "\n  credentials \"{}\" {{\n    environment_id = \"{}\"\n    audience = \"{}\"\n    prefix = \"{}\"\n    verifier_hash = \"{}\"\n    generation = {}\n    expires_at = \"{credential_expires}\"\n    revoked = {}\n  }}\n",
            credential.credential_id,
            credential.environment_id,
            credential.audience,
            escape_acl_string(&credential.prefix),
            escape_acl_string(credential.verifier_hash()),
            credential.generation,
            if credential.revoked { "true" } else { "false" },
        ));
    }
    acl.push_str("}\n");
    require_inference_tokenizer_revision(&acl)?;
    debug_assert_eq!(INFERENCE_TOKENIZER_REVISION_V1, "a3s.gateway.tokenizer.v1");
    Ok(acl)
}

fn escape_acl_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn validate_credential_prefix(prefix: &str) -> Result<(), String> {
    let Some(suffix) = prefix.strip_prefix("a3s_inf_") else {
        return Err("inference credential prefix must start with 'a3s_inf_'".into());
    };
    if suffix.len() < 8
        || suffix.len() > 32
        || !suffix
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return Err(
            "inference credential prefix suffix must contain 8 to 32 lowercase ASCII letters or digits"
                .into(),
        );
    }
    Ok(())
}

fn validate_argon2id_verifier(verifier_hash: &str) -> Result<(), String> {
    if verifier_hash.is_empty() || verifier_hash.len() > MAX_VERIFIER_BYTES {
        return Err(format!(
            "inference credential verifier_hash must contain 1 to {MAX_VERIFIER_BYTES} bytes"
        ));
    }
    PasswordHash::new(verifier_hash)
        .map_err(|_| "inference credential verifier_hash is not a valid PHC string".to_string())?;
    let parts = verifier_hash.split('$').collect::<Vec<_>>();
    if parts.len() != 6 || parts[1] != "argon2id" || parts[2] != "v=19" {
        return Err("inference credential verifier_hash must use Argon2id PHC version 19".into());
    }
    let mut memory = None;
    let mut iterations = None;
    let mut lanes = None;
    for parameter in parts[3].split(',') {
        let (name, value) = parameter.split_once('=').ok_or_else(|| {
            "inference credential verifier_hash has invalid Argon2id parameters".to_string()
        })?;
        let value = value.parse::<u32>().map_err(|_| {
            "inference credential verifier_hash has invalid Argon2id parameters".to_string()
        })?;
        match name {
            "m" if memory.replace(value).is_none() => {}
            "t" if iterations.replace(value).is_none() => {}
            "p" if lanes.replace(value).is_none() => {}
            _ => {
                return Err(
                    "inference credential verifier_hash must contain exactly m, t, and p parameters"
                        .into(),
                );
            }
        }
    }
    let (Some(memory), Some(iterations), Some(lanes)) = (memory, iterations, lanes) else {
        return Err(
            "inference credential verifier_hash must contain m, t, and p parameters".into(),
        );
    };
    if !(MIN_ARGON2_MEMORY_KIB..=MAX_ARGON2_MEMORY_KIB).contains(&memory)
        || !(MIN_ARGON2_ITERATIONS..=MAX_ARGON2_ITERATIONS).contains(&iterations)
        || !(1..=MAX_ARGON2_LANES).contains(&lanes)
        || !(MIN_ARGON2_SALT_ENCODED_LEN..=MAX_ARGON2_SALT_ENCODED_LEN).contains(&parts[4].len())
        || !(MIN_ARGON2_OUTPUT_ENCODED_LEN..=MAX_ARGON2_OUTPUT_ENCODED_LEN)
            .contains(&parts[5].len())
    {
        return Err(
            "inference credential verifier_hash uses unsupported Argon2id cost, salt, or output bounds"
                .into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    fn credential(prefix: &str) -> InferenceCredentialAclProjection {
        InferenceCredentialAclProjection::new(
            Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
            Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap(),
            INFERENCE_CREDENTIAL_AUDIENCE,
            prefix,
            VERIFIER,
            7,
            Utc.with_ymd_and_hms(2098, 12, 31, 23, 0, 0).unwrap(),
            false,
        )
        .unwrap()
    }

    #[test]
    fn renders_credentials_inside_frozen_inference_shell() {
        let expires_at = Utc.with_ymd_and_hms(2099, 1, 1, 0, 0, 0).unwrap();
        let acl = render_inference_policy_acl(expires_at, &[credential("a3s_inf_abc12345")]).unwrap();
        require_inference_tokenizer_revision(&acl).unwrap();
        assert!(acl.contains("credentials \"33333333-3333-4333-8333-333333333333\""));
        assert!(acl.contains("audience = \"cloud-inference\""));
        assert!(acl.contains("prefix = \"a3s_inf_abc12345\""));
        assert!(acl.contains(&format!("verifier_hash = \"{VERIFIER}\"")));
        assert!(!acl.contains("routes "));
        assert!(!acl.contains("workers "));
    }

    #[test]
    fn rejects_wrong_audience_prefix_and_overlapping_prefixes() {
        assert!(InferenceCredentialAclProjection::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "cloud-mcp",
            "a3s_inf_abc12345",
            VERIFIER,
            1,
            Utc.with_ymd_and_hms(2099, 1, 1, 0, 0, 0).unwrap(),
            false,
        )
        .unwrap_err()
        .contains("audience"));

        assert!(InferenceCredentialAclProjection::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            INFERENCE_CREDENTIAL_AUDIENCE,
            "a3s_mcp_abc12345",
            VERIFIER,
            1,
            Utc.with_ymd_and_hms(2099, 1, 1, 0, 0, 0).unwrap(),
            false,
        )
        .unwrap_err()
        .contains("a3s_inf_"));

        let expires_at = Utc.with_ymd_and_hms(2099, 1, 1, 0, 0, 0).unwrap();
        let left = credential("a3s_inf_abc12345");
        let mut right = credential("a3s_inf_abc12345x");
        right.credential_id = Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap();
        let error = render_inference_policy_acl(expires_at, &[left, right]).unwrap_err();
        assert!(error.contains("overlap"));
    }

    #[test]
    fn redacts_verifier_from_debug_and_serialized_views() {
        let credential = credential("a3s_inf_abc12345");
        let debug = format!("{credential:?}");
        let json = serde_json::to_string(&credential).unwrap();
        assert!(!debug.contains(VERIFIER));
        assert!(debug.contains("<redacted>"));
        assert!(!json.contains(VERIFIER));
        assert!(!json.contains("verifier_hash"));
    }
}
