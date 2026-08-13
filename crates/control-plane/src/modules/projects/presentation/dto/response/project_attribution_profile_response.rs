use crate::modules::projects::domain::entities::ProjectAttributionProfile;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAttributionProfileResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub id: Uuid,
    pub previous_profile_id: Option<Uuid>,
    pub business_owner_reference: String,
    pub cost_attribution_code: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

impl From<ProjectAttributionProfile> for ProjectAttributionProfileResponse {
    fn from(profile: ProjectAttributionProfile) -> Self {
        Self {
            organization_id: profile.organization_id.as_uuid(),
            project_id: profile.project_id.as_uuid(),
            id: profile.id.as_uuid(),
            previous_profile_id: profile.previous_profile_id.map(|id| id.as_uuid()),
            business_owner_reference: profile.business_owner_reference.as_str().to_owned(),
            cost_attribution_code: profile
                .cost_attribution_code
                .as_ref()
                .map(|code| code.as_str().to_owned()),
            labels: profile.labels.into_map(),
            created_by: profile.created_by.as_uuid(),
            created_at: profile.created_at,
        }
    }
}
