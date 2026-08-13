use crate::modules::projects::domain::value_objects::ProjectName;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OrganizationId, ProjectAttributionProfileId, ProjectId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub organization_id: OrganizationId,
    pub id: ProjectId,
    pub name: ProjectName,
    pub aggregate_version: u64,
    #[serde(default)]
    pub current_attribution_profile_id: Option<ProjectAttributionProfileId>,
    pub created_at: DateTime<Utc>,
}

impl Project {
    pub fn create(
        organization_id: OrganizationId,
        id: ProjectId,
        name: ProjectName,
        created_at: DateTime<Utc>,
    ) -> Self {
        let created_at = canonical_timestamp(created_at);
        Self {
            organization_id,
            id,
            name,
            aggregate_version: 1,
            current_attribution_profile_id: None,
            created_at,
        }
    }

    pub fn point_to_attribution_profile(
        &self,
        expected_version: u64,
        profile_id: ProjectAttributionProfileId,
    ) -> Result<Self, String> {
        if self.aggregate_version != expected_version {
            return Err("project changed while updating its attribution profile".into());
        }
        if profile_id.as_uuid().is_nil() || self.current_attribution_profile_id == Some(profile_id)
        {
            return Err("project attribution profile reference is invalid".into());
        }
        let aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "project aggregate version overflowed".to_owned())?;
        let mut next = self.clone();
        next.aggregate_version = aggregate_version;
        next.current_attribution_profile_id = Some(profile_id);
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribution_pointer_uses_optimistic_concurrency_without_mutating_history() {
        let project = Project::create(
            OrganizationId::new(),
            ProjectId::new(),
            ProjectName::parse("platform").expect("name"),
            Utc::now(),
        );
        let profile_id = ProjectAttributionProfileId::new();
        let next = project
            .point_to_attribution_profile(1, profile_id)
            .expect("transition");

        assert_eq!(project.aggregate_version, 1);
        assert_eq!(project.current_attribution_profile_id, None);
        assert_eq!(next.aggregate_version, 2);
        assert_eq!(next.current_attribution_profile_id, Some(profile_id));
        assert!(project
            .point_to_attribution_profile(2, ProjectAttributionProfileId::new())
            .is_err());
    }
}
