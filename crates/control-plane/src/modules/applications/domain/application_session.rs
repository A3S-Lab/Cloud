use super::{
    ApplicationEndUser, ApplicationInteractionMode, ApplicationMessage, ApplicationRelease,
    ConversationVariableRevision,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ApplicationEndUserId, ApplicationId, ApplicationReleaseId,
    ApplicationSessionId, ConversationVariableRevisionId, OrganizationId, ProjectId, Sha256Digest,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationSessionStatus {
    Active,
    Closed,
}

impl ApplicationSessionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Closed => "closed",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "active" => Ok(Self::Active),
            "closed" => Ok(Self::Closed),
            _ => Err(format!("unsupported Application session status {value:?}")),
        }
    }
}

/// Channel-visible Application conversation or one-shot invocation session.
///
/// It pins one immutable release and owns only ordered presentation messages
/// plus Applications-scoped conversation-variable heads. WorkflowRun and Flow
/// remain the execution and history authorities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationSession {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_number: u64,
    pub application_release_digest: Sha256Digest,
    pub end_user_id: ApplicationEndUserId,
    pub id: ApplicationSessionId,
    pub interaction_mode: ApplicationInteractionMode,
    pub status: ApplicationSessionStatus,
    pub last_message_sequence: u64,
    pub current_variable_revision_id: ConversationVariableRevisionId,
    pub current_variable_revision_number: u64,
    pub current_variable_digest: Sha256Digest,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
}

impl ApplicationSession {
    pub fn create(
        id: ApplicationSessionId,
        release: &ApplicationRelease,
        end_user: &ApplicationEndUser,
        initial_variables: &ConversationVariableRevision,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        release.validate()?;
        end_user.validate_release(release)?;
        initial_variables.validate()?;
        let created_at = canonical_timestamp(created_at);
        if created_at < release.created_at
            || created_at < end_user.created_at
            || initial_variables.organization_id != release.organization_id
            || initial_variables.project_id != release.project_id
            || initial_variables.application_id != release.application_id
            || initial_variables.application_release_id != release.id
            || &initial_variables.application_release_digest != release.contract.digest()
            || initial_variables.session_id != id
            || initial_variables.revision_number != 1
            || initial_variables.created_at != created_at
        {
            return Err(
                "initial Application session state does not match its exact release".into(),
            );
        }
        let value = Self {
            organization_id: release.organization_id,
            project_id: release.project_id,
            application_id: release.application_id,
            application_release_id: release.id,
            application_release_number: release.release_number,
            application_release_digest: release.contract.digest().clone(),
            end_user_id: end_user.id,
            id,
            interaction_mode: release.contract.spec().delivery.interaction_mode,
            status: ApplicationSessionStatus::Active,
            last_message_sequence: 0,
            current_variable_revision_id: initial_variables.id,
            current_variable_revision_number: initial_variables.revision_number,
            current_variable_digest: initial_variables.values_digest.clone(),
            aggregate_version: 1,
            created_at,
            updated_at: created_at,
            closed_at: None,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn append_message(
        &self,
        expected_version: u64,
        message: &ApplicationMessage,
    ) -> Result<Self, String> {
        self.validate()?;
        message.validate()?;
        let expected_sequence = self
            .last_message_sequence
            .checked_add(1)
            .ok_or_else(|| "Application session message sequence is exhausted".to_owned())?;
        if self.status != ApplicationSessionStatus::Active
            || expected_version == 0
            || expected_version != self.aggregate_version
            || message.organization_id != self.organization_id
            || message.project_id != self.project_id
            || message.application_id != self.application_id
            || message.application_release_id != self.application_release_id
            || message.application_release_digest != self.application_release_digest
            || message.session_id != self.id
            || message.sequence != expected_sequence
            || message.created_at < self.updated_at
        {
            return Err("Application message is stale, foreign, or out of sequence".into());
        }
        let mut value = self.clone();
        value.last_message_sequence = expected_sequence;
        value.aggregate_version = self.next_aggregate_version()?;
        value.updated_at = std::cmp::max(self.updated_at, message.created_at);
        value.validate()?;
        Ok(value)
    }

    pub fn advance_variables(
        &self,
        expected_version: u64,
        revision: &ConversationVariableRevision,
    ) -> Result<Self, String> {
        self.validate()?;
        revision.validate()?;
        let expected_revision_number = self
            .current_variable_revision_number
            .checked_add(1)
            .ok_or_else(|| "Conversation variable revision number is exhausted".to_owned())?;
        if self.status != ApplicationSessionStatus::Active
            || expected_version == 0
            || expected_version != self.aggregate_version
            || revision.organization_id != self.organization_id
            || revision.project_id != self.project_id
            || revision.application_id != self.application_id
            || revision.application_release_id != self.application_release_id
            || revision.application_release_digest != self.application_release_digest
            || revision.session_id != self.id
            || revision.revision_number != expected_revision_number
            || revision.parent_revision_id != Some(self.current_variable_revision_id)
            || revision.parent_digest.as_ref() != Some(&self.current_variable_digest)
            || revision.created_at < self.updated_at
        {
            return Err("Conversation variable revision is stale, foreign, or forked".into());
        }
        let mut value = self.clone();
        value.current_variable_revision_id = revision.id;
        value.current_variable_revision_number = revision.revision_number;
        value.current_variable_digest = revision.values_digest.clone();
        value.aggregate_version = self.next_aggregate_version()?;
        value.updated_at = std::cmp::max(self.updated_at, revision.created_at);
        value.validate()?;
        Ok(value)
    }

    pub fn close(&self, expected_version: u64, closed_at: DateTime<Utc>) -> Result<Self, String> {
        self.validate()?;
        let closed_at = canonical_timestamp(closed_at);
        if self.status != ApplicationSessionStatus::Active
            || expected_version == 0
            || expected_version != self.aggregate_version
            || closed_at < self.updated_at
        {
            return Err("Application session close is stale or time-regressing".into());
        }
        let mut value = self.clone();
        value.status = ApplicationSessionStatus::Closed;
        value.aggregate_version = self.next_aggregate_version()?;
        value.updated_at = closed_at;
        value.closed_at = Some(closed_at);
        value.validate()?;
        Ok(value)
    }

    pub fn restore(mut self) -> Result<Self, String> {
        self.created_at = canonical_timestamp(self.created_at);
        self.updated_at = canonical_timestamp(self.updated_at);
        self.closed_at = self.closed_at.map(canonical_timestamp);
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        let expected_version = 1_u64
            .checked_add(self.last_message_sequence)
            .and_then(|value| {
                value.checked_add(self.current_variable_revision_number.saturating_sub(1))
            })
            .and_then(|value| {
                value.checked_add(u64::from(self.status == ApplicationSessionStatus::Closed))
            })
            .ok_or_else(|| "Application session aggregate version is exhausted".to_owned())?;
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.application_release_id.as_uuid().is_nil()
            || self.application_release_number == 0
            || self.end_user_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.current_variable_revision_id.as_uuid().is_nil()
            || self.current_variable_revision_number == 0
            || self.aggregate_version != expected_version
            || Sha256Digest::parse(self.application_release_digest.as_str())?
                != self.application_release_digest
            || Sha256Digest::parse(self.current_variable_digest.as_str())?
                != self.current_variable_digest
            || self.created_at != canonical_timestamp(self.created_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.created_at
            || self
                .closed_at
                .is_some_and(|closed_at| closed_at != canonical_timestamp(closed_at))
            || (self.status == ApplicationSessionStatus::Closed) != self.closed_at.is_some()
            || self
                .closed_at
                .is_some_and(|closed_at| closed_at != self.updated_at)
        {
            return Err("stored Application session aggregate is invalid".into());
        }
        Ok(())
    }

    pub fn validate_release(&self, release: &ApplicationRelease) -> Result<(), String> {
        self.validate()?;
        release.validate()?;
        if self.organization_id != release.organization_id
            || self.project_id != release.project_id
            || self.application_id != release.application_id
            || self.application_release_id != release.id
            || self.application_release_number != release.release_number
            || &self.application_release_digest != release.contract.digest()
            || self.interaction_mode != release.contract.spec().delivery.interaction_mode
            || self.created_at < release.created_at
        {
            return Err("Application session is not pinned to the exact release".into());
        }
        Ok(())
    }

    fn next_aggregate_version(&self) -> Result<u64, String> {
        self.aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Application session aggregate version is exhausted".to_owned())
    }
}
