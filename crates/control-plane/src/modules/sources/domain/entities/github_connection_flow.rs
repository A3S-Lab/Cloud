use crate::modules::shared_kernel::domain::{canonical_timestamp, OrganizationId};
use crate::modules::sources::domain::value_objects::GithubInstallationId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubConnectionFlowStage {
    AwaitingInstallation,
    AwaitingOauth,
    Completed,
}

impl GithubConnectionFlowStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingInstallation => "awaiting_installation",
            Self::AwaitingOauth => "awaiting_oauth",
            Self::Completed => "completed",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "awaiting_installation" => Ok(Self::AwaitingInstallation),
            "awaiting_oauth" => Ok(Self::AwaitingOauth),
            "completed" => Ok(Self::Completed),
            _ => Err("GitHub connection flow stage is invalid".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubConnectionFlow {
    pub id: Uuid,
    pub organization_id: OrganizationId,
    pub stage: GithubConnectionFlowStage,
    pub state_digest: String,
    pub installation_id: Option<GithubInstallationId>,
    pub pkce_verifier_digest: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub consumed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GithubConnectionFlowError {
    #[error("GitHub connection state is invalid")]
    InvalidState,
    #[error("GitHub connection state has expired")]
    Expired,
    #[error("GitHub connection state has already been used")]
    Replayed,
}

impl GithubConnectionFlow {
    pub fn begin(
        id: Uuid,
        organization_id: OrganizationId,
        state_digest: String,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::restore(Self {
            id,
            organization_id,
            stage: GithubConnectionFlowStage::AwaitingInstallation,
            state_digest,
            installation_id: None,
            pkce_verifier_digest: None,
            created_at,
            expires_at,
            consumed_at: None,
        })
    }

    pub fn restore(mut flow: Self) -> Result<Self, String> {
        if !is_sha256_digest(&flow.state_digest) {
            return Err("GitHub connection state digest is invalid".into());
        }
        flow.created_at = canonical_timestamp(flow.created_at);
        flow.expires_at = canonical_timestamp(flow.expires_at);
        flow.consumed_at = flow.consumed_at.map(canonical_timestamp);
        let lifetime = flow.expires_at - flow.created_at;
        if lifetime < chrono::Duration::minutes(1) || lifetime > chrono::Duration::minutes(30) {
            return Err("GitHub connection state lifetime must be between 1 and 30 minutes".into());
        }
        let fields_match_stage = match flow.stage {
            GithubConnectionFlowStage::AwaitingInstallation => {
                flow.installation_id.is_none()
                    && flow.pkce_verifier_digest.is_none()
                    && flow.consumed_at.is_none()
            }
            GithubConnectionFlowStage::AwaitingOauth => {
                flow.installation_id.is_some()
                    && flow
                        .pkce_verifier_digest
                        .as_deref()
                        .is_some_and(is_sha256_digest)
                    && flow.consumed_at.is_none()
            }
            GithubConnectionFlowStage::Completed => {
                flow.installation_id.is_some()
                    && flow
                        .pkce_verifier_digest
                        .as_deref()
                        .is_some_and(is_sha256_digest)
                    && flow.consumed_at.is_some_and(|consumed_at| {
                        consumed_at >= flow.created_at && consumed_at < flow.expires_at
                    })
            }
        };
        if !fields_match_stage {
            return Err("GitHub connection flow fields do not match its stage".into());
        }
        Ok(flow)
    }

    pub fn prepare_oauth(
        &mut self,
        installation_id: GithubInstallationId,
        oauth_state_digest: String,
        pkce_verifier_digest: String,
        now: DateTime<Utc>,
    ) -> Result<(), GithubConnectionFlowError> {
        self.require_live(now)?;
        if self.stage != GithubConnectionFlowStage::AwaitingInstallation {
            return Err(GithubConnectionFlowError::Replayed);
        }
        if !is_sha256_digest(&oauth_state_digest) || !is_sha256_digest(&pkce_verifier_digest) {
            return Err(GithubConnectionFlowError::InvalidState);
        }
        self.stage = GithubConnectionFlowStage::AwaitingOauth;
        self.state_digest = oauth_state_digest;
        self.installation_id = Some(installation_id);
        self.pkce_verifier_digest = Some(pkce_verifier_digest);
        Ok(())
    }

    pub fn require_oauth(
        &self,
        state_digest: &str,
        pkce_verifier_digest: &str,
        now: DateTime<Utc>,
    ) -> Result<GithubInstallationId, GithubConnectionFlowError> {
        self.require_live(now)?;
        if self.stage == GithubConnectionFlowStage::Completed {
            return Err(GithubConnectionFlowError::Replayed);
        }
        if self.stage != GithubConnectionFlowStage::AwaitingOauth
            || self.state_digest != state_digest
            || self.pkce_verifier_digest.as_deref() != Some(pkce_verifier_digest)
        {
            return Err(GithubConnectionFlowError::InvalidState);
        }
        self.installation_id
            .ok_or(GithubConnectionFlowError::InvalidState)
    }

    pub fn complete(&mut self, now: DateTime<Utc>) -> Result<(), GithubConnectionFlowError> {
        self.require_live(now)?;
        if self.stage == GithubConnectionFlowStage::Completed {
            return Err(GithubConnectionFlowError::Replayed);
        }
        if self.stage != GithubConnectionFlowStage::AwaitingOauth {
            return Err(GithubConnectionFlowError::InvalidState);
        }
        self.stage = GithubConnectionFlowStage::Completed;
        self.consumed_at = Some(canonical_timestamp(now));
        Ok(())
    }

    fn require_live(&self, now: DateTime<Utc>) -> Result<(), GithubConnectionFlowError> {
        if self.consumed_at.is_some() || self.stage == GithubConnectionFlowStage::Completed {
            return Err(GithubConnectionFlowError::Replayed);
        }
        if canonical_timestamp(now) >= self.expires_at {
            return Err(GithubConnectionFlowError::Expired);
        }
        Ok(())
    }
}

fn is_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}
