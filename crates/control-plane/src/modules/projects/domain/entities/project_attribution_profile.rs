use crate::modules::projects::domain::value_objects::{
    BusinessOwnerReference, CostAttributionCode, ProjectAttributionLabels,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OrganizationId, PrincipalId, ProjectAttributionProfileId, ProjectId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectAttributionProfile {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub id: ProjectAttributionProfileId,
    pub previous_profile_id: Option<ProjectAttributionProfileId>,
    pub business_owner_reference: BusinessOwnerReference,
    pub cost_attribution_code: Option<CostAttributionCode>,
    pub labels: ProjectAttributionLabels,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
}

impl ProjectAttributionProfile {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        organization_id: OrganizationId,
        project_id: ProjectId,
        id: ProjectAttributionProfileId,
        previous_profile_id: Option<ProjectAttributionProfileId>,
        business_owner_reference: BusinessOwnerReference,
        cost_attribution_code: Option<CostAttributionCode>,
        labels: ProjectAttributionLabels,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let profile = Self {
            organization_id,
            project_id,
            id,
            previous_profile_id,
            business_owner_reference,
            cost_attribution_code,
            labels,
            created_by,
            created_at: canonical_timestamp(created_at),
        };
        profile.validate()?;
        Ok(profile)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.created_by.as_uuid().is_nil()
            || self.previous_profile_id == Some(self.id)
            || self.created_at != canonical_timestamp(self.created_at)
            || BusinessOwnerReference::parse(self.business_owner_reference.as_str())?
                != self.business_owner_reference
            || self
                .cost_attribution_code
                .as_ref()
                .map(|code| CostAttributionCode::parse(code.as_str()))
                .transpose()?
                != self.cost_attribution_code
            || ProjectAttributionLabels::parse(self.labels.as_map().clone())? != self.labels
        {
            return Err("project attribution profile is invalid".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn profiles_are_immutable_snapshots_with_explicit_lineage() {
        let first_id = ProjectAttributionProfileId::new();
        let second = ProjectAttributionProfile::create(
            OrganizationId::new(),
            ProjectId::new(),
            ProjectAttributionProfileId::new(),
            Some(first_id),
            BusinessOwnerReference::parse("finance/platform").expect("owner"),
            Some(CostAttributionCode::parse("CC-1042").expect("code")),
            ProjectAttributionLabels::parse(BTreeMap::from([(
                "service.tier".into(),
                "critical".into(),
            )]))
            .expect("labels"),
            PrincipalId::new(),
            Utc::now(),
        )
        .expect("profile");

        assert_eq!(second.previous_profile_id, Some(first_id));
        second.validate().expect("valid profile");
    }
}
