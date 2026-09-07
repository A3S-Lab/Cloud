use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OperationId, OrganizationId, PluginAssignmentId, PluginPlanProjectionId,
    Sha256Digest,
};
use a3s_use_core::{
    PlanPackageRole, PlanPolicyDecision, PluginOperationAction, PluginOperationConfirmation,
    PluginOperationPlanEnvelope, PLUGIN_OPERATION_PLAN_SCHEMA_V4,
};
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPluginPlanProjection {
    pub organization_id: OrganizationId,
    pub id: PluginPlanProjectionId,
    pub assignment_id: PluginAssignmentId,
    pub operation_id: OperationId,
    pub assignment_generation: u64,
    pub envelope: PluginOperationPlanEnvelope,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginPlanProjection {
    pub organization_id: OrganizationId,
    pub id: PluginPlanProjectionId,
    pub assignment_id: PluginAssignmentId,
    pub operation_id: OperationId,
    pub assignment_generation: u64,
    pub use_operation_id: String,
    pub plan_schema: String,
    pub plan_digest: Sha256Digest,
    pub expires_at: DateTime<Utc>,
    pub action: PluginOperationAction,
    pub root_package_id: String,
    pub root_package_digest: Option<Sha256Digest>,
    pub root_manifest_digest: Option<Sha256Digest>,
    pub authority_decision: PlanPolicyDecision,
    pub authority_policy_digest: Sha256Digest,
    pub impact_digest: Sha256Digest,
    pub permission_evidence_digest: Sha256Digest,
    pub provider_evidence_digest: Sha256Digest,
    pub confirmation_digest: Option<Sha256Digest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<PluginOperationConfirmation>,
    pub terminal_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PluginPlanProjection {
    pub fn from_validated_envelope(input: NewPluginPlanProjection) -> Result<Self, String> {
        input.envelope.validate().map_err(|error| error.to_string())?;
        let plan = &input.envelope.plan;
        if plan.schema != PLUGIN_OPERATION_PLAN_SCHEMA_V4 {
            return Err("plugin plan projection schema is unsupported".into());
        }
        let root = plan
            .packages
            .iter()
            .find(|package| package.role == PlanPackageRole::Root)
            .ok_or_else(|| "plugin plan projection is missing the root package transition".to_owned())?;
        let root_package_digest = root
            .after
            .as_ref()
            .or(root.before.as_ref())
            .map(|state| Sha256Digest::parse(state.release.package_sha256.clone()))
            .transpose()
            .map_err(|error| error.to_string())?;
        let root_manifest_digest = root
            .after
            .as_ref()
            .or(root.before.as_ref())
            .map(|state| Sha256Digest::parse(state.release.manifest_sha256.clone()))
            .transpose()
            .map_err(|error| error.to_string())?;
        let created_at = canonical_timestamp(input.created_at);
        let expires_at = millis_to_utc(plan.expires_at_ms)?;
        let projection = Self {
            organization_id: input.organization_id,
            id: input.id,
            assignment_id: input.assignment_id,
            operation_id: input.operation_id,
            assignment_generation: input.assignment_generation,
            use_operation_id: plan.operation_id.clone(),
            plan_schema: plan.schema.clone(),
            plan_digest: Sha256Digest::parse(input.envelope.plan_digest.clone())
                .map_err(|error| error.to_string())?,
            expires_at,
            action: plan.action,
            root_package_id: root.package_id.clone(),
            root_package_digest,
            root_manifest_digest,
            authority_decision: plan.authority.decision,
            authority_policy_digest: Sha256Digest::parse(plan.authority.policy_digest.clone())
                .map_err(|error| error.to_string())?,
            impact_digest: digest_json(&plan.impact)?,
            permission_evidence_digest: digest_json(
                &plan
                    .packages
                    .iter()
                    .filter_map(|package| {
                        package
                            .after
                            .as_ref()
                            .or(package.before.as_ref())
                            .map(|state| state.release.permission_ceiling_digest.clone())
                    })
                    .collect::<Vec<_>>(),
            )?,
            provider_evidence_digest: digest_json(&plan.providers)?,
            confirmation_digest: None,
            confirmation: None,
            terminal_reason: None,
            created_at,
            updated_at: created_at,
        };
        projection.validate()?;
        Ok(projection)
    }

    pub fn confirm(
        &self,
        confirmation: &PluginOperationConfirmation,
        confirmed_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        self.validate()?;
        if self.authority_decision != PlanPolicyDecision::Ask {
            return Err("plugin plan projection does not require confirmation".into());
        }
        if self.confirmation_digest.is_some() {
            return Err("plugin plan projection is already confirmed".into());
        }
        if self.terminal_reason.is_some() {
            return Err("plugin plan projection is already terminal".into());
        }
        if confirmed_at > self.expires_at {
            return Err("plugin plan projection has expired".into());
        }
        let now_ms: u64 = confirmed_at
            .timestamp_millis()
            .try_into()
            .map_err(|_| "plugin plan confirmation time is invalid".to_owned())?;
        // Reconstruct a minimal envelope validate path via confirmation fields only.
        confirmation.validate().map_err(|error| error.to_string())?;
        if confirmation.operation_id != self.use_operation_id
            || confirmation.plan_digest != self.plan_digest.as_str()
        {
            return Err("plugin plan confirmation does not match the projection".into());
        }
        if confirmation.confirmed_at_ms == 0 || confirmation.confirmed_at_ms > now_ms + 60_000 {
            return Err("plugin plan confirmation time is invalid".into());
        }
        let mut next = self.clone();
        next.confirmation_digest = Some(
            Sha256Digest::parse(confirmation.descriptor_digest().map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?,
        );
        next.confirmation = Some(confirmation.clone());
        next.updated_at = canonical_timestamp(confirmed_at.max(self.updated_at));
        next.validate()?;
        Ok(next)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.assignment_generation == 0
            || self.use_operation_id.trim().is_empty()
            || self.use_operation_id.len() > 256
            || self.plan_schema != PLUGIN_OPERATION_PLAN_SCHEMA_V4
            || self.root_package_id.trim().is_empty()
            || self.root_package_id.len() > 127
            || self.updated_at < self.created_at
            || self
                .terminal_reason
                .as_ref()
                .is_some_and(|reason| reason.trim().is_empty() || reason.len() > 512)
        {
            return Err("plugin plan projection identity or bounds are invalid".into());
        }
        match (&self.confirmation_digest, &self.confirmation) {
            (None, None) => Ok(()),
            (Some(digest), Some(confirmation)) => {
                confirmation
                    .validate()
                    .map_err(|error| error.to_string())?;
                if confirmation.operation_id != self.use_operation_id
                    || confirmation.plan_digest != self.plan_digest.as_str()
                {
                    return Err("plugin plan confirmation does not match the projection".into());
                }
                let confirmation_digest = Sha256Digest::parse(
                    confirmation
                        .descriptor_digest()
                        .map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                if &confirmation_digest != digest {
                    return Err("plugin plan confirmation digest drifted".into());
                }
                Ok(())
            }
            _ => Err("plugin plan confirmation evidence is incomplete".into()),
        }
    }

    pub fn awaits_confirmation(&self) -> bool {
        self.authority_decision == PlanPolicyDecision::Ask
            && self.confirmation_digest.is_none()
            && self.terminal_reason.is_none()
    }
}

fn millis_to_utc(millis: u64) -> Result<DateTime<Utc>, String> {
    let millis = i64::try_from(millis).map_err(|_| "plugin plan expiry overflowed".to_owned())?;
    Utc.timestamp_millis_opt(millis)
        .single()
        .ok_or_else(|| "plugin plan expiry is invalid".into())
}

fn digest_json<T: Serialize>(value: &T) -> Result<Sha256Digest, String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    let digest = Sha256::digest(bytes);
    Sha256Digest::parse(format!("sha256:{digest:x}")).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_use_core::PluginOperationPlan;
    use uuid::Uuid;

    const INSTALL_PLAN: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/plugins/operation-plan-install-v4.json"
    ));
    const CONFIRMATION: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/plugins/operation-confirmation-v1.json"
    ));

    #[test]
    fn projects_bounded_review_fields_from_a_validated_ask_plan() {
        let plan = PluginOperationPlan::from_json(INSTALL_PLAN).expect("plan");
        let envelope = PluginOperationPlanEnvelope::new(plan).expect("envelope");
        let projection = PluginPlanProjection::from_validated_envelope(NewPluginPlanProjection {
            organization_id: OrganizationId::new(),
            id: PluginPlanProjectionId::new(),
            assignment_id: PluginAssignmentId::new(),
            operation_id: OperationId::from_uuid(Uuid::now_v7()),
            assignment_generation: 1,
            envelope: envelope.clone(),
            created_at: Utc::now(),
        })
        .expect("projection");
        assert_eq!(projection.plan_digest.as_str(), envelope.plan_digest);
        assert_eq!(projection.action, PluginOperationAction::Install);
        assert_eq!(projection.authority_decision, PlanPolicyDecision::Ask);
        assert_eq!(projection.root_package_id, "acme/research");
        assert!(projection.awaits_confirmation());
    }

    #[test]
    fn confirms_an_ask_projection_with_matching_confirmation() {
        use a3s_use_core::PluginOperationConfirmation;
        use chrono::TimeZone;

        let plan = PluginOperationPlan::from_json(INSTALL_PLAN).expect("plan");
        let envelope = PluginOperationPlanEnvelope::new(plan).expect("envelope");
        let confirmation =
            PluginOperationConfirmation::from_json(CONFIRMATION).expect("confirmation");
        let created_at = Utc.timestamp_millis_opt(1_785_360_000_000).unwrap();
        let confirmed_at = Utc.timestamp_millis_opt(1_785_360_200_000).unwrap();
        let projection = PluginPlanProjection::from_validated_envelope(NewPluginPlanProjection {
            organization_id: OrganizationId::new(),
            id: PluginPlanProjectionId::new(),
            assignment_id: PluginAssignmentId::new(),
            operation_id: OperationId::from_uuid(Uuid::now_v7()),
            assignment_generation: 1,
            envelope,
            created_at,
        })
        .expect("projection");
        let confirmed = projection.confirm(&confirmation, confirmed_at).expect("confirm");
        assert!(confirmed.confirmation_digest.is_some());
        assert!(!confirmed.awaits_confirmation());
    }
}
