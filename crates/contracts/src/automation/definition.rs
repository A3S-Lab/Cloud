use super::validation::{
    sorted_unique, template_placeholders, validate_cron_expression, validate_digest,
    validate_event_key, validate_grant, validate_name, validate_single_line, validate_timezone,
    validate_uuid, MAX_SAFE_INTEGER,
};
use a3s_acl::{canonical_digest, generate_acl, parse_acl};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

use super::codec::{definition_document, parse_definition, parse_revision, revision_document};

pub const AUTOMATION_DEFINITION_SCHEMA_V1: &str = "cloud.automation.definition.v1";
pub const AUTOMATION_REVISION_SCHEMA_V1: &str = "cloud.automation.revision.v1";
pub const AUTOMATION_DEFINITION_MAX_ACL_BYTES: usize = 128 * 1024;
pub const AUTOMATION_MAX_NAME_BYTES: usize = 128;
pub const AUTOMATION_MAX_DEDUPLICATION_TEMPLATE_BYTES: usize = 512;
pub const AUTOMATION_MAX_DEDUPLICATION_WINDOW_MS: u64 = 30 * 24 * 60 * 60 * 1_000;
pub const AUTOMATION_MAX_CONCURRENCY: u64 = 4_096;
pub const AUTOMATION_MAX_MISFIRE_GRACE_MS: u64 = 7 * 24 * 60 * 60 * 1_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSubscriptionReferenceV1 {
    pub subscription_id: Uuid,
    pub revision_digest: String,
}

impl AutomationSubscriptionReferenceV1 {
    pub fn validate(&self) -> Result<(), String> {
        validate_uuid("Automation subscription ID", self.subscription_id)?;
        validate_digest(
            "Automation subscription revision digest",
            &self.revision_digest,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationScheduleTriggerV1 {
    pub expression: String,
    pub timezone: String,
}

impl AutomationScheduleTriggerV1 {
    fn validate(&self) -> Result<(), String> {
        validate_cron_expression(&self.expression)?;
        validate_timezone(&self.timezone)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationWebhookTriggerV1 {
    pub subscription: AutomationSubscriptionReferenceV1,
    pub request_schema_digest: String,
}

impl AutomationWebhookTriggerV1 {
    fn validate(&self) -> Result<(), String> {
        self.subscription.validate()?;
        validate_digest(
            "Automation webhook request schema digest",
            &self.request_schema_digest,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationEventTriggerV1 {
    pub subscription: AutomationSubscriptionReferenceV1,
    pub event_key: String,
    pub filter_digest: Option<String>,
}

impl AutomationEventTriggerV1 {
    fn validate(&self, kind: &str) -> Result<(), String> {
        self.subscription.validate()?;
        validate_event_key(&self.event_key)?;
        if let Some(digest) = &self.filter_digest {
            validate_digest(&format!("Automation {kind} filter digest"), digest)?;
        }
        Ok(())
    }
}

/// Closed trigger union. Provider connection and normalized source facts stay
/// with Sources or A3S Use; this value carries only the immutable trigger
/// identity and policy needed by Automations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "details",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AutomationTriggerV1 {
    Schedule(AutomationScheduleTriggerV1),
    Webhook(AutomationWebhookTriggerV1),
    PluginEvent(AutomationEventTriggerV1),
    SourceEvent(AutomationEventTriggerV1),
}

impl AutomationTriggerV1 {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Schedule(trigger) => trigger.validate(),
            Self::Webhook(trigger) => trigger.validate(),
            Self::PluginEvent(trigger) => trigger.validate("plugin-event"),
            Self::SourceEvent(trigger) => trigger.validate("source-event"),
        }
    }

    pub const fn subscription(&self) -> Option<&AutomationSubscriptionReferenceV1> {
        match self {
            Self::Schedule(_) => None,
            Self::Webhook(trigger) => Some(&trigger.subscription),
            Self::PluginEvent(trigger) | Self::SourceEvent(trigger) => Some(&trigger.subscription),
        }
    }

    pub const fn is_schedule(&self) -> bool {
        matches!(self, Self::Schedule(_))
    }

    pub const fn requires_event_identity(&self) -> bool {
        !self.is_schedule()
    }

    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Schedule(_) => "schedule",
            Self::Webhook(_) => "webhook",
            Self::PluginEvent(_) => "plugin_event",
            Self::SourceEvent(_) => "source_event",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTargetKindV1 {
    ApplicationRelease,
    WorkflowRevision,
    Task,
}

impl AutomationTargetKindV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ApplicationRelease => "application_release",
            Self::WorkflowRevision => "workflow_revision",
            Self::Task => "task",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationApplicationTargetV1 {
    pub application_id: Uuid,
    pub application_release_id: Uuid,
    pub release_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationWorkflowTargetV1 {
    pub workflow_definition_id: Uuid,
    pub workflow_revision_id: Uuid,
    pub revision_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationTaskTargetV1 {
    pub task_profile_id: Uuid,
    pub task_revision_id: Uuid,
    pub revision_digest: String,
}

/// Exact target union. A target is always a specific immutable release or
/// revision; mutable "latest" selectors are intentionally unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "binding",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AutomationTargetV1 {
    ApplicationRelease(AutomationApplicationTargetV1),
    WorkflowRevision(AutomationWorkflowTargetV1),
    Task(AutomationTaskTargetV1),
}

impl AutomationTargetV1 {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::ApplicationRelease(target) => {
                validate_uuid("Automation application ID", target.application_id)?;
                validate_uuid(
                    "Automation application release ID",
                    target.application_release_id,
                )?;
                validate_digest(
                    "Automation application release digest",
                    &target.release_digest,
                )
            }
            Self::WorkflowRevision(target) => {
                validate_uuid(
                    "Automation Workflow definition ID",
                    target.workflow_definition_id,
                )?;
                validate_uuid(
                    "Automation Workflow revision ID",
                    target.workflow_revision_id,
                )?;
                validate_digest(
                    "Automation Workflow revision digest",
                    &target.revision_digest,
                )
            }
            Self::Task(target) => {
                validate_uuid("Automation Task profile ID", target.task_profile_id)?;
                validate_uuid("Automation Task revision ID", target.task_revision_id)?;
                validate_digest("Automation Task revision digest", &target.revision_digest)
            }
        }
    }

    pub const fn kind(&self) -> AutomationTargetKindV1 {
        match self {
            Self::ApplicationRelease(_) => AutomationTargetKindV1::ApplicationRelease,
            Self::WorkflowRevision(_) => AutomationTargetKindV1::WorkflowRevision,
            Self::Task(_) => AutomationTargetKindV1::Task,
        }
    }

    pub fn revision_digest(&self) -> &str {
        match self {
            Self::ApplicationRelease(target) => &target.release_digest,
            Self::WorkflowRevision(target) => &target.revision_digest,
            Self::Task(target) => &target.revision_digest,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationDeduplicationScopeV1 {
    Automation,
    Subscription,
}

impl AutomationDeduplicationScopeV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Automation => "automation",
            Self::Subscription => "subscription",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "automation" => Ok(Self::Automation),
            "subscription" => Ok(Self::Subscription),
            _ => Err(format!(
                "unsupported Automation deduplication scope {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationDeduplicationPolicyV1 {
    pub scope: AutomationDeduplicationScopeV1,
    pub key_template: String,
    pub window_ms: u64,
}

impl AutomationDeduplicationPolicyV1 {
    fn validate(&self, trigger: &AutomationTriggerV1) -> Result<BTreeSet<String>, String> {
        let placeholders = template_placeholders(&self.key_template)?;
        if self.key_template.len() > AUTOMATION_MAX_DEDUPLICATION_TEMPLATE_BYTES {
            return Err("Automation deduplication template exceeds its byte bound".into());
        }
        if self.window_ms == 0 || self.window_ms > AUTOMATION_MAX_DEDUPLICATION_WINDOW_MS {
            return Err("Automation deduplication window is outside its closed bound".into());
        }
        if !placeholders.contains("automation_id") || !placeholders.contains("revision_id") {
            return Err(
                "Automation deduplication template must bind automation_id and revision_id".into(),
            );
        }
        if trigger.requires_event_identity() {
            if !placeholders.contains("event_id") {
                return Err(
                    "event-trigger deduplication template must bind the normalized event_id".into(),
                );
            }
            if self.scope == AutomationDeduplicationScopeV1::Subscription
                && !placeholders.contains("subscription_id")
            {
                return Err(
                    "subscription-scoped deduplication template must bind subscription_id".into(),
                );
            }
        } else if placeholders.contains("event_id") || placeholders.contains("subscription_id") {
            return Err("schedule deduplication template cannot bind an event identity".into());
        }
        if self.scope == AutomationDeduplicationScopeV1::Subscription
            && trigger.subscription().is_none()
        {
            return Err("subscription-scoped deduplication requires a subscription trigger".into());
        }
        Ok(placeholders)
    }

    pub fn render_key(
        &self,
        automation_id: Uuid,
        revision_id: Uuid,
        origin: &super::invocation::AutomationInvocationOriginV1,
        subscription_id: Option<Uuid>,
    ) -> Result<String, String> {
        origin.validate()?;
        validate_uuid("Automation ID", automation_id)?;
        validate_uuid("Automation revision ID", revision_id)?;
        let mut key = self.key_template.clone();
        key = key.replace("{automation_id}", &automation_id.to_string());
        key = key.replace("{revision_id}", &revision_id.to_string());
        if let Some(subscription_id) = subscription_id {
            validate_uuid("Automation subscription ID", subscription_id)?;
            key = key.replace("{subscription_id}", &subscription_id.to_string());
        }
        match origin {
            super::invocation::AutomationInvocationOriginV1::DueTime { scheduled_at } => {
                key = key.replace(
                    "{scheduled_at}",
                    &super::validation::timestamp_string(*scheduled_at),
                );
            }
            super::invocation::AutomationInvocationOriginV1::Event { event_id, .. } => {
                validate_uuid("Automation event ID", *event_id)?;
                key = key.replace("{event_id}", &event_id.to_string());
            }
        }
        if key.contains('{') || key.contains('}') {
            return Err("Automation deduplication key could not resolve its template".into());
        }
        validate_single_line("Automation deduplication key", &key, 1_024)?;
        Ok(key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationConcurrencyModeV1 {
    Queue,
    Drop,
    Replace,
}

impl AutomationConcurrencyModeV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::Drop => "drop",
            Self::Replace => "replace",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "queue" => Ok(Self::Queue),
            "drop" => Ok(Self::Drop),
            "replace" => Ok(Self::Replace),
            _ => Err(format!("unsupported Automation concurrency mode {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationConcurrencyPolicyV1 {
    pub maximum: u64,
    pub mode: AutomationConcurrencyModeV1,
}

impl AutomationConcurrencyPolicyV1 {
    fn validate(&self) -> Result<(), String> {
        if self.maximum == 0 || self.maximum > AUTOMATION_MAX_CONCURRENCY {
            return Err("Automation maximum concurrency is outside its closed bound".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationMisfireModeV1 {
    Skip,
    FireOnce,
    FireLatest,
}

impl AutomationMisfireModeV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::FireOnce => "fire_once",
            Self::FireLatest => "fire_latest",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "skip" => Ok(Self::Skip),
            "fire_once" => Ok(Self::FireOnce),
            "fire_latest" => Ok(Self::FireLatest),
            _ => Err(format!("unsupported Automation misfire mode {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationMisfirePolicyV1 {
    pub mode: AutomationMisfireModeV1,
    pub grace_ms: u64,
}

impl AutomationMisfirePolicyV1 {
    fn validate(&self) -> Result<(), String> {
        if self.grace_ms > AUTOMATION_MAX_MISFIRE_GRACE_MS {
            return Err("Automation misfire grace is outside its closed bound".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationTriggerPolicyV1 {
    pub deduplication: AutomationDeduplicationPolicyV1,
    pub concurrency: AutomationConcurrencyPolicyV1,
    pub misfire: AutomationMisfirePolicyV1,
}

impl AutomationTriggerPolicyV1 {
    fn validate(&self, trigger: &AutomationTriggerV1) -> Result<(), String> {
        self.deduplication.validate(trigger)?;
        self.concurrency.validate()?;
        self.misfire.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationAuthorizationPolicyV1 {
    pub policy_digest: String,
    pub required_grants: Vec<String>,
}

impl AutomationAuthorizationPolicyV1 {
    fn normalize_and_validate(&mut self) -> Result<(), String> {
        validate_digest(
            "Automation authorization policy digest",
            &self.policy_digest,
        )?;
        if self.required_grants.is_empty() || self.required_grants.len() > 32 {
            return Err("Automation authorization must contain one to 32 grants".into());
        }
        for grant in &self.required_grants {
            validate_grant(grant)?;
        }
        sorted_unique(&mut self.required_grants, "Automation authorization grants")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationDefinitionSpecV1 {
    pub schema: String,
    pub automation_id: Uuid,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub name: String,
    pub trigger: AutomationTriggerV1,
    pub target: AutomationTargetV1,
    pub policy: AutomationTriggerPolicyV1,
    pub authorization: AutomationAuthorizationPolicyV1,
}

impl AutomationDefinitionSpecV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUTOMATION_DEFINITION_SCHEMA_V1 {
            return Err(format!(
                "unsupported Automation definition schema {:?}",
                self.schema
            ));
        }
        validate_uuid("Automation ID", self.automation_id)?;
        validate_uuid("Automation organization ID", self.organization_id)?;
        validate_uuid("Automation project ID", self.project_id)?;
        validate_uuid("Automation environment ID", self.environment_id)?;
        validate_name("Automation name", &self.name, AUTOMATION_MAX_NAME_BYTES)?;
        self.trigger.validate()?;
        self.target.validate()?;
        self.policy.validate(&self.trigger)?;
        let mut authorization = self.authorization.clone();
        authorization.normalize_and_validate()
    }
}

/// Canonical immutable trigger intent. Persistence and delivery layers may
/// attach lifecycle metadata, but they must retain this exact ACL and digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationDefinitionV1 {
    spec: AutomationDefinitionSpecV1,
    canonical_acl: String,
    digest: String,
}

impl AutomationDefinitionV1 {
    pub const SCHEMA: &'static str = AUTOMATION_DEFINITION_SCHEMA_V1;

    pub fn from_spec(mut spec: AutomationDefinitionSpecV1) -> Result<Self, String> {
        spec.authorization.normalize_and_validate()?;
        spec.validate()?;
        let document = definition_document(&spec)?;
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > AUTOMATION_DEFINITION_MAX_ACL_BYTES {
            return Err("Automation definition ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated Automation definition ACL is invalid: {error}"))?;
        let reparsed_spec = parse_definition(&reparsed)?;
        if reparsed_spec != spec {
            return Err("generated Automation definition ACL changed its semantic value".into());
        }
        let digest = canonical_digest(&reparsed)
            .map_err(|error| format!("Automation definition is not canonicalizable: {error}"))?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > AUTOMATION_DEFINITION_MAX_ACL_BYTES {
            return Err("Automation definition ACL size is invalid".into());
        }
        if source.replace("\r\n", "").contains('\r') {
            return Err("Automation definition ACL contains a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let document = parse_acl(&normalized)
            .map_err(|error| format!("Automation definition ACL is invalid: {error}"))?;
        let definition = Self::from_spec(parse_definition(&document)?)?;
        if definition.canonical_acl != normalized {
            return Err("Automation definition ACL is not canonical".into());
        }
        Ok(definition)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let definition = Self::parse_acl(source)?;
        if definition.digest != stored_digest {
            return Err("stored Automation definition ACL and digest do not match".into());
        }
        Ok(definition)
    }

    pub fn validate(&self) -> Result<(), String> {
        let restored = Self::restore(&self.canonical_acl, &self.digest)?;
        if restored != *self {
            return Err("Automation definition drifted from its canonical ACL".into());
        }
        Ok(())
    }

    pub const fn spec(&self) -> &AutomationDefinitionSpecV1 {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationRevisionSpecV1 {
    pub schema: String,
    pub revision_id: Uuid,
    pub revision_number: u64,
    pub parent_revision_id: Option<Uuid>,
    pub parent_digest: Option<String>,
    pub definition: AutomationDefinitionSpecV1,
}

impl AutomationRevisionSpecV1 {
    fn validate(&self) -> Result<(), String> {
        if self.schema != AUTOMATION_REVISION_SCHEMA_V1 {
            return Err(format!(
                "unsupported Automation revision schema {:?}",
                self.schema
            ));
        }
        validate_uuid("Automation revision ID", self.revision_id)?;
        if self.revision_number == 0 || self.revision_number > MAX_SAFE_INTEGER {
            return Err("Automation revision number is outside its JSON-safe bound".into());
        }
        match (
            self.revision_number,
            self.parent_revision_id,
            &self.parent_digest,
        ) {
            (1, None, None) => {}
            (1, Some(_), _) | (1, _, Some(_)) => {
                return Err("first Automation revision cannot have a parent".into())
            }
            (_, None, None) => return Err("successor Automation revision requires a parent".into()),
            (_, Some(parent_id), Some(parent_digest)) => {
                validate_uuid("Automation parent revision ID", parent_id)?;
                validate_digest("Automation parent revision digest", parent_digest)?;
            }
            (_, Some(_), None) | (_, None, Some(_)) => {
                return Err("Automation revision parent ID and digest must be paired".into())
            }
        }
        self.definition.validate()?;
        if self.definition.automation_id.is_nil() {
            return Err("Automation revision definition ID must not be nil".into());
        }
        Ok(())
    }
}

/// Immutable, digest-linked lineage for one Automation definition revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationRevisionV1 {
    spec: AutomationRevisionSpecV1,
    canonical_acl: String,
    digest: String,
}

impl AutomationRevisionV1 {
    pub const SCHEMA: &'static str = AUTOMATION_REVISION_SCHEMA_V1;

    pub fn from_spec(mut spec: AutomationRevisionSpecV1) -> Result<Self, String> {
        spec.definition.authorization.normalize_and_validate()?;
        spec.validate()?;
        let document = revision_document(&spec)?;
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > AUTOMATION_DEFINITION_MAX_ACL_BYTES {
            return Err("Automation revision ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated Automation revision ACL is invalid: {error}"))?;
        let reparsed_spec = parse_revision(&reparsed)?;
        if reparsed_spec != spec {
            return Err("generated Automation revision ACL changed its semantic value".into());
        }
        let digest = canonical_digest(&reparsed)
            .map_err(|error| format!("Automation revision is not canonicalizable: {error}"))?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn from_definition(
        revision_id: Uuid,
        revision_number: u64,
        parent: Option<&Self>,
        definition: AutomationDefinitionSpecV1,
    ) -> Result<Self, String> {
        let (parent_revision_id, parent_digest) = parent
            .map(|value| (Some(value.spec.revision_id), Some(value.digest.clone())))
            .unwrap_or((None, None));
        Self::from_spec(AutomationRevisionSpecV1 {
            schema: Self::SCHEMA.into(),
            revision_id,
            revision_number,
            parent_revision_id,
            parent_digest,
            definition,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > AUTOMATION_DEFINITION_MAX_ACL_BYTES {
            return Err("Automation revision ACL size is invalid".into());
        }
        if source.replace("\r\n", "").contains('\r') {
            return Err("Automation revision ACL contains a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let document = parse_acl(&normalized)
            .map_err(|error| format!("Automation revision ACL is invalid: {error}"))?;
        let revision = Self::from_spec(parse_revision(&document)?)?;
        if revision.canonical_acl != normalized {
            return Err("Automation revision ACL is not canonical".into());
        }
        Ok(revision)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let revision = Self::parse_acl(source)?;
        if revision.digest != stored_digest {
            return Err("stored Automation revision ACL and digest do not match".into());
        }
        Ok(revision)
    }

    pub fn validate(&self) -> Result<(), String> {
        let restored = Self::restore(&self.canonical_acl, &self.digest)?;
        if restored != *self {
            return Err("Automation revision drifted from its canonical ACL".into());
        }
        Ok(())
    }

    pub fn validate_successor_of(&self, parent: &Self) -> Result<(), String> {
        self.validate()?;
        parent.validate()?;
        if self.spec.definition.automation_id != parent.spec.definition.automation_id {
            return Err("Automation successor changed its definition identity".into());
        }
        if self.spec.revision_id == parent.spec.revision_id {
            return Err("Automation successor must use a new revision identity".into());
        }
        let expected_revision_number = parent
            .spec
            .revision_number
            .checked_add(1)
            .ok_or_else(|| "Automation parent revision number overflowed".to_owned())?;
        if self.spec.revision_number != expected_revision_number
            || self.spec.parent_revision_id != Some(parent.spec.revision_id)
            || self.spec.parent_digest.as_deref() != Some(parent.digest.as_str())
        {
            return Err("Automation successor lineage is not contiguous".into());
        }
        if self.spec.definition == parent.spec.definition {
            return Err("Automation successor must change immutable trigger intent".into());
        }
        Ok(())
    }

    pub const fn spec(&self) -> &AutomationRevisionSpecV1 {
        &self.spec
    }

    pub fn definition(&self) -> Result<AutomationDefinitionV1, String> {
        AutomationDefinitionV1::from_spec(self.spec.definition.clone())
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}
