use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OrganizationId, ProjectId, RepositoryError, WorkflowDefinitionId,
    WorkflowRevisionId,
};
use crate::modules::workflow::domain::repositories::WorkflowDefinitionWriteReference;
use crate::modules::workflow::domain::{
    CreateWorkflowDefinitionWrite, IWorkflowDefinitionRepository, ReviseWorkflowDefinitionWrite,
    WorkflowDefinition, WorkflowDefinitionRecord, WorkflowRevision,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryWorkflowDefinitionRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    definitions: BTreeMap<(OrganizationId, WorkflowDefinitionId), WorkflowDefinition>,
    names: BTreeMap<(OrganizationId, ProjectId, String), WorkflowDefinitionId>,
    revisions:
        BTreeMap<(OrganizationId, WorkflowDefinitionId, WorkflowRevisionId), WorkflowRevision>,
    idempotency: BTreeMap<(String, String), (String, WorkflowDefinitionWriteReference)>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryWorkflowDefinitionRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IWorkflowDefinitionRepository for InMemoryWorkflowDefinitionRepository {
    async fn create(
        &self,
        write: CreateWorkflowDefinitionWrite,
    ) -> Result<IdempotentWrite<WorkflowDefinitionRecord>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(record) = replay(&state, &write.idempotency)? {
            return Ok(IdempotentWrite {
                value: record,
                replayed: true,
            });
        }
        validate_record(&write.record)?;
        if write.record.revision.revision_number != 1
            || write.record.revision.parent_revision_id.is_some()
        {
            return Err(RepositoryError::Storage(
                "initial WorkflowDefinition write contains a non-initial revision".into(),
            ));
        }
        let definition = &write.record.definition;
        let key = (definition.organization_id, definition.id);
        let name_key = (
            definition.organization_id,
            definition.project_id,
            workflow_name_key(&definition.name),
        );
        if state.definitions.contains_key(&key) || state.names.contains_key(&name_key) {
            return Err(RepositoryError::Conflict(
                "Workflow name is already in use in this project".into(),
            ));
        }
        state.names.insert(name_key, definition.id);
        state.definitions.insert(key, definition.clone());
        state.revisions.insert(
            (
                definition.organization_id,
                definition.id,
                write.record.revision.id,
            ),
            write.record.revision.clone(),
        );
        store_replay(&mut state, &write.idempotency, &write.record);
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.record,
            replayed: false,
        })
    }

    async fn revise(
        &self,
        write: ReviseWorkflowDefinitionWrite,
    ) -> Result<IdempotentWrite<WorkflowDefinitionRecord>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(record) = replay(&state, &write.idempotency)? {
            return Ok(IdempotentWrite {
                value: record,
                replayed: true,
            });
        }
        validate_record(&write.record)?;
        let key = (
            write.record.definition.organization_id,
            write.record.definition.id,
        );
        let current = state
            .definitions
            .get(&key)
            .ok_or(RepositoryError::NotFound)?
            .clone();
        validate_successor(&current, &write)?;
        let old_name_key = (
            current.organization_id,
            current.project_id,
            workflow_name_key(&current.name),
        );
        let new_name_key = (
            write.record.definition.organization_id,
            write.record.definition.project_id,
            workflow_name_key(&write.record.definition.name),
        );
        if new_name_key != old_name_key && state.names.contains_key(&new_name_key) {
            return Err(RepositoryError::Conflict(
                "Workflow name is already in use in this project".into(),
            ));
        }
        if new_name_key != old_name_key {
            state.names.remove(&old_name_key);
            state.names.insert(new_name_key, write.record.definition.id);
        }
        state
            .definitions
            .insert(key, write.record.definition.clone());
        state.revisions.insert(
            (
                write.record.definition.organization_id,
                write.record.definition.id,
                write.record.revision.id,
            ),
            write.record.revision.clone(),
        );
        store_replay(&mut state, &write.idempotency, &write.record);
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.record,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        definition_id: WorkflowDefinitionId,
    ) -> Result<Option<WorkflowDefinition>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .definitions
            .get(&(organization_id, definition_id))
            .cloned())
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<WorkflowDefinition>, RepositoryError> {
        let mut values = self
            .state
            .read()
            .await
            .definitions
            .values()
            .filter(|value| {
                value.organization_id == organization_id && value.project_id == project_id
            })
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|left, right| {
            workflow_name_key(&left.name)
                .cmp(&workflow_name_key(&right.name))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(values)
    }

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        definition_id: WorkflowDefinitionId,
        revision_id: WorkflowRevisionId,
    ) -> Result<Option<WorkflowRevision>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .revisions
            .get(&(organization_id, definition_id, revision_id))
            .cloned())
    }

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        definition_id: WorkflowDefinitionId,
    ) -> Result<Vec<WorkflowRevision>, RepositoryError> {
        let mut values = self
            .state
            .read()
            .await
            .revisions
            .values()
            .filter(|value| {
                value.organization_id == organization_id
                    && value.workflow_definition_id == definition_id
            })
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|left, right| {
            right
                .revision_number
                .cmp(&left.revision_number)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(values)
    }
}

fn validate_record(record: &WorkflowDefinitionRecord) -> Result<(), RepositoryError> {
    record
        .definition
        .validate()
        .and_then(|()| record.revision.validate())
        .map_err(RepositoryError::Storage)?;
    let definition = &record.definition;
    let revision = &record.revision;
    if revision.organization_id != definition.organization_id
        || revision.project_id != definition.project_id
        || revision.workflow_definition_id != definition.id
        || revision.id != definition.current_revision_id
        || revision.revision_number != definition.current_revision_number
        || revision.contract.digest() != &definition.current_revision_digest
        || revision.contract.spec().name != definition.name
        || revision.contract.spec().description != definition.description
    {
        return Err(RepositoryError::Storage(
            "WorkflowDefinition aggregate and current revision do not match".into(),
        ));
    }
    Ok(())
}

fn validate_successor(
    current: &WorkflowDefinition,
    write: &ReviseWorkflowDefinitionWrite,
) -> Result<(), RepositoryError> {
    let next = &write.record.definition;
    let revision = &write.record.revision;
    if current.aggregate_version != write.expected_version
        || next.aggregate_version != write.expected_version.saturating_add(1)
        || next.organization_id != current.organization_id
        || next.project_id != current.project_id
        || next.id != current.id
        || next.created_by != current.created_by
        || next.created_at != current.created_at
        || revision.parent_revision_id != Some(current.current_revision_id)
        || revision.parent_digest.as_ref() != Some(&current.current_revision_digest)
    {
        return Err(RepositoryError::Conflict(
            "WorkflowDefinition was revised from a stale aggregate version".into(),
        ));
    }
    Ok(())
}

fn replay(
    state: &State,
    idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
) -> Result<Option<WorkflowDefinitionRecord>, RepositoryError> {
    let key = (
        idempotency.storage_key().0.to_owned(),
        idempotency.storage_key().1.to_owned(),
    );
    let Some((digest, reference)) = state.idempotency.get(&key) else {
        return Ok(None);
    };
    if digest != &idempotency.request_digest {
        return Err(RepositoryError::IdempotencyConflict);
    }
    let head = state
        .definitions
        .get(&(reference.organization_id, reference.workflow_definition_id))
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Storage("WorkflowDefinition replay target is missing".into())
        })?;
    let revision = state
        .revisions
        .get(&(
            reference.organization_id,
            reference.workflow_definition_id,
            reference.workflow_revision_id,
        ))
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Storage("Workflow revision replay target is missing".into())
        })?;
    let definition = head.at_revision(&revision).map_err(|error| {
        RepositoryError::Storage(format!(
            "WorkflowDefinition replay target is invalid: {error}"
        ))
    })?;
    Ok(Some(WorkflowDefinitionRecord {
        definition,
        revision,
    }))
}

fn store_replay(
    state: &mut State,
    idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
    record: &WorkflowDefinitionRecord,
) {
    state.idempotency.insert(
        (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        ),
        (
            idempotency.request_digest.clone(),
            WorkflowDefinitionWriteReference {
                organization_id: record.definition.organization_id,
                workflow_definition_id: record.definition.id,
                workflow_revision_id: record.revision.id,
            },
        ),
    );
}

fn workflow_name_key(value: &str) -> String {
    value.trim().to_lowercase()
}
