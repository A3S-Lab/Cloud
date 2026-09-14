use crate::modules::automations::domain::{
    AutomationDefinitionRecord, AutomationWebhookEndpointRecord,
};
use a3s_cloud_contracts::{
    AutomationRevisionV1, AutomationWebhookSecretReferenceV1,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateAutomationWebhookEndpointRequest {
    pub endpoint_id: Uuid,
    pub endpoint_key: String,
    pub signing_secret: AutomationWebhookSecretReferenceV1,
    pub max_body_bytes: u64,
    pub automation_id: Uuid,
    pub revision_id: Uuid,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeAutomationWebhookEndpointRequest {
    pub expected_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationWebhookSecretReferenceResponse {
    pub secret_id: Uuid,
    pub version: u64,
}

impl From<AutomationWebhookSecretReferenceV1> for AutomationWebhookSecretReferenceResponse {
    fn from(value: AutomationWebhookSecretReferenceV1) -> Self {
        Self {
            secret_id: value.secret_id,
            version: value.version,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationWebhookEndpointResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub endpoint_id: Uuid,
    pub endpoint_key: String,
    pub automation_id: Uuid,
    pub revision_id: Uuid,
    pub revision_digest: String,
    pub signature_algorithm: String,
    pub signing_secret: AutomationWebhookSecretReferenceResponse,
    pub request_schema_digest: String,
    pub max_body_bytes: u64,
    pub generation: u64,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub state_changed_at: Option<DateTime<Utc>>,
}

impl From<AutomationWebhookEndpointRecord> for AutomationWebhookEndpointResponse {
    fn from(record: AutomationWebhookEndpointRecord) -> Self {
        let endpoint = record.endpoint;
        Self {
            organization_id: endpoint.organization_id,
            project_id: endpoint.project_id,
            environment_id: endpoint.environment_id,
            endpoint_id: endpoint.endpoint_id,
            endpoint_key: endpoint.endpoint_key,
            automation_id: endpoint.automation_id,
            revision_id: endpoint.revision_id,
            revision_digest: endpoint.revision_digest,
            signature_algorithm: endpoint.signature_algorithm.as_str().into(),
            signing_secret: endpoint.signing_secret.into(),
            request_schema_digest: endpoint.request_schema_digest,
            max_body_bytes: endpoint.max_body_bytes,
            generation: endpoint.generation,
            state: match endpoint.state {
                a3s_cloud_contracts::AutomationWebhookEndpointStateV1::Active => "active".into(),
                a3s_cloud_contracts::AutomationWebhookEndpointStateV1::Disabled => {
                    "disabled".into()
                }
                a3s_cloud_contracts::AutomationWebhookEndpointStateV1::Revoked => "revoked".into(),
            },
            created_at: endpoint.created_at,
            state_changed_at: endpoint.state_changed_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationDefinitionResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub automation_id: Uuid,
    pub name: String,
    pub trigger_kind: String,
    pub revision_id: Uuid,
    pub revision_number: u64,
    pub revision_digest: String,
    pub definition_acl: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<AutomationDefinitionRecord> for AutomationDefinitionResponse {
    fn from(record: AutomationDefinitionRecord) -> Self {
        let spec = record.definition.spec();
        let revision_spec = record.revision.spec();
        Self {
            organization_id: spec.organization_id,
            project_id: spec.project_id,
            environment_id: spec.environment_id,
            automation_id: spec.automation_id,
            name: spec.name.clone(),
            trigger_kind: match &spec.trigger {
                a3s_cloud_contracts::AutomationTriggerV1::Schedule(_) => "schedule".into(),
                a3s_cloud_contracts::AutomationTriggerV1::Webhook(_) => "webhook".into(),
                a3s_cloud_contracts::AutomationTriggerV1::PluginEvent(_) => "plugin_event".into(),
                a3s_cloud_contracts::AutomationTriggerV1::SourceEvent(_) => "source_event".into(),
            },
            revision_id: revision_spec.revision_id,
            revision_number: revision_spec.revision_number,
            revision_digest: record.revision.digest().to_owned(),
            definition_acl: record.definition.canonical_acl().to_owned(),
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRevisionResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub automation_id: Uuid,
    pub revision_id: Uuid,
    pub revision_number: u64,
    pub parent_revision_id: Option<Uuid>,
    pub parent_digest: Option<String>,
    pub revision_acl: String,
    pub revision_digest: String,
}

impl From<AutomationRevisionV1> for AutomationRevisionResponse {
    fn from(revision: AutomationRevisionV1) -> Self {
        let spec = revision.spec();
        let definition = &spec.definition;
        Self {
            organization_id: definition.organization_id,
            project_id: definition.project_id,
            environment_id: definition.environment_id,
            automation_id: definition.automation_id,
            revision_id: spec.revision_id,
            revision_number: spec.revision_number,
            parent_revision_id: spec.parent_revision_id,
            parent_digest: spec.parent_digest.clone(),
            revision_acl: revision.canonical_acl().to_owned(),
            revision_digest: revision.digest().to_owned(),
        }
    }
}
