use crate::modules::projects::domain::entities::{Project, ProjectAttributionProfile};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectAttributionProfileId, ProjectId,
    RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectAttributionRecord {
    pub project: Project,
    pub attribution_profile: ProjectAttributionProfile,
}

#[derive(Debug, Clone)]
pub struct UpdateProjectAttributionWrite {
    pub record: ProjectAttributionRecord,
    pub expected_project_version: u64,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
    pub request_id: Uuid,
}

impl UpdateProjectAttributionWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.record.attribution_profile.validate()?;
        self.idempotency.validate()?;
        let project = &self.record.project;
        let profile = &self.record.attribution_profile;
        if self.request_id.is_nil()
            || project.organization_id != profile.organization_id
            || project.id != profile.project_id
            || project.current_attribution_profile_id != Some(profile.id)
            || self.expected_project_version.checked_add(1) != Some(project.aggregate_version)
            || self.event.event_key != "project.attribution-profile.updated"
            || self.event.schema_version != 1
            || self.event.organization_id() != Some(project.organization_id.as_uuid())
            || self.event.aggregate_id != project.id.as_uuid()
            || self.event.aggregate_version != project.aggregate_version
            || self.event.occurred_at != profile.created_at
            || self.event.correlation_id != self.request_id
        {
            return Err("project attribution update is inconsistent".into());
        }
        Ok(())
    }

    pub fn validate_against(&self, existing: &Project) -> Result<(), String> {
        self.validate()?;
        let next = &self.record.project;
        let profile = &self.record.attribution_profile;
        if existing.organization_id != next.organization_id
            || existing.id != next.id
            || existing.name != next.name
            || existing.created_at != next.created_at
            || existing.aggregate_version != self.expected_project_version
            || existing.current_attribution_profile_id != profile.previous_profile_id
        {
            return Err("project changed while updating its attribution profile".into());
        }
        let expected =
            existing.point_to_attribution_profile(self.expected_project_version, profile.id)?;
        if expected != *next {
            return Err("project attribution transition is invalid".into());
        }
        Ok(())
    }
}

#[async_trait]
pub trait IProjectRepository: Send + Sync {
    async fn create(
        &self,
        project: Project,
        event: DomainEventEnvelope,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<Project>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Option<Project>, RepositoryError>;

    async fn list(&self, organization_id: OrganizationId) -> Result<Vec<Project>, RepositoryError>;

    async fn replay_attribution_update(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<ProjectAttributionRecord>>, RepositoryError>;

    async fn update_attribution(
        &self,
        write: UpdateProjectAttributionWrite,
    ) -> Result<IdempotentWrite<ProjectAttributionRecord>, RepositoryError>;

    async fn find_attribution_profile(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        attribution_profile_id: ProjectAttributionProfileId,
    ) -> Result<Option<ProjectAttributionProfile>, RepositoryError>;
}
