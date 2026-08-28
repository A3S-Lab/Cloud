use crate::modules::projects::domain::entities::ProjectAttributionProfile;
use crate::modules::shared_kernel::domain::{
    OrganizationId, ProjectAttributionProfileId, ProjectId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAttributionProfileUpdated {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub attribution_profile_id: ProjectAttributionProfileId,
    pub previous_attribution_profile_id: Option<ProjectAttributionProfileId>,
    pub business_owner_reference: String,
    pub cost_attribution_code: Option<String>,
    pub labels: BTreeMap<String, String>,
}

impl ProjectAttributionProfileUpdated {
    pub fn envelope(
        profile: &ProjectAttributionProfile,
        project_aggregate_version: u64,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "project.attribution-profile.updated".into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: profile.organization_id.as_uuid(),
            },
            aggregate_id: profile.project_id.as_uuid(),
            aggregate_version: project_aggregate_version,
            occurred_at: profile.created_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: profile.organization_id,
                project_id: profile.project_id,
                attribution_profile_id: profile.id,
                previous_attribution_profile_id: profile.previous_profile_id,
                business_owner_reference: profile.business_owner_reference.as_str().to_owned(),
                cost_attribution_code: profile
                    .cost_attribution_code
                    .as_ref()
                    .map(|code| code.as_str().to_owned()),
                labels: profile.labels.as_map().clone(),
            })?,
        })
    }
}
