use super::{
    DurableCellApplication, DurableCellApplicationChanged, DurableCellApplicationDesiredState,
    DurableCellApplicationRevision, DurableCellProjectionIdentity,
};
use crate::modules::shared_kernel::domain::{
    DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId, IdempotencyRequest,
    IdempotentWrite, OrganizationId, PrincipalId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableCellApplicationRecord {
    pub application: DurableCellApplication,
    pub revision: DurableCellApplicationRevision,
}

impl DurableCellApplicationRecord {
    pub fn new(
        application: DurableCellApplication,
        revision: DurableCellApplicationRevision,
    ) -> Result<Self, String> {
        let record = Self {
            application,
            revision,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.application.validate()?;
        self.revision.validate()?;
        DurableCellProjectionIdentity::for_current_revision(&self.application, &self.revision)?;
        Ok(())
    }

    pub(crate) fn replay_snapshot(
        head: &DurableCellApplication,
        revision: DurableCellApplicationRevision,
        desired_state: DurableCellApplicationDesiredState,
        aggregate_version: u64,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        head.validate()?;
        revision.validate()?;
        if revision.organization_id != head.organization_id
            || revision.project_id != head.project_id
            || revision.environment_id != head.environment_id
            || revision.application_id != head.id
        {
            return Err("Durable Cell replay revision is foreign".into());
        }
        let application = DurableCellApplication {
            organization_id: head.organization_id,
            project_id: head.project_id,
            environment_id: head.environment_id,
            id: head.id,
            name: head.name.clone(),
            desired_state,
            current_revision_id: revision.id,
            current_revision_number: revision.revision_number,
            current_definition_digest: revision.definition.digest().clone(),
            aggregate_version,
            created_by: head.created_by,
            created_at: head.created_at,
            updated_at,
        }
        .restore()?;
        Self::new(application, revision)
    }
}

#[derive(Debug, Clone)]
pub struct CreateDurableCellApplicationWrite {
    pub record: DurableCellApplicationRecord,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl CreateDurableCellApplicationWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.record.validate()?;
        if self.record.application.aggregate_version != 1
            || self.record.revision.revision_number != 1
            || self.record.revision.parent_revision_id.is_some()
            || self.record.revision.parent_definition_digest.is_some()
            || self.actor_principal_id.as_uuid().is_nil()
            || self.actor_principal_id != self.record.application.created_by
            || self.actor_principal_id != self.record.revision.created_by
            || self.request_id.is_nil()
        {
            return Err("initial Durable Cell application write is invalid".into());
        }
        validate_event(
            &self.event,
            &self.record,
            self.request_id,
            "durable-cell.application.created",
        )
    }
}

#[derive(Debug, Clone)]
pub struct ReviseDurableCellApplicationWrite {
    pub record: DurableCellApplicationRecord,
    pub expected_version: u64,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl ReviseDurableCellApplicationWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.record.validate()?;
        if self.expected_version == 0
            || self.record.application.aggregate_version != self.expected_version.saturating_add(1)
            || self.actor_principal_id.as_uuid().is_nil()
            || self.actor_principal_id != self.record.revision.created_by
            || self.request_id.is_nil()
        {
            return Err("Durable Cell application revision write is invalid".into());
        }
        validate_event(
            &self.event,
            &self.record,
            self.request_id,
            "durable-cell.application.revised",
        )
    }

    pub fn validate_against(&self, current: &DurableCellApplication) -> Result<(), String> {
        self.validate()?;
        let expected = current.advance(self.expected_version, &self.record.revision)?;
        if expected != self.record.application {
            return Err("Durable Cell application successor changed immutable state".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct RequestDurableCellApplicationStateWrite {
    pub record: DurableCellApplicationRecord,
    pub expected_version: u64,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl RequestDurableCellApplicationStateWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.record.validate()?;
        if self.expected_version == 0
            || self.record.application.aggregate_version != self.expected_version.saturating_add(1)
            || self.actor_principal_id.as_uuid().is_nil()
            || self.request_id.is_nil()
        {
            return Err("Durable Cell desired-state write is invalid".into());
        }
        validate_event(
            &self.event,
            &self.record,
            self.request_id,
            "durable-cell.application.state-requested",
        )
    }

    pub fn validate_against(&self, current: &DurableCellApplication) -> Result<(), String> {
        self.validate()?;
        if current.current_revision_id != self.record.revision.id
            || current.current_revision_number != self.record.revision.revision_number
            || &current.current_definition_digest != self.record.revision.definition.digest()
            || current.desired_state == self.record.application.desired_state
        {
            return Err("Durable Cell desired-state write changed revision authority".into());
        }
        let expected = current.request_state(
            self.expected_version,
            self.record.application.desired_state,
            self.record.application.updated_at,
        )?;
        if expected != self.record.application {
            return Err("Durable Cell desired-state write changed immutable state".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DurableCellWriteReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub application_id: DurableCellApplicationId,
    pub revision_id: DurableCellApplicationRevisionId,
    pub desired_state: DurableCellApplicationDesiredState,
    pub aggregate_version: u64,
    pub updated_at: DateTime<Utc>,
}

impl From<&DurableCellApplicationRecord> for DurableCellWriteReference {
    fn from(record: &DurableCellApplicationRecord) -> Self {
        Self {
            organization_id: record.application.organization_id,
            project_id: record.application.project_id,
            environment_id: record.application.environment_id,
            application_id: record.application.id,
            revision_id: record.revision.id,
            desired_state: record.application.desired_state,
            aggregate_version: record.application.aggregate_version,
            updated_at: record.application.updated_at,
        }
    }
}

#[async_trait]
pub trait IDurableCellApplicationRepository: Send + Sync {
    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<DurableCellApplicationRecord>, RepositoryError>;

    async fn create(
        &self,
        write: CreateDurableCellApplicationWrite,
    ) -> Result<IdempotentWrite<DurableCellApplicationRecord>, RepositoryError>;

    async fn revise(
        &self,
        write: ReviseDurableCellApplicationWrite,
    ) -> Result<IdempotentWrite<DurableCellApplicationRecord>, RepositoryError>;

    async fn request_state(
        &self,
        write: RequestDurableCellApplicationStateWrite,
    ) -> Result<IdempotentWrite<DurableCellApplicationRecord>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
    ) -> Result<Option<DurableCellApplication>, RepositoryError>;

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        limit: usize,
    ) -> Result<Vec<DurableCellApplication>, RepositoryError>;

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
        revision_id: DurableCellApplicationRevisionId,
    ) -> Result<Option<DurableCellApplicationRevision>, RepositoryError>;

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
        limit: usize,
    ) -> Result<Vec<DurableCellApplicationRevision>, RepositoryError>;
}

fn validate_event(
    event: &DomainEventEnvelope,
    record: &DurableCellApplicationRecord,
    request_id: Uuid,
    event_key: &str,
) -> Result<(), String> {
    let application = &record.application;
    let revision = &record.revision;
    if event.event_key != event_key
        || event.schema_version != 1
        || event.organization_id != application.organization_id.as_uuid()
        || event.aggregate_id != application.id.as_uuid()
        || event.aggregate_version != application.aggregate_version
        || event.occurred_at != application.updated_at
        || event.correlation_id != request_id
    {
        return Err("Durable Cell write and domain event are inconsistent".into());
    }
    let payload: DurableCellApplicationChanged = serde_json::from_value(event.payload.clone())
        .map_err(|error| format!("Durable Cell domain event is invalid: {error}"))?;
    let projection = DurableCellProjectionIdentity::for_current_revision(application, revision)?;
    if payload.project_id != application.project_id.as_uuid()
        || payload.environment_id != application.environment_id.as_uuid()
        || payload.application_id != application.id.as_uuid()
        || payload.revision_id != revision.id.as_uuid()
        || payload.revision_number != revision.revision_number
        || payload.definition_digest != revision.definition.digest().as_str()
        || payload.desired_state != application.desired_state.as_str()
        || payload.storage_namespace_id != projection.storage_namespace_id.as_uuid()
        || payload.workload_id != projection.workload_id.as_uuid()
        || payload.workload_revision_id != projection.workload_revision_id.as_uuid()
        || payload.deployment_id != projection.deployment_id.as_uuid()
        || payload.operation_id != projection.operation_id.as_uuid()
    {
        return Err("Durable Cell domain event payload is inconsistent".into());
    }
    Ok(())
}
