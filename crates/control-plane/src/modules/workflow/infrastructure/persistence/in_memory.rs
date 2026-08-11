use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OntologyId, OntologyRevisionId, OrganizationId, ProjectId, RepositoryError,
};
use crate::modules::workflow::domain::repositories::OntologyWriteReference;
use crate::modules::workflow::domain::{
    CreateOntologyWrite, IOntologyRepository, Ontology, OntologyRecord, OntologyRevision,
    ReviseOntologyWrite,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryOntologyRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    ontologies: BTreeMap<(OrganizationId, OntologyId), Ontology>,
    names: BTreeMap<(OrganizationId, ProjectId, String), OntologyId>,
    revisions: BTreeMap<(OrganizationId, OntologyId, OntologyRevisionId), OntologyRevision>,
    idempotency: BTreeMap<(String, String), (String, OntologyWriteReference)>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryOntologyRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IOntologyRepository for InMemoryOntologyRepository {
    async fn create(
        &self,
        write: CreateOntologyWrite,
    ) -> Result<IdempotentWrite<OntologyRecord>, RepositoryError> {
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
                "initial Ontology write contains a non-initial revision".into(),
            ));
        }
        let ontology = &write.record.ontology;
        let ontology_key = (ontology.organization_id, ontology.id);
        let name_key = (
            ontology.organization_id,
            ontology.project_id,
            ontology.name.key().to_owned(),
        );
        if state.ontologies.contains_key(&ontology_key) || state.names.contains_key(&name_key) {
            return Err(RepositoryError::Conflict(
                "Ontology name is already in use in this project".into(),
            ));
        }
        state.names.insert(name_key, ontology.id);
        state.ontologies.insert(ontology_key, ontology.clone());
        state.revisions.insert(
            (
                ontology.organization_id,
                ontology.id,
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
        write: ReviseOntologyWrite,
    ) -> Result<IdempotentWrite<OntologyRecord>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(record) = replay(&state, &write.idempotency)? {
            return Ok(IdempotentWrite {
                value: record,
                replayed: true,
            });
        }
        validate_record(&write.record)?;
        let ontology_key = (
            write.record.ontology.organization_id,
            write.record.ontology.id,
        );
        let current = state
            .ontologies
            .get(&ontology_key)
            .ok_or(RepositoryError::NotFound)?
            .clone();
        validate_successor(&current, &write)?;
        let old_name_key = (
            current.organization_id,
            current.project_id,
            current.name.key().to_owned(),
        );
        let new_name_key = (
            write.record.ontology.organization_id,
            write.record.ontology.project_id,
            write.record.ontology.name.key().to_owned(),
        );
        if new_name_key != old_name_key && state.names.contains_key(&new_name_key) {
            return Err(RepositoryError::Conflict(
                "Ontology name is already in use in this project".into(),
            ));
        }
        if new_name_key != old_name_key {
            state.names.remove(&old_name_key);
            state.names.insert(new_name_key, write.record.ontology.id);
        }
        state
            .ontologies
            .insert(ontology_key, write.record.ontology.clone());
        state.revisions.insert(
            (
                write.record.ontology.organization_id,
                write.record.ontology.id,
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
        ontology_id: OntologyId,
    ) -> Result<Option<Ontology>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .ontologies
            .get(&(organization_id, ontology_id))
            .cloned())
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<Ontology>, RepositoryError> {
        let mut ontologies = self
            .state
            .read()
            .await
            .ontologies
            .values()
            .filter(|ontology| {
                ontology.organization_id == organization_id && ontology.project_id == project_id
            })
            .cloned()
            .collect::<Vec<_>>();
        ontologies.sort_by(|left, right| {
            left.name
                .key()
                .cmp(right.name.key())
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(ontologies)
    }

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        ontology_id: OntologyId,
        revision_id: OntologyRevisionId,
    ) -> Result<Option<OntologyRevision>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .revisions
            .get(&(organization_id, ontology_id, revision_id))
            .cloned())
    }

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        ontology_id: OntologyId,
    ) -> Result<Vec<OntologyRevision>, RepositoryError> {
        let mut revisions = self
            .state
            .read()
            .await
            .revisions
            .values()
            .filter(|revision| {
                revision.organization_id == organization_id && revision.ontology_id == ontology_id
            })
            .cloned()
            .collect::<Vec<_>>();
        revisions.sort_by(|left, right| {
            right
                .revision_number
                .cmp(&left.revision_number)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(revisions)
    }
}

fn validate_record(record: &OntologyRecord) -> Result<(), RepositoryError> {
    record
        .ontology
        .validate()
        .and_then(|()| record.revision.validate())
        .map_err(RepositoryError::Storage)?;
    let revision = &record.revision;
    let ontology = &record.ontology;
    if revision.organization_id != ontology.organization_id
        || revision.project_id != ontology.project_id
        || revision.ontology_id != ontology.id
        || revision.id != ontology.current_revision_id
        || revision.revision_number != ontology.current_revision_number
        || revision.contract.digest() != &ontology.current_revision_digest
    {
        return Err(RepositoryError::Storage(
            "Ontology aggregate and current revision do not match".into(),
        ));
    }
    Ok(())
}

fn validate_successor(
    current: &Ontology,
    write: &ReviseOntologyWrite,
) -> Result<(), RepositoryError> {
    let next = &write.record.ontology;
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
            "Ontology was revised from a stale aggregate version".into(),
        ));
    }
    Ok(())
}

fn replay(
    state: &State,
    idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
) -> Result<Option<OntologyRecord>, RepositoryError> {
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
        .ontologies
        .get(&(reference.organization_id, reference.ontology_id))
        .cloned()
        .ok_or_else(|| RepositoryError::Storage("Ontology replay target is missing".into()))?;
    let revision = state
        .revisions
        .get(&(
            reference.organization_id,
            reference.ontology_id,
            reference.revision_id,
        ))
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Storage("Ontology revision replay target is missing".into())
        })?;
    let ontology = head.at_revision(&revision).map_err(|error| {
        RepositoryError::Storage(format!("Ontology replay target is invalid: {error}"))
    })?;
    Ok(Some(OntologyRecord { ontology, revision }))
}

fn store_replay(
    state: &mut State,
    idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
    record: &OntologyRecord,
) {
    state.idempotency.insert(
        (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        ),
        (
            idempotency.request_digest.clone(),
            OntologyWriteReference {
                organization_id: record.ontology.organization_id,
                ontology_id: record.ontology.id,
                revision_id: record.revision.id,
            },
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        IdempotencyRequest, OntologyRevisionId, PrincipalId, Sha256Digest,
    };
    use crate::modules::workflow::domain::{
        OntologyContract, OntologyName, OntologyObjectType, OntologyRevisionPublished, OntologySpec,
    };
    use chrono::Utc;
    use uuid::Uuid;

    fn digest(value: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", value.to_string().repeat(64))).expect("digest")
    }

    fn record() -> OntologyRecord {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let ontology_id = OntologyId::new();
        let revision_id = OntologyRevisionId::new();
        let actor = PrincipalId::new();
        let contract = OntologyContract::from_spec(OntologySpec {
            name: "Commerce".into(),
            description: String::new(),
            object_types: vec![OntologyObjectType {
                id: "customer".into(),
                label: "Customer".into(),
                schema_digest: digest('a'),
                key_fields: vec!["id".into()],
            }],
            relation_types: Vec::new(),
            rules: Vec::new(),
        })
        .expect("contract");
        let revision = OntologyRevision::initial(
            organization_id,
            project_id,
            ontology_id,
            revision_id,
            contract.clone(),
            actor,
            Utc::now(),
        );
        let ontology = Ontology::create(
            organization_id,
            project_id,
            ontology_id,
            OntologyName::parse("Commerce").expect("name"),
            String::new(),
            revision_id,
            contract.digest().clone(),
            actor,
            revision.created_at,
        )
        .expect("ontology");
        OntologyRecord { ontology, revision }
    }

    #[tokio::test]
    async fn create_is_idempotent_without_duplicating_outbox() {
        let repository = InMemoryOntologyRepository::new();
        let record = record();
        let event =
            OntologyRevisionPublished::created(&record.ontology, &record.revision, Uuid::new_v4())
                .expect("event");
        let idempotency =
            IdempotencyRequest::new("ontologies", "create", b"request").expect("idempotency");
        let write = CreateOntologyWrite {
            record: record.clone(),
            event,
            actor_principal_id: record.ontology.created_by,
            request_id: Uuid::new_v4(),
            idempotency,
        };
        assert!(
            !repository
                .create(write.clone())
                .await
                .expect("create")
                .replayed
        );
        assert!(repository.create(write).await.expect("replay").replayed);
        assert_eq!(repository.outbox_events().await.len(), 1);
    }
}
