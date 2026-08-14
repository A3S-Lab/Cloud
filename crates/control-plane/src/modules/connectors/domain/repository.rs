use super::{
    ConnectorProfile, ConnectorRevision, ConnectorRevisionPublished, ConnectorSecretBinding,
};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, IdempotencyRequest, IdempotentWrite,
    OrganizationId, PrincipalId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorRecord {
    pub profile: ConnectorProfile,
    pub revision: ConnectorRevision,
}

impl ConnectorRecord {
    pub fn new(profile: ConnectorProfile, revision: ConnectorRevision) -> Result<Self, String> {
        let record = Self { profile, revision };
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.profile.validate()?;
        self.revision.validate()?;
        if self.revision.organization_id != self.profile.organization_id
            || self.revision.project_id != self.profile.project_id
            || self.revision.environment_id != self.profile.environment_id
            || self.revision.profile_id != self.profile.id
            || self.revision.id != self.profile.current_revision_id
            || self.revision.revision_number != self.profile.current_revision_number
            || self.revision.definition.digest() != &self.profile.current_revision_digest
            || self.revision.created_at != self.profile.updated_at
        {
            return Err("Connector profile and current revision do not match".into());
        }
        Ok(())
    }

    pub fn secret_bindings(&self) -> Vec<ConnectorSecretBinding> {
        self.revision.definition.secret_bindings()
    }
}

#[derive(Debug, Clone)]
pub struct CreateConnectorProfileWrite {
    pub record: ConnectorRecord,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl CreateConnectorProfileWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.record.validate()?;
        if self.record.profile.aggregate_version != 1
            || self.record.revision.revision_number != 1
            || self.record.revision.parent_revision_id.is_some()
            || self.record.revision.parent_digest.is_some()
            || self.actor_principal_id != self.record.profile.created_by
            || self.actor_principal_id != self.record.revision.created_by
        {
            return Err("initial Connector profile write is invalid".into());
        }
        validate_event(
            &self.event,
            &self.record,
            self.request_id,
            "connector.profile.created",
        )
    }
}

#[derive(Debug, Clone)]
pub struct ReviseConnectorProfileWrite {
    pub record: ConnectorRecord,
    pub expected_version: u64,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl ReviseConnectorProfileWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.record.validate()?;
        if self.expected_version == 0
            || self.record.profile.aggregate_version != self.expected_version.saturating_add(1)
            || self.actor_principal_id != self.record.revision.created_by
        {
            return Err("Connector profile revision write is invalid".into());
        }
        validate_event(
            &self.event,
            &self.record,
            self.request_id,
            "connector.profile.revised",
        )
    }

    pub fn validate_against(&self, current: &ConnectorProfile) -> Result<(), String> {
        self.validate()?;
        let expected = current.advance(self.expected_version, &self.record.revision)?;
        if expected != self.record.profile {
            return Err("Connector profile successor changed immutable state".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConnectorWriteReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub revision_id: ConnectorRevisionId,
}

impl From<&ConnectorRecord> for ConnectorWriteReference {
    fn from(record: &ConnectorRecord) -> Self {
        Self {
            organization_id: record.profile.organization_id,
            project_id: record.profile.project_id,
            environment_id: record.profile.environment_id,
            profile_id: record.profile.id,
            revision_id: record.revision.id,
        }
    }
}

#[async_trait]
pub trait IConnectorProfileRepository: Send + Sync {
    async fn create(
        &self,
        write: CreateConnectorProfileWrite,
    ) -> Result<IdempotentWrite<ConnectorRecord>, RepositoryError>;

    async fn revise(
        &self,
        write: ReviseConnectorProfileWrite,
    ) -> Result<IdempotentWrite<ConnectorRecord>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
    ) -> Result<Option<ConnectorProfile>, RepositoryError>;

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<ConnectorProfile>, RepositoryError>;

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
    ) -> Result<Option<ConnectorRevision>, RepositoryError>;

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
    ) -> Result<Vec<ConnectorRevision>, RepositoryError>;
}

fn validate_event(
    event: &DomainEventEnvelope,
    record: &ConnectorRecord,
    request_id: Uuid,
    event_key: &str,
) -> Result<(), String> {
    if event.event_key != event_key
        || event.schema_version != 1
        || event.organization_id != record.profile.organization_id.as_uuid()
        || event.aggregate_id != record.profile.id.as_uuid()
        || event.aggregate_version != record.profile.aggregate_version
        || event.occurred_at != record.revision.created_at
        || event.correlation_id != request_id
    {
        return Err("Connector profile write and domain event are inconsistent".into());
    }
    let payload: ConnectorRevisionPublished = serde_json::from_value(event.payload.clone())
        .map_err(|error| format!("Connector domain event is invalid: {error}"))?;
    if payload.project_id != record.profile.project_id.as_uuid()
        || payload.environment_id != record.profile.environment_id.as_uuid()
        || payload.profile_id != record.profile.id.as_uuid()
        || payload.revision_id != record.revision.id.as_uuid()
        || payload.revision_number != record.revision.revision_number
        || payload.parent_revision_id != record.revision.parent_revision_id.map(|id| id.as_uuid())
        || payload.definition_kind != record.revision.definition.kind()
        || payload.definition_schema != record.revision.definition.schema()
        || payload.definition_digest != record.revision.definition.digest().as_str()
        || payload.secret_binding_count != record.secret_bindings().len()
    {
        return Err("Connector domain event payload is inconsistent".into());
    }
    Ok(())
}
