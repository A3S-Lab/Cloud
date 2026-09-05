use super::definition::AutomationRevisionV1;
use super::invocation::AutomationInvocationEnvelopeV1;
use super::validation::{validate_digest, validate_single_line, validate_timestamp, validate_uuid};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const AUTOMATION_AUDIT_SCHEMA_V1: &str = "cloud.automation.audit.v1";
pub const AUTOMATION_OUTBOX_SCHEMA_V1: &str = "cloud.automation.outbox.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationAuditActionV1 {
    DefinitionCreated,
    RevisionPublished,
    InvocationAdmitted,
    InvocationRejected,
    InvocationReplayed,
}

impl AutomationAuditActionV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DefinitionCreated => "definition_created",
            Self::RevisionPublished => "revision_published",
            Self::InvocationAdmitted => "invocation_admitted",
            Self::InvocationRejected => "invocation_rejected",
            Self::InvocationReplayed => "invocation_replayed",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "definition_created" => Ok(Self::DefinitionCreated),
            "revision_published" => Ok(Self::RevisionPublished),
            "invocation_admitted" => Ok(Self::InvocationAdmitted),
            "invocation_rejected" => Ok(Self::InvocationRejected),
            "invocation_replayed" => Ok(Self::InvocationReplayed),
            _ => Err(format!("unsupported Automation audit action {value:?}")),
        }
    }

    const fn requires_invocation(self) -> bool {
        matches!(
            self,
            Self::InvocationAdmitted | Self::InvocationRejected | Self::InvocationReplayed
        )
    }
}

/// Redacted audit fact. It records authorization and identity digests only;
/// provider payloads, credentials, and source facts stay with their owners.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationAuditRecordV1 {
    pub schema: String,
    pub audit_id: Uuid,
    pub action: AutomationAuditActionV1,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub automation_id: Uuid,
    pub revision_id: Uuid,
    pub invocation_id: Option<Uuid>,
    pub actor_id: Option<Uuid>,
    pub authorization_policy_digest: String,
    pub deduplication_key: Option<String>,
    pub correlation_id: Uuid,
    pub occurred_at: DateTime<Utc>,
}

impl AutomationAuditRecordV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUTOMATION_AUDIT_SCHEMA_V1 {
            return Err(format!(
                "unsupported Automation audit schema {:?}",
                self.schema
            ));
        }
        validate_uuid("Automation audit ID", self.audit_id)?;
        validate_uuid("Automation audit organization ID", self.organization_id)?;
        validate_uuid("Automation audit project ID", self.project_id)?;
        validate_uuid("Automation audit environment ID", self.environment_id)?;
        validate_uuid("Automation audit Automation ID", self.automation_id)?;
        validate_uuid("Automation audit revision ID", self.revision_id)?;
        if self.action.requires_invocation() != self.invocation_id.is_some() {
            return Err("Automation audit invocation identity does not match its action".into());
        }
        if self
            .invocation_id
            .is_some_and(|invocation_id| invocation_id.is_nil())
        {
            return Err("Automation audit invocation ID must not be nil".into());
        }
        if self.actor_id.is_some_and(|actor_id| actor_id.is_nil()) {
            return Err("Automation audit actor ID must not be nil".into());
        }
        validate_digest(
            "Automation audit authorization policy digest",
            &self.authorization_policy_digest,
        )?;
        if let Some(key) = &self.deduplication_key {
            validate_single_line("Automation audit deduplication key", key, 1_024)?;
        }
        validate_uuid("Automation audit correlation ID", self.correlation_id)?;
        validate_timestamp("Automation audit occurred_at", self.occurred_at)
    }

    pub fn for_revision_published(
        revision: &AutomationRevisionV1,
        audit_id: Uuid,
        actor_id: Option<Uuid>,
        correlation_id: Uuid,
        occurred_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        revision.validate()?;
        let definition = &revision.spec().definition;
        let record = Self {
            schema: AUTOMATION_AUDIT_SCHEMA_V1.into(),
            audit_id,
            action: AutomationAuditActionV1::RevisionPublished,
            organization_id: definition.organization_id,
            project_id: definition.project_id,
            environment_id: definition.environment_id,
            automation_id: definition.automation_id,
            revision_id: revision.spec().revision_id,
            invocation_id: None,
            actor_id,
            authorization_policy_digest: definition.authorization.policy_digest.clone(),
            deduplication_key: None,
            correlation_id,
            occurred_at,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn for_invocation(
        envelope: &AutomationInvocationEnvelopeV1,
        action: AutomationAuditActionV1,
        audit_id: Uuid,
        occurred_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if !action.requires_invocation() {
            return Err("Automation invocation audit action is required".into());
        }
        envelope.validate()?;
        let record = Self {
            schema: AUTOMATION_AUDIT_SCHEMA_V1.into(),
            audit_id,
            action,
            organization_id: envelope.organization_id,
            project_id: envelope.project_id,
            environment_id: envelope.environment_id,
            automation_id: envelope.automation_id,
            revision_id: envelope.automation_revision_id,
            invocation_id: Some(envelope.invocation_id),
            actor_id: envelope.authorization.principal_id,
            authorization_policy_digest: envelope.authorization.policy_digest.clone(),
            deduplication_key: Some(envelope.deduplication_key.clone()),
            correlation_id: envelope.correlation_id,
            occurred_at,
        };
        record.validate()?;
        Ok(record)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationOutboxEventKindV1 {
    RevisionPublished,
    InvocationAdmitted,
}

impl AutomationOutboxEventKindV1 {
    pub const fn event_key(self) -> &'static str {
        match self {
            Self::RevisionPublished => "automation.revision.published",
            Self::InvocationAdmitted => "automation.invocation.admitted",
        }
    }
}

/// Transactional publication fact. The payload is represented by a digest;
/// consumers resolve the exact owner contract instead of receiving copied
/// trigger/provider state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationOutboxMessageV1 {
    pub schema: String,
    pub message_id: Uuid,
    pub kind: AutomationOutboxEventKindV1,
    pub event_version: u32,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub automation_id: Uuid,
    pub revision_id: Uuid,
    pub invocation_id: Option<Uuid>,
    pub payload_digest: String,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
    pub occurred_at: DateTime<Utc>,
}

impl AutomationOutboxMessageV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUTOMATION_OUTBOX_SCHEMA_V1 {
            return Err(format!(
                "unsupported Automation Outbox schema {:?}",
                self.schema
            ));
        }
        validate_uuid("Automation Outbox message ID", self.message_id)?;
        if self.event_version != 1 {
            return Err("unsupported Automation Outbox event version".into());
        }
        validate_uuid("Automation Outbox organization ID", self.organization_id)?;
        validate_uuid("Automation Outbox project ID", self.project_id)?;
        validate_uuid("Automation Outbox environment ID", self.environment_id)?;
        validate_uuid("Automation Outbox Automation ID", self.automation_id)?;
        validate_uuid("Automation Outbox revision ID", self.revision_id)?;
        if matches!(self.kind, AutomationOutboxEventKindV1::InvocationAdmitted)
            != self.invocation_id.is_some()
        {
            return Err("Automation Outbox invocation identity does not match its event".into());
        }
        if self
            .invocation_id
            .is_some_and(|invocation_id| invocation_id.is_nil())
        {
            return Err("Automation Outbox invocation ID must not be nil".into());
        }
        if self
            .causation_id
            .is_some_and(|causation_id| causation_id.is_nil())
        {
            return Err("Automation Outbox causation ID must not be nil".into());
        }
        validate_digest("Automation Outbox payload digest", &self.payload_digest)?;
        validate_uuid("Automation Outbox correlation ID", self.correlation_id)?;
        validate_timestamp("Automation Outbox occurred_at", self.occurred_at)
    }

    pub fn event_key(&self) -> &'static str {
        self.kind.event_key()
    }

    pub fn for_revision(
        revision: &AutomationRevisionV1,
        message_id: Uuid,
        correlation_id: Uuid,
        causation_id: Option<Uuid>,
        occurred_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        revision.validate()?;
        let definition = &revision.spec().definition;
        let message = Self {
            schema: AUTOMATION_OUTBOX_SCHEMA_V1.into(),
            message_id,
            kind: AutomationOutboxEventKindV1::RevisionPublished,
            event_version: 1,
            organization_id: definition.organization_id,
            project_id: definition.project_id,
            environment_id: definition.environment_id,
            automation_id: definition.automation_id,
            revision_id: revision.spec().revision_id,
            invocation_id: None,
            payload_digest: revision.digest().into(),
            correlation_id,
            causation_id,
            occurred_at,
        };
        message.validate()?;
        Ok(message)
    }

    pub fn for_invocation(
        envelope: &AutomationInvocationEnvelopeV1,
        message_id: Uuid,
        causation_id: Option<Uuid>,
        occurred_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        envelope.validate()?;
        let payload_digest = envelope.digest()?;
        let message = Self {
            schema: AUTOMATION_OUTBOX_SCHEMA_V1.into(),
            message_id,
            kind: AutomationOutboxEventKindV1::InvocationAdmitted,
            event_version: 1,
            organization_id: envelope.organization_id,
            project_id: envelope.project_id,
            environment_id: envelope.environment_id,
            automation_id: envelope.automation_id,
            revision_id: envelope.automation_revision_id,
            invocation_id: Some(envelope.invocation_id),
            payload_digest,
            correlation_id: envelope.correlation_id,
            causation_id,
            occurred_at,
        };
        message.validate()?;
        Ok(message)
    }
}
