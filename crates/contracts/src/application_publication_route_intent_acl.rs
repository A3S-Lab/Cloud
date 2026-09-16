//! Application publication route intent ACL projection for Cloud → Gateway.
//!
//! Declare-only Published Language: exact-release channel/origin facts plus an
//! opaque rate-shaping profile id and policy revision digest. Gateway snapshot
//! compile admits the block; rate enforcement stays later (distributed-api §8).
//! Edge must not invent Applications authority from Route aggregates.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

const MAX_CHANNELS: usize = 16;
const MAX_ORIGINS: usize = 32;
const MAX_CHANNEL_LEN: usize = 64;
const MAX_ORIGIN_LEN: usize = 512;
const MAX_PROFILE_ID_LEN: usize = 128;

/// One Applications-owned publication route intent projected into Gateway ACL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationPublicationRouteIntentAclProjection {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub application_id: Uuid,
    pub application_release_id: Uuid,
    pub application_release_digest: String,
    pub publication_route_intent_id: Uuid,
    pub channels: Vec<String>,
    pub embed_origin_allowlist: Vec<String>,
    pub rate_shaping_profile_id: String,
    pub rate_shaping_policy_revision_digest: String,
}

impl ApplicationPublicationRouteIntentAclProjection {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.is_nil()
            || self.project_id.is_nil()
            || self.application_id.is_nil()
            || self.application_release_id.is_nil()
            || self.publication_route_intent_id.is_nil()
        {
            return Err(
                "application publication route intent ACL projection identity is invalid".into(),
            );
        }
        if !valid_sha256(&self.application_release_digest) {
            return Err(
                "application publication route intent ACL release digest is invalid".into(),
            );
        }
        if !valid_sha256(&self.rate_shaping_policy_revision_digest) {
            return Err(
                "application publication route intent ACL rate policy digest is invalid".into(),
            );
        }
        if self.channels.is_empty() || self.channels.len() > MAX_CHANNELS {
            return Err(format!(
                "application publication route intent ACL channels must contain 1 to {MAX_CHANNELS} entries"
            ));
        }
        let mut seen_channels = HashSet::new();
        for channel in &self.channels {
            validate_token(channel, "channel", MAX_CHANNEL_LEN)?;
            if !seen_channels.insert(channel.as_str()) {
                return Err(
                    "application publication route intent ACL channels must be unique".into(),
                );
            }
        }
        if self.embed_origin_allowlist.len() > MAX_ORIGINS {
            return Err(format!(
                "application publication route intent ACL embed origins exceed {MAX_ORIGINS}"
            ));
        }
        let mut seen_origins = HashSet::new();
        for origin in &self.embed_origin_allowlist {
            validate_origin(origin)?;
            if !seen_origins.insert(origin.as_str()) {
                return Err(
                    "application publication route intent ACL embed origins must be unique".into(),
                );
            }
        }
        if !self.embed_origin_allowlist.is_empty()
            && !self.channels.iter().any(|channel| channel == "embed")
        {
            return Err(
                "application publication route intent ACL embed origins require the embed channel"
                    .into(),
            );
        }
        validate_token(
            &self.rate_shaping_profile_id,
            "rate_shaping_profile_id",
            MAX_PROFILE_ID_LEN,
        )?;
        Ok(())
    }
}

/// Render declare-only publication route intent ACL blocks for one Gateway snapshot.
///
/// Empty input renders nothing. Non-empty intents are sorted by
/// `publication_route_intent_id` for digest stability.
pub fn render_application_publication_route_intent_acl_blocks(
    intents: &[ApplicationPublicationRouteIntentAclProjection],
) -> Result<String, String> {
    if intents.is_empty() {
        return Ok(String::new());
    }
    let mut ordered = intents.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|intent| intent.publication_route_intent_id);
    let mut seen = HashSet::new();
    let mut out = String::from("application_publication_route_intents {\n");
    for intent in ordered {
        intent.validate()?;
        if !seen.insert(intent.publication_route_intent_id) {
            return Err(
                "application publication route intent IDs must be unique within one snapshot"
                    .into(),
            );
        }
        out.push_str(&render_one_intent(intent)?);
    }
    out.push_str("}\n");
    Ok(out)
}

fn render_one_intent(
    intent: &ApplicationPublicationRouteIntentAclProjection,
) -> Result<String, String> {
    let channels = intent
        .channels
        .iter()
        .map(|channel| format!("\"{}\"", escape_acl_string(channel)))
        .collect::<Vec<_>>()
        .join(", ");
    let origins = intent
        .embed_origin_allowlist
        .iter()
        .map(|origin| format!("\"{}\"", escape_acl_string(origin)))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "  intents \"{}\" {{\n    organization_id = \"{}\"\n    project_id = \"{}\"\n    application_id = \"{}\"\n    application_release_id = \"{}\"\n    application_release_digest = \"{}\"\n    channels = [{channels}]\n    embed_origin_allowlist = [{origins}]\n    rate_shaping {{\n      profile_id = \"{}\"\n      policy_revision_digest = \"{}\"\n    }}\n  }}\n",
        intent.publication_route_intent_id,
        intent.organization_id,
        intent.project_id,
        intent.application_id,
        intent.application_release_id,
        escape_acl_string(&intent.application_release_digest),
        escape_acl_string(&intent.rate_shaping_profile_id),
        escape_acl_string(&intent.rate_shaping_policy_revision_digest),
    ))
}

fn validate_token(value: &str, field: &str, max_len: usize) -> Result<(), String> {
    if value.is_empty() || value.len() > max_len || value.trim() != value {
        return Err(format!(
            "application publication route intent ACL {field} must be a non-empty canonical token up to {max_len} characters"
        ));
    }
    if value.contains('\n') || value.contains('\r') || value.contains('\0') {
        return Err(format!(
            "application publication route intent ACL {field} must not contain control characters"
        ));
    }
    Ok(())
}

fn validate_origin(origin: &str) -> Result<(), String> {
    validate_token(origin, "embed_origin", MAX_ORIGIN_LEN)?;
    if !(origin.starts_with("http://") || origin.starts_with("https://")) {
        return Err(
            "application publication route intent ACL embed origin must be http(s)".into(),
        );
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    let Some(hexadecimal) = value.strip_prefix("sha256:") else {
        return false;
    };
    hexadecimal.len() == 64
        && hexadecimal
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
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

    fn digest(byte: u8) -> String {
        format!("sha256:{}", format!("{:02x}", byte).repeat(32))
    }

    fn intent() -> ApplicationPublicationRouteIntentAclProjection {
        ApplicationPublicationRouteIntentAclProjection {
            organization_id: Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap(),
            project_id: Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap(),
            application_id: Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
            application_release_id: Uuid::parse_str("44444444-4444-4444-8444-444444444444")
                .unwrap(),
            application_release_digest: digest(0xab),
            publication_route_intent_id: Uuid::parse_str("55555555-5555-4555-8555-555555555555")
                .unwrap(),
            channels: vec!["api_blocking".into(), "embed".into()],
            embed_origin_allowlist: vec!["https://app.example.com".into()],
            rate_shaping_profile_id: "default-burst".into(),
            rate_shaping_policy_revision_digest: digest(0xcd),
        }
    }

    #[test]
    fn empty_intents_render_nothing() {
        assert_eq!(
            render_application_publication_route_intent_acl_blocks(&[]).unwrap(),
            ""
        );
    }

    #[test]
    fn renders_declare_only_rate_profile_and_channels() {
        let rendered = render_application_publication_route_intent_acl_blocks(&[intent()]).unwrap();
        assert!(rendered.starts_with("application_publication_route_intents {"));
        assert!(rendered.contains("intents \"55555555-5555-4555-8555-555555555555\""));
        assert!(rendered.contains("channels = [\"api_blocking\", \"embed\"]"));
        assert!(rendered.contains("embed_origin_allowlist = [\"https://app.example.com\"]"));
        assert!(rendered.contains("profile_id = \"default-burst\""));
        assert!(rendered.contains(&format!(
            "policy_revision_digest = \"{}\"",
            digest(0xcd)
        )));
        assert!(!rendered.contains("requests_per_minute"));
        assert!(!rendered.contains("token_bucket"));
    }

    #[test]
    fn sorts_by_intent_id_for_digest_stability() {
        let mut later = intent();
        later.publication_route_intent_id =
            Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap();
        let mut earlier = intent();
        earlier.publication_route_intent_id =
            Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
        earlier.channels = vec!["web".into()];
        earlier.embed_origin_allowlist.clear();
        let forward = render_application_publication_route_intent_acl_blocks(&[
            later.clone(),
            earlier.clone(),
        ])
        .unwrap();
        let reverse =
            render_application_publication_route_intent_acl_blocks(&[earlier, later]).unwrap();
        assert_eq!(forward, reverse);
        let first = forward.find("00000000-0000-4000-8000-000000000001").unwrap();
        let second = forward.find("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap();
        assert!(first < second);
    }

    #[test]
    fn rejects_duplicate_intent_ids_and_invalid_digests() {
        let left = intent();
        let right = intent();
        assert!(render_application_publication_route_intent_acl_blocks(&[left, right]).is_err());
        let mut bad = intent();
        bad.rate_shaping_policy_revision_digest = "not-a-digest".into();
        assert!(render_application_publication_route_intent_acl_blocks(&[bad]).is_err());
    }

    #[test]
    fn rejects_origins_without_embed_channel() {
        let mut bad = intent();
        bad.channels = vec!["api_blocking".into()];
        assert!(bad.validate().is_err());
    }
}
