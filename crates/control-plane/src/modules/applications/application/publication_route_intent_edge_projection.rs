//! Edge-facing desired-state projection for publication route intent.
//!
//! `APP0.3-C15` freezes the aggregate-free shape later Edge PublishRoute
//! admission and Gateway snapshot compile may consume. Applications still owns
//! the intent; this type is not an Edge command, Route aggregate, or live apply
//! acknowledgement. Rate shaping remains declare-only (Gateway enforces later
//! per distributed-api section 8).

use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationPublicationRouteIntentId, ApplicationReleaseId, OrganizationId,
    ProjectId, Sha256Digest,
};
use serde::Serialize;

/// Aggregate-free Edge desired-state projection of one publication route intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationPublicationRouteIntentEdgeProjection {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_digest: Sha256Digest,
    pub publication_route_intent_id: ApplicationPublicationRouteIntentId,
    /// Ordered snake_case channel names from the intent `BTreeSet`.
    pub channels: Vec<String>,
    pub embed_origin_allowlist: Vec<String>,
    pub rate_shaping_profile_id: String,
    pub rate_shaping_policy_revision_digest: Sha256Digest,
}

impl ApplicationPublicationRouteIntentEdgeProjection {
    /// Fail-closed consumer validation before Edge admission may trust the shape.
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.application_release_id.as_uuid().is_nil()
            || self.publication_route_intent_id.as_uuid().is_nil()
        {
            return Err(
                "Application publication route intent Edge projection identity is invalid".into(),
            );
        }
        if self.channels.is_empty() {
            return Err(
                "Application publication route intent Edge projection requires channels".into(),
            );
        }
        for channel in &self.channels {
            if channel.trim().is_empty() || channel.trim() != channel.as_str() {
                return Err(
                    "Application publication route intent Edge projection channel is not canonical"
                        .into(),
                );
            }
        }
        if self.rate_shaping_profile_id.trim().is_empty()
            || self.rate_shaping_profile_id.trim() != self.rate_shaping_profile_id.as_str()
        {
            return Err(
                "Application publication route intent Edge projection rate profile id is invalid"
                    .into(),
            );
        }
        if Sha256Digest::parse(self.application_release_digest.as_str())?
            != self.application_release_digest
        {
            return Err(
                "Application publication route intent Edge projection release digest is not canonical"
                    .into(),
            );
        }
        if Sha256Digest::parse(self.rate_shaping_policy_revision_digest.as_str())?
            != self.rate_shaping_policy_revision_digest
        {
            return Err(
                "Application publication route intent Edge projection rate policy digest is not canonical"
                    .into(),
            );
        }
        Ok(())
    }
}
