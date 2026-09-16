//! Applications-owned exact-release publication route intent.
//!
//! `APP0.3-C10` freezes the component-only domain aggregate named by ROADMAP
//! joint APP0.3 (exact route intent and rate policy profile reference).
//! Persistence, CQRS, REST, Gateway apply, and Delivery rate middleware stay
//! later. Gateway enforces versioned rate shaping per distributed-api §8.

use super::ApplicationRelease;
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationPublicationRouteIntentId, ApplicationReleaseId, OrganizationId,
    ProjectId, Sha256Digest,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use url::Url;
use uuid::Uuid;

/// Stable UUIDv5 namespace material for publication route intent identity.
pub const APPLICATION_PUBLICATION_ROUTE_INTENT_IDENTITY: &str =
    "application-publication-route-intent:v1";

/// Maximum admitted embed origin allowlist entries on one route intent.
pub const APPLICATION_PUBLICATION_EMBED_ORIGIN_MAX_ENTRIES: usize = 32;

/// Maximum opaque rate-shaping profile id length.
pub const APPLICATION_PUBLICATION_RATE_PROFILE_ID_MAX_CHARS: usize = 128;

/// Closed publication channels aligned with parity-manifest `publication.*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationPublicationChannel {
    ApiBlocking,
    ApiStreaming,
    Embed,
    Mcp,
    Web,
    Internal,
}

impl ApplicationPublicationChannel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ApiBlocking => "api_blocking",
            Self::ApiStreaming => "api_streaming",
            Self::Embed => "embed",
            Self::Mcp => "mcp",
            Self::Web => "web",
            Self::Internal => "internal",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "api_blocking" => Ok(Self::ApiBlocking),
            "api_streaming" => Ok(Self::ApiStreaming),
            "embed" => Ok(Self::Embed),
            "mcp" => Ok(Self::Mcp),
            "web" => Ok(Self::Web),
            "internal" => Ok(Self::Internal),
            _ => Err(format!(
                "unsupported Application publication channel {value:?}"
            )),
        }
    }
}

/// Opaque Gateway rate-shaping policy profile reference (declare only).
///
/// Enforcement stays with Gateway over a versioned token-bucket/GCRA profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationPublicationRateShapingPolicyRef {
    pub profile_id: String,
    pub policy_revision_digest: Sha256Digest,
}

impl ApplicationPublicationRateShapingPolicyRef {
    pub fn create(
        profile_id: impl Into<String>,
        policy_revision_digest: Sha256Digest,
    ) -> Result<Self, String> {
        let value = Self {
            profile_id: profile_id.into(),
            policy_revision_digest,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        let profile_id = normalize_rate_profile_id(&self.profile_id)?;
        if profile_id != self.profile_id {
            return Err("Application publication rate profile id is not canonical".into());
        }
        if Sha256Digest::parse(self.policy_revision_digest.as_str())?
            != self.policy_revision_digest
        {
            return Err(
                "Application publication rate policy revision digest is not canonical".into(),
            );
        }
        Ok(())
    }
}

/// Exact-release publication route intent owned by Applications.
///
/// Identity is deterministic from the release binding, unique channel set,
/// canonical embed origin allowlist, and rate-shaping policy reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationPublicationRouteIntent {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_digest: Sha256Digest,
    pub id: ApplicationPublicationRouteIntentId,
    pub channels: BTreeSet<ApplicationPublicationChannel>,
    pub embed_origin_allowlist: Vec<String>,
    pub rate_shaping_policy: ApplicationPublicationRateShapingPolicyRef,
}

impl ApplicationPublicationRouteIntent {
    pub fn create(
        release: &ApplicationRelease,
        channels: Vec<ApplicationPublicationChannel>,
        embed_origin_allowlist: Vec<String>,
        rate_shaping_policy: ApplicationPublicationRateShapingPolicyRef,
    ) -> Result<Self, String> {
        release.validate()?;
        let channels = normalize_channels(channels)?;
        let embed_origin_allowlist = normalize_embed_origins(embed_origin_allowlist, &channels)?;
        rate_shaping_policy.validate()?;
        let application_release_digest = release.contract.digest().clone();
        let id = Self::deterministic_id(
            release.organization_id,
            release.project_id,
            release.application_id,
            release.id,
            &application_release_digest,
            &channels,
            &embed_origin_allowlist,
            &rate_shaping_policy,
        )?;
        let value = Self {
            organization_id: release.organization_id,
            project_id: release.project_id,
            application_id: release.application_id,
            application_release_id: release.id,
            application_release_digest,
            id,
            channels,
            embed_origin_allowlist,
            rate_shaping_policy,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn restore(self) -> Result<Self, String> {
        self.validate()?;
        Ok(self)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn deterministic_id(
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        application_release_id: ApplicationReleaseId,
        application_release_digest: &Sha256Digest,
        channels: &BTreeSet<ApplicationPublicationChannel>,
        embed_origin_allowlist: &[String],
        rate_shaping_policy: &ApplicationPublicationRateShapingPolicyRef,
    ) -> Result<ApplicationPublicationRouteIntentId, String> {
        if organization_id.as_uuid().is_nil()
            || project_id.as_uuid().is_nil()
            || application_id.as_uuid().is_nil()
            || application_release_id.as_uuid().is_nil()
        {
            return Err(
                "Application publication route intent release identity cannot be nil".into(),
            );
        }
        let namespace = Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            APPLICATION_PUBLICATION_ROUTE_INTENT_IDENTITY.as_bytes(),
        );
        let channel_material = channels
            .iter()
            .map(|channel| channel.as_str())
            .collect::<Vec<_>>()
            .join("\0");
        let origin_material = embed_origin_allowlist.join("\0");
        let material = format!(
            "{organization_id}\0{project_id}\0{application_id}\0{application_release_id}\0{}\0{channel_material}\0{origin_material}\0{}\0{}",
            application_release_digest.as_str(),
            rate_shaping_policy.profile_id,
            rate_shaping_policy.policy_revision_digest.as_str(),
        );
        Ok(ApplicationPublicationRouteIntentId::from_uuid(Uuid::new_v5(
            &namespace,
            material.as_bytes(),
        )))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.application_release_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
        {
            return Err("stored Application publication route intent is invalid".into());
        }
        if Sha256Digest::parse(self.application_release_digest.as_str())?
            != self.application_release_digest
        {
            return Err(
                "Application publication route intent release digest is not canonical".into(),
            );
        }
        let channels = normalize_channels(self.channels.iter().copied().collect())?;
        if channels != self.channels {
            return Err("Application publication route intent channels are not canonical".into());
        }
        let origins = normalize_embed_origins(self.embed_origin_allowlist.clone(), &channels)?;
        if origins != self.embed_origin_allowlist {
            return Err(
                "Application publication route intent embed origins are not canonical".into(),
            );
        }
        self.rate_shaping_policy.validate()?;
        let expected = Self::deterministic_id(
            self.organization_id,
            self.project_id,
            self.application_id,
            self.application_release_id,
            &self.application_release_digest,
            &self.channels,
            &self.embed_origin_allowlist,
            &self.rate_shaping_policy,
        )?;
        if self.id != expected {
            return Err("Application publication route intent identity drifted".into());
        }
        Ok(())
    }
}

fn normalize_channels(
    channels: Vec<ApplicationPublicationChannel>,
) -> Result<BTreeSet<ApplicationPublicationChannel>, String> {
    if channels.is_empty() {
        return Err("Application publication route intent requires at least one channel".into());
    }
    let unique = channels.iter().copied().collect::<BTreeSet<_>>();
    if unique.len() != channels.len() {
        return Err("Application publication route intent contains duplicate channels".into());
    }
    Ok(unique)
}

fn normalize_embed_origins(
    origins: Vec<String>,
    channels: &BTreeSet<ApplicationPublicationChannel>,
) -> Result<Vec<String>, String> {
    if origins.is_empty() {
        return Ok(Vec::new());
    }
    if !channels.contains(&ApplicationPublicationChannel::Embed) {
        return Err("Application publication embed origins require the embed channel".into());
    }
    if origins.len() > APPLICATION_PUBLICATION_EMBED_ORIGIN_MAX_ENTRIES {
        return Err(
            "Application publication embed origin allowlist exceeds the maximum length".into(),
        );
    }
    let mut unique = BTreeSet::new();
    for origin in origins {
        let canonical = canonicalize_embed_origin(&origin)?;
        if !unique.insert(canonical) {
            return Err("Application publication embed origin allowlist contains duplicates".into());
        }
    }
    Ok(unique.into_iter().collect())
}

fn canonicalize_embed_origin(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Application publication embed origin cannot be empty".into());
    }
    let parsed =
        Url::parse(trimmed).map_err(|_| "Application publication embed origin is invalid".to_owned())?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("Application publication embed origin is invalid".into());
    }
    match parsed.origin() {
        url::Origin::Tuple(_, _, _) => Ok(parsed.origin().ascii_serialization()),
        url::Origin::Opaque(_) => Err("Application publication embed origin is invalid".into()),
    }
}

fn normalize_rate_profile_id(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("Application publication rate profile id cannot be blank".into());
    }
    if trimmed.chars().count() > APPLICATION_PUBLICATION_RATE_PROFILE_ID_MAX_CHARS {
        return Err("Application publication rate profile id exceeds the maximum length".into());
    }
    if trimmed != value {
        return Err("Application publication rate profile id is not canonical".into());
    }
    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::applications::domain::{
        ApplicationAudience, ApplicationDeliveryPolicy, ApplicationExperience,
        ApplicationInteractionMode, ApplicationReleaseContract, ApplicationReleaseContractSpec,
        ApplicationResponseMode, ApplicationWorkflowBinding,
    };
    use crate::modules::shared_kernel::domain::{
        PrincipalId, WorkflowDefinitionId, WorkflowRevisionId,
    };
    use chrono::{TimeZone, Utc};

    fn digest(marker: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
    }

    fn rate_policy() -> ApplicationPublicationRateShapingPolicyRef {
        ApplicationPublicationRateShapingPolicyRef::create("public-api-default", digest('a'))
            .expect("rate policy")
    }

    fn release() -> ApplicationRelease {
        let contract = ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
            experience: ApplicationExperience::Chatflow,
            audience: ApplicationAudience::ProjectMembers,
            delivery: ApplicationDeliveryPolicy {
                interaction_mode: ApplicationInteractionMode::Conversation,
                response_modes: vec![ApplicationResponseMode::Blocking],
            },
            workflow: ApplicationWorkflowBinding {
                workflow_definition_id: WorkflowDefinitionId::new(),
                workflow_revision_id: WorkflowRevisionId::new(),
                workflow_contract_digest: digest('a'),
                workflow_payload_set_digest: digest('b'),
                workflow_semantic_contract_set_digest: digest('c'),
                input_schema_digest: digest('d'),
                output_schema_digest: digest('e'),
            },
            presentation_digest: digest('f'),
        })
        .expect("contract");
        ApplicationRelease::initial(
            OrganizationId::new(),
            ProjectId::new(),
            ApplicationId::new(),
            ApplicationReleaseId::new(),
            contract,
            PrincipalId::new(),
            Utc.with_ymd_and_hms(2026, 9, 15, 12, 0, 0)
                .single()
                .expect("timestamp"),
        )
        .expect("release")
    }

    #[test]
    fn create_happy_path_and_restore() {
        let release = release();
        let intent = ApplicationPublicationRouteIntent::create(
            &release,
            vec![
                ApplicationPublicationChannel::ApiBlocking,
                ApplicationPublicationChannel::Embed,
                ApplicationPublicationChannel::Web,
            ],
            vec![
                "https://app.example.com/".into(),
                "https://docs.example.com".into(),
            ],
            rate_policy(),
        )
        .expect("create");
        assert_eq!(intent.organization_id, release.organization_id);
        assert_eq!(intent.application_release_digest, *release.contract.digest());
        assert_eq!(
            intent.embed_origin_allowlist,
            vec![
                "https://app.example.com".to_owned(),
                "https://docs.example.com".to_owned(),
            ]
        );
        intent.clone().restore().expect("restore");
        intent.validate().expect("validate");
    }

    #[test]
    fn unknown_channel_rejects() {
        assert!(ApplicationPublicationChannel::parse("sse").is_err());
        assert!(ApplicationPublicationChannel::parse("api-blocking").is_err());
    }

    #[test]
    fn duplicate_channel_rejects() {
        let release = release();
        assert!(
            ApplicationPublicationRouteIntent::create(
                &release,
                vec![
                    ApplicationPublicationChannel::Web,
                    ApplicationPublicationChannel::Web,
                ],
                Vec::new(),
                rate_policy(),
            )
            .is_err()
        );
    }

    #[test]
    fn empty_channels_reject() {
        let release = release();
        assert!(
            ApplicationPublicationRouteIntent::create(
                &release,
                Vec::new(),
                Vec::new(),
                rate_policy(),
            )
            .is_err()
        );
    }

    #[test]
    fn nil_or_blank_rate_profile_rejects() {
        assert!(ApplicationPublicationRateShapingPolicyRef::create("", digest('a')).is_err());
        assert!(ApplicationPublicationRateShapingPolicyRef::create("   ", digest('a')).is_err());
        let mut policy = rate_policy();
        policy.profile_id = String::new();
        let release = release();
        assert!(
            ApplicationPublicationRouteIntent::create(
                &release,
                vec![ApplicationPublicationChannel::Internal],
                Vec::new(),
                policy,
            )
            .is_err()
        );
    }

    #[test]
    fn digest_canonicality_and_identity_drift_fail_closed() {
        assert!(Sha256Digest::parse(format!("sha256:{}", "A".repeat(64))).is_err());
        let release = release();
        let mut intent = ApplicationPublicationRouteIntent::create(
            &release,
            vec![ApplicationPublicationChannel::Mcp],
            Vec::new(),
            rate_policy(),
        )
        .expect("create");
        intent.application_release_digest = digest('1');
        assert!(intent.validate().is_err());
    }

    #[test]
    fn identity_is_stable_on_exact_replay() {
        let release = release();
        let channels = vec![
            ApplicationPublicationChannel::ApiStreaming,
            ApplicationPublicationChannel::Embed,
        ];
        let origins = vec!["https://embed.example.com".into()];
        let first = ApplicationPublicationRouteIntent::create(
            &release,
            channels.clone(),
            origins.clone(),
            rate_policy(),
        )
        .expect("first");
        let again = ApplicationPublicationRouteIntent::create(
            &release,
            channels,
            origins,
            rate_policy(),
        )
        .expect("replay");
        assert_eq!(first.id, again.id);
        assert_eq!(first, again);
    }

    #[test]
    fn oversized_origins_reject() {
        let release = release();
        let origins = (0..=APPLICATION_PUBLICATION_EMBED_ORIGIN_MAX_ENTRIES)
            .map(|index| format!("https://tenant{index}.example.com"))
            .collect::<Vec<_>>();
        assert!(
            ApplicationPublicationRouteIntent::create(
                &release,
                vec![ApplicationPublicationChannel::Embed],
                origins,
                rate_policy(),
            )
            .is_err()
        );
    }

    #[test]
    fn empty_duplicate_or_invalid_origins_reject() {
        let release = release();
        assert!(
            ApplicationPublicationRouteIntent::create(
                &release,
                vec![ApplicationPublicationChannel::Embed],
                vec!["".into()],
                rate_policy(),
            )
            .is_err()
        );
        assert!(
            ApplicationPublicationRouteIntent::create(
                &release,
                vec![ApplicationPublicationChannel::Embed],
                vec![
                    "https://app.example.com".into(),
                    "https://app.example.com/".into(),
                ],
                rate_policy(),
            )
            .is_err()
        );
        assert!(
            ApplicationPublicationRouteIntent::create(
                &release,
                vec![ApplicationPublicationChannel::Embed],
                vec!["ftp://files.example.com".into()],
                rate_policy(),
            )
            .is_err()
        );
        assert!(
            ApplicationPublicationRouteIntent::create(
                &release,
                vec![ApplicationPublicationChannel::Web],
                vec!["https://app.example.com".into()],
                rate_policy(),
            )
            .is_err()
        );
    }
}
