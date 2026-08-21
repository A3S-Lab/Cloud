use super::{ApplicationRelease, ApplicationWorkflowEffect};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, ApplicationId, ApplicationReleaseId,
    ApplicationSessionId, ConversationVariableRevisionId, OrganizationId, ProjectId, Sha256Digest,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const APPLICATION_CONVERSATION_VARIABLES_MAX_BYTES: usize = 256 * 1024;
const INITIAL_VARIABLES_IDENTITY: &[u8] = b"application-conversation-variables:initial:v1";

/// Immutable complete value snapshot for one optimistic session-variable
/// revision. Workflow owns run-local variables; this record owns only the
/// release-declared Applications session scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationVariableRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_digest: Sha256Digest,
    pub session_id: ApplicationSessionId,
    pub id: ConversationVariableRevisionId,
    pub revision_number: u64,
    pub parent_revision_id: Option<ConversationVariableRevisionId>,
    pub parent_digest: Option<Sha256Digest>,
    pub values: Value,
    pub values_digest: Sha256Digest,
    pub source_effect: Option<ApplicationWorkflowEffect>,
    pub created_at: DateTime<Utc>,
}

impl ConversationVariableRevision {
    pub fn initial(
        session_id: ApplicationSessionId,
        release: &ApplicationRelease,
        values: Value,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        release.validate()?;
        if session_id.as_uuid().is_nil() {
            return Err("Application session identity cannot be nil".into());
        }
        let created_at = canonical_timestamp(created_at);
        if created_at < release.created_at {
            return Err("Conversation variables cannot predate the Application release".into());
        }
        let values_digest = conversation_values_digest(&values)?;
        let value = Self {
            organization_id: release.organization_id,
            project_id: release.project_id,
            application_id: release.application_id,
            application_release_id: release.id,
            application_release_digest: release.contract.digest().clone(),
            session_id,
            id: ConversationVariableRevisionId::from_uuid(Uuid::new_v5(
                &session_id.as_uuid(),
                INITIAL_VARIABLES_IDENTITY,
            )),
            revision_number: 1,
            parent_revision_id: None,
            parent_digest: None,
            values,
            values_digest,
            source_effect: None,
            created_at,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn successor(
        parent: &Self,
        source_effect: ApplicationWorkflowEffect,
        values: Value,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        parent.validate()?;
        source_effect.validate()?;
        let values_digest = conversation_values_digest(&values)?;
        if values_digest == parent.values_digest {
            return Err("Conversation variable assignment must change the canonical value".into());
        }
        let created_at = canonical_timestamp(created_at);
        if created_at < parent.created_at {
            return Err("Conversation variable revision time regressed".into());
        }
        let id = Self::successor_id(parent.session_id, &source_effect)?;
        let value = Self {
            organization_id: parent.organization_id,
            project_id: parent.project_id,
            application_id: parent.application_id,
            application_release_id: parent.application_release_id,
            application_release_digest: parent.application_release_digest.clone(),
            session_id: parent.session_id,
            id,
            revision_number: parent
                .revision_number
                .checked_add(1)
                .ok_or_else(|| "Conversation variable revision number is exhausted".to_owned())?,
            parent_revision_id: Some(parent.id),
            parent_digest: Some(parent.values_digest.clone()),
            values,
            values_digest,
            source_effect: Some(source_effect),
            created_at,
        };
        value.validate()?;
        Ok(value)
    }

    /// Deterministic identity used to recover one optimistic Workflow write
    /// even after the session variable head has advanced again.
    pub fn successor_id(
        session_id: ApplicationSessionId,
        source_effect: &ApplicationWorkflowEffect,
    ) -> Result<ConversationVariableRevisionId, String> {
        if session_id.as_uuid().is_nil() {
            return Err("Application session identity cannot be nil".into());
        }
        source_effect.validate()?;
        Ok(ConversationVariableRevisionId::from_uuid(
            source_effect.deterministic_uuid(session_id.as_uuid(), "conversation-variables")?,
        ))
    }

    pub fn restore(mut self) -> Result<Self, String> {
        self.created_at = canonical_timestamp(self.created_at);
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.application_release_id.as_uuid().is_nil()
            || self.session_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.revision_number == 0
            || Sha256Digest::parse(self.application_release_digest.as_str())?
                != self.application_release_digest
            || self
                .parent_digest
                .as_ref()
                .map(|digest| Sha256Digest::parse(digest.as_str()))
                .transpose()?
                .as_ref()
                != self.parent_digest.as_ref()
            || conversation_values_digest(&self.values)? != self.values_digest
            || self.created_at != canonical_timestamp(self.created_at)
        {
            return Err("stored Conversation variable revision is invalid".into());
        }
        match (
            self.revision_number,
            self.parent_revision_id,
            self.parent_digest.as_ref(),
            self.source_effect.as_ref(),
        ) {
            (1, None, None, None) => {
                let expected = Uuid::new_v5(&self.session_id.as_uuid(), INITIAL_VARIABLES_IDENTITY);
                if self.id.as_uuid() != expected {
                    return Err("initial Conversation variable identity drifted".into());
                }
            }
            (revision_number, Some(parent_id), Some(parent_digest), Some(effect))
                if revision_number > 1
                    && !parent_id.as_uuid().is_nil()
                    && parent_digest != &self.values_digest =>
            {
                effect.validate()?;
                let expected = Self::successor_id(self.session_id, effect)?;
                if self.id != expected {
                    return Err("Conversation variable effect identity drifted".into());
                }
            }
            _ => return Err("Conversation variable lineage is invalid".into()),
        }
        Ok(())
    }

    pub fn validate_successor_of(&self, parent: &Self) -> Result<(), String> {
        self.validate()?;
        parent.validate()?;
        let effect = self.source_effect.clone().ok_or_else(|| {
            "Conversation variable successor requires a Workflow effect".to_owned()
        })?;
        let expected = Self::successor(parent, effect, self.values.clone(), self.created_at)?;
        if expected != *self {
            return Err("Conversation variable successor changed immutable lineage".into());
        }
        Ok(())
    }
}

fn conversation_values_digest(value: &Value) -> Result<Sha256Digest, String> {
    if !value.is_object() {
        return Err("Conversation variables must be a JSON object".into());
    }
    Ok(Sha256Digest::from_bytes(&canonical_json_bounded(
        value,
        APPLICATION_CONVERSATION_VARIABLES_MAX_BYTES,
        "Conversation variables",
    )?))
}
