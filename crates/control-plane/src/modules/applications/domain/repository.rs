use super::{Application, ApplicationRelease, ApplicationReleasePublished};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationReleaseId, IdempotencyRequest, IdempotentWrite, OrganizationId,
    PrincipalId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationRecord {
    pub application: Application,
    pub release: ApplicationRelease,
}

impl ApplicationRecord {
    pub fn new(application: Application, release: ApplicationRelease) -> Result<Self, String> {
        let record = Self {
            application,
            release,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.application.validate()?;
        self.release.validate()?;
        if self.release.organization_id != self.application.organization_id
            || self.release.project_id != self.application.project_id
            || self.release.application_id != self.application.id
            || self.release.id != self.application.current_release_id
            || self.release.release_number != self.application.current_release_number
            || self.release.contract.digest() != &self.application.current_release_digest
            || self.release.contract.spec().experience != self.application.experience
            || self.release.created_at != self.application.updated_at
        {
            return Err("Application and current release do not match".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct CreateApplicationWrite {
    pub record: ApplicationRecord,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl CreateApplicationWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.record.validate()?;
        self.idempotency.validate()?;
        if self.record.application.aggregate_version != 1
            || self.record.release.release_number != 1
            || self.record.release.parent_release_id.is_some()
            || self.record.release.parent_digest.is_some()
            || self.actor_principal_id.as_uuid().is_nil()
            || self.actor_principal_id != self.record.application.created_by
            || self.actor_principal_id != self.record.release.created_by
            || self.request_id.is_nil()
        {
            return Err("initial Application write is invalid".into());
        }
        validate_event(&self.event, &self.record, self.request_id)
    }
}

#[derive(Debug, Clone)]
pub struct PublishApplicationReleaseWrite {
    pub record: ApplicationRecord,
    pub expected_version: u64,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl PublishApplicationReleaseWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.record.validate()?;
        self.idempotency.validate()?;
        if self.expected_version == 0
            || self.record.application.aggregate_version != self.expected_version.saturating_add(1)
            || self.actor_principal_id.as_uuid().is_nil()
            || self.actor_principal_id != self.record.release.created_by
            || self.request_id.is_nil()
        {
            return Err("Application release publication write is invalid".into());
        }
        validate_event(&self.event, &self.record, self.request_id)
    }

    pub fn validate_against(&self, current: &Application) -> Result<(), String> {
        self.validate()?;
        let expected = current.advance(self.expected_version, &self.record.release)?;
        if expected != self.record.application {
            return Err("Application release successor changed immutable state".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApplicationWriteReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub release_id: ApplicationReleaseId,
}

impl From<&ApplicationRecord> for ApplicationWriteReference {
    fn from(record: &ApplicationRecord) -> Self {
        Self {
            organization_id: record.application.organization_id,
            project_id: record.application.project_id,
            application_id: record.application.id,
            release_id: record.release.id,
        }
    }
}

#[async_trait]
pub trait IApplicationRepository: Send + Sync {
    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<ApplicationRecord>, RepositoryError>;

    async fn create(
        &self,
        write: CreateApplicationWrite,
    ) -> Result<IdempotentWrite<ApplicationRecord>, RepositoryError>;

    async fn publish_release(
        &self,
        write: PublishApplicationReleaseWrite,
    ) -> Result<IdempotentWrite<ApplicationRecord>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
    ) -> Result<Option<Application>, RepositoryError>;

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        limit: usize,
    ) -> Result<Vec<Application>, RepositoryError>;

    async fn find_release(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        release_id: ApplicationReleaseId,
    ) -> Result<Option<ApplicationRelease>, RepositoryError>;

    async fn list_releases(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        limit: usize,
    ) -> Result<Vec<ApplicationRelease>, RepositoryError>;
}

fn validate_event(
    event: &DomainEventEnvelope,
    record: &ApplicationRecord,
    request_id: Uuid,
) -> Result<(), String> {
    if event.event_id.is_nil()
        || event.event_key != "application.release.published"
        || event.schema_version != 1
        || event.organization_id != record.application.organization_id.as_uuid()
        || event.aggregate_id != record.application.id.as_uuid()
        || event.aggregate_version != record.application.aggregate_version
        || event.occurred_at != record.release.created_at
        || event.correlation_id != request_id
    {
        return Err("Application write and domain event are inconsistent".into());
    }
    let payload: ApplicationReleasePublished = serde_json::from_value(event.payload.clone())
        .map_err(|error| format!("Application release event is invalid: {error}"))?;
    let spec = record.release.contract.spec();
    let response_modes = spec
        .delivery
        .response_modes
        .iter()
        .map(|mode| mode.as_str().to_owned())
        .collect::<Vec<_>>();
    if payload.project_id != record.application.project_id.as_uuid()
        || payload.application_id != record.application.id.as_uuid()
        || payload.release_id != record.release.id.as_uuid()
        || payload.release_number != record.release.release_number
        || payload.parent_release_id
            != record
                .release
                .parent_release_id
                .map(|value| value.as_uuid())
        || payload.parent_digest.as_deref()
            != record
                .release
                .parent_digest
                .as_ref()
                .map(|value| value.as_str())
        || payload.experience != spec.experience.as_str()
        || payload.audience != spec.audience.as_str()
        || payload.interaction_mode != spec.delivery.interaction_mode.as_str()
        || payload.response_modes != response_modes
        || payload.contract_schema != super::APPLICATION_RELEASE_CONTRACT_SCHEMA
        || payload.contract_digest != record.release.contract.digest().as_str()
        || payload.workflow_definition_id != spec.workflow.workflow_definition_id.as_uuid()
        || payload.workflow_revision_id != spec.workflow.workflow_revision_id.as_uuid()
        || payload.workflow_contract_digest != spec.workflow.workflow_contract_digest.as_str()
        || payload.workflow_payload_set_digest != spec.workflow.workflow_payload_set_digest.as_str()
        || payload.workflow_semantic_contract_set_digest
            != spec.workflow.workflow_semantic_contract_set_digest.as_str()
        || payload.input_schema_digest != spec.workflow.input_schema_digest.as_str()
        || payload.output_schema_digest != spec.workflow.output_schema_digest.as_str()
        || payload.presentation_digest != spec.presentation_digest.as_str()
    {
        return Err("Application release event payload is inconsistent".into());
    }
    Ok(())
}
