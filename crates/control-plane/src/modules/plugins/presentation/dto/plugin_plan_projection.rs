use crate::modules::plugins::domain::entities::PluginPlanProjection;
use a3s_use_core::{PlanPolicyDecision, PluginOperationAction, PluginOperationConfirmation};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginPlanProjectionResponse {
    pub organization_id: Uuid,
    pub id: Uuid,
    pub assignment_id: Uuid,
    pub operation_id: Uuid,
    pub assignment_generation: u64,
    pub use_operation_id: String,
    pub plan_schema: String,
    pub plan_digest: String,
    pub expires_at: DateTime<Utc>,
    pub action: PluginOperationAction,
    pub root_package_id: String,
    pub root_package_digest: Option<String>,
    pub root_manifest_digest: Option<String>,
    pub authority_decision: PlanPolicyDecision,
    pub authority_policy_digest: String,
    pub impact_digest: String,
    pub permission_evidence_digest: String,
    pub provider_evidence_digest: String,
    pub confirmation_digest: Option<String>,
    pub terminal_reason: Option<String>,
    pub awaits_confirmation: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<PluginPlanProjection> for PluginPlanProjectionResponse {
    fn from(value: PluginPlanProjection) -> Self {
        Self {
            organization_id: value.organization_id.as_uuid(),
            id: value.id.as_uuid(),
            assignment_id: value.assignment_id.as_uuid(),
            operation_id: value.operation_id.as_uuid(),
            assignment_generation: value.assignment_generation,
            use_operation_id: value.use_operation_id.clone(),
            plan_schema: value.plan_schema.clone(),
            plan_digest: value.plan_digest.as_str().to_owned(),
            expires_at: value.expires_at,
            action: value.action,
            root_package_id: value.root_package_id.clone(),
            root_package_digest: value
                .root_package_digest
                .as_ref()
                .map(|digest| digest.as_str().to_owned()),
            root_manifest_digest: value
                .root_manifest_digest
                .as_ref()
                .map(|digest| digest.as_str().to_owned()),
            authority_decision: value.authority_decision,
            authority_policy_digest: value.authority_policy_digest.as_str().to_owned(),
            impact_digest: value.impact_digest.as_str().to_owned(),
            permission_evidence_digest: value.permission_evidence_digest.as_str().to_owned(),
            provider_evidence_digest: value.provider_evidence_digest.as_str().to_owned(),
            confirmation_digest: value
                .confirmation_digest
                .as_ref()
                .map(|digest| digest.as_str().to_owned()),
            terminal_reason: value.terminal_reason.clone(),
            awaits_confirmation: value.awaits_confirmation(),
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfirmPluginPlanProjectionRequest {
    pub confirmation: PluginOperationConfirmation,
}
