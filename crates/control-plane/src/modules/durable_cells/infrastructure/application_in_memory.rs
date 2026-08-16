use crate::modules::durable_cells::domain::{
    CreateDurableCellApplicationWrite, DurableCellApplication, DurableCellApplicationRecord,
    DurableCellApplicationRevision, DurableCellWriteReference, IDurableCellApplicationRepository,
    RequestDurableCellApplicationStateWrite, ReviseDurableCellApplicationWrite,
};
use crate::modules::shared_kernel::domain::{
    DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId, IdempotencyRequest,
    IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryDurableCellApplicationRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    applications: BTreeMap<(OrganizationId, DurableCellApplicationId), DurableCellApplication>,
    names: BTreeMap<(OrganizationId, ProjectId, EnvironmentId, String), DurableCellApplicationId>,
    revisions: BTreeMap<
        (
            OrganizationId,
            DurableCellApplicationId,
            DurableCellApplicationRevisionId,
        ),
        DurableCellApplicationRevision,
    >,
    idempotency: BTreeMap<(String, String), (String, DurableCellWriteReference)>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryDurableCellApplicationRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IDurableCellApplicationRepository for InMemoryDurableCellApplicationRepository {
    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<DurableCellApplicationRecord>, RepositoryError> {
        let state = self.state.read().await;
        replay(&state, idempotency)
    }

    async fn create(
        &self,
        write: CreateDurableCellApplicationWrite,
    ) -> Result<IdempotentWrite<DurableCellApplicationRecord>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(record) = replay(&state, &write.idempotency)? {
            return Ok(IdempotentWrite {
                value: record,
                replayed: true,
            });
        }
        write.validate().map_err(RepositoryError::Storage)?;
        let application = &write.record.application;
        let application_key = (application.organization_id, application.id);
        let name_key = (
            application.organization_id,
            application.project_id,
            application.environment_id,
            application.name.key().to_owned(),
        );
        if state.applications.contains_key(&application_key) || state.names.contains_key(&name_key)
        {
            return Err(RepositoryError::Conflict(
                "Durable Cell application name or identity is already in use".into(),
            ));
        }
        let revision_key = revision_key(&write.record.revision);
        if state.revisions.contains_key(&revision_key) {
            return Err(RepositoryError::Conflict(
                "Durable Cell revision identity is already in use".into(),
            ));
        }
        state.names.insert(name_key, application.id);
        state
            .applications
            .insert(application_key, application.clone());
        state
            .revisions
            .insert(revision_key, write.record.revision.clone());
        store_replay(&mut state, &write.idempotency, &write.record);
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.record,
            replayed: false,
        })
    }

    async fn revise(
        &self,
        write: ReviseDurableCellApplicationWrite,
    ) -> Result<IdempotentWrite<DurableCellApplicationRecord>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(record) = replay(&state, &write.idempotency)? {
            return Ok(IdempotentWrite {
                value: record,
                replayed: true,
            });
        }
        write.validate().map_err(RepositoryError::Storage)?;
        let application_key = (
            write.record.application.organization_id,
            write.record.application.id,
        );
        let current = state
            .applications
            .get(&application_key)
            .ok_or(RepositoryError::NotFound)?
            .clone();
        write
            .validate_against(&current)
            .map_err(RepositoryError::Conflict)?;
        let revision_key = revision_key(&write.record.revision);
        if state.revisions.contains_key(&revision_key) {
            return Err(RepositoryError::Conflict(
                "Durable Cell revision identity is already in use".into(),
            ));
        }
        state
            .applications
            .insert(application_key, write.record.application.clone());
        state
            .revisions
            .insert(revision_key, write.record.revision.clone());
        store_replay(&mut state, &write.idempotency, &write.record);
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.record,
            replayed: false,
        })
    }

    async fn request_state(
        &self,
        write: RequestDurableCellApplicationStateWrite,
    ) -> Result<IdempotentWrite<DurableCellApplicationRecord>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(record) = replay(&state, &write.idempotency)? {
            return Ok(IdempotentWrite {
                value: record,
                replayed: true,
            });
        }
        write.validate().map_err(RepositoryError::Storage)?;
        let application_key = (
            write.record.application.organization_id,
            write.record.application.id,
        );
        let current = state
            .applications
            .get(&application_key)
            .ok_or(RepositoryError::NotFound)?
            .clone();
        write
            .validate_against(&current)
            .map_err(RepositoryError::Conflict)?;
        let stored_revision = state
            .revisions
            .get(&revision_key(&write.record.revision))
            .ok_or_else(|| {
                RepositoryError::Storage("Durable Cell desired-state revision is missing".into())
            })?;
        if stored_revision != &write.record.revision {
            return Err(RepositoryError::Storage(
                "Durable Cell desired-state revision drifted".into(),
            ));
        }
        state
            .applications
            .insert(application_key, write.record.application.clone());
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
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
    ) -> Result<Option<DurableCellApplication>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .applications
            .get(&(organization_id, application_id))
            .filter(|application| {
                application.project_id == project_id && application.environment_id == environment_id
            })
            .cloned())
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        limit: usize,
    ) -> Result<Vec<DurableCellApplication>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut applications = self
            .state
            .read()
            .await
            .applications
            .values()
            .filter(|application| {
                application.organization_id == organization_id
                    && application.project_id == project_id
                    && application.environment_id == environment_id
            })
            .cloned()
            .collect::<Vec<_>>();
        applications.sort_by(|left, right| {
            left.name
                .key()
                .cmp(right.name.key())
                .then_with(|| left.id.cmp(&right.id))
        });
        applications.truncate(limit);
        Ok(applications)
    }

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
        revision_id: DurableCellApplicationRevisionId,
    ) -> Result<Option<DurableCellApplicationRevision>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .revisions
            .get(&(organization_id, application_id, revision_id))
            .filter(|revision| {
                revision.project_id == project_id && revision.environment_id == environment_id
            })
            .cloned())
    }

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
        limit: usize,
    ) -> Result<Vec<DurableCellApplicationRevision>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut revisions = self
            .state
            .read()
            .await
            .revisions
            .values()
            .filter(|revision| {
                revision.organization_id == organization_id
                    && revision.project_id == project_id
                    && revision.environment_id == environment_id
                    && revision.application_id == application_id
            })
            .cloned()
            .collect::<Vec<_>>();
        revisions.sort_by(|left, right| {
            right
                .revision_number
                .cmp(&left.revision_number)
                .then_with(|| left.id.cmp(&right.id))
        });
        revisions.truncate(limit);
        Ok(revisions)
    }
}

fn replay(
    state: &State,
    idempotency: &IdempotencyRequest,
) -> Result<Option<DurableCellApplicationRecord>, RepositoryError> {
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
    load_record(state, reference).map(Some)
}

fn load_record(
    state: &State,
    reference: &DurableCellWriteReference,
) -> Result<DurableCellApplicationRecord, RepositoryError> {
    let head = state
        .applications
        .get(&(reference.organization_id, reference.application_id))
        .filter(|application| {
            application.project_id == reference.project_id
                && application.environment_id == reference.environment_id
        })
        .ok_or_else(|| {
            RepositoryError::Storage("Durable Cell replay application is missing".into())
        })?;
    let revision = state
        .revisions
        .get(&(
            reference.organization_id,
            reference.application_id,
            reference.revision_id,
        ))
        .filter(|revision| {
            revision.project_id == reference.project_id
                && revision.environment_id == reference.environment_id
        })
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Storage("Durable Cell replay revision is missing".into())
        })?;
    DurableCellApplicationRecord::replay_snapshot(
        head,
        revision,
        reference.desired_state,
        reference.aggregate_version,
        reference.updated_at,
    )
    .map_err(RepositoryError::Storage)
}

fn revision_key(
    revision: &DurableCellApplicationRevision,
) -> (
    OrganizationId,
    DurableCellApplicationId,
    DurableCellApplicationRevisionId,
) {
    (
        revision.organization_id,
        revision.application_id,
        revision.id,
    )
}

fn store_replay(
    state: &mut State,
    idempotency: &IdempotencyRequest,
    record: &DurableCellApplicationRecord,
) {
    state.idempotency.insert(
        (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        ),
        (
            idempotency.request_digest.clone(),
            DurableCellWriteReference::from(record),
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::durable_cells::domain::{
        CreateDurableCellApplicationWrite, DurableCellApplicationChanged,
        DurableCellApplicationDefinition, DurableCellApplicationDefinitionSpec,
        DurableCellApplicationDesiredState, DurableCellClassSpec, DurableCellRollbackPolicy,
        DurableCellStateSchema, RequestDurableCellApplicationStateWrite,
        ReviseDurableCellApplicationWrite,
    };
    use crate::modules::shared_kernel::domain::{
        BuildRunId, PrincipalId, ResourceName, Sha256Digest,
    };
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }

    fn definition(character: char, write_version: u64) -> DurableCellApplicationDefinition {
        DurableCellApplicationDefinition::from_spec(DurableCellApplicationDefinitionSpec {
            build_run_id: BuildRunId::new(),
            bundle_digest: digest(character),
            bundle_size_bytes: 1024,
            main_module: "worker.mjs".into(),
            compatibility_date: "2026-08-16".into(),
            compatibility_flags: Vec::new(),
            cell_classes: vec![DurableCellClassSpec {
                name: "Counter".into(),
                state_schema: DurableCellStateSchema {
                    minimum_readable_version: 1,
                    maximum_readable_version: 2,
                    write_version,
                },
            }],
            service_profile_digest: digest('f'),
            rollback_policy: DurableCellRollbackPolicy::Compatible,
        })
        .expect("definition")
    }

    fn idempotency(scope: &str, key: &str, body: &[u8]) -> IdempotencyRequest {
        IdempotencyRequest::new(scope, key, body).expect("idempotency")
    }

    #[tokio::test]
    async fn application_writes_are_exact_replay_safe_without_a_second_lifecycle() {
        let repository = InMemoryDurableCellApplicationRepository::new();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let actor = PrincipalId::new();
        let application_id = DurableCellApplicationId::new();
        let created_at = Utc::now();
        let initial = DurableCellApplicationRevision::initial(
            organization_id,
            project_id,
            environment_id,
            application_id,
            DurableCellApplicationRevisionId::new(),
            definition('a', 1),
            actor,
            created_at,
        )
        .expect("initial revision");
        let application = DurableCellApplication::create(
            application_id,
            ResourceName::parse("Tenant counters").expect("name"),
            &initial,
        )
        .expect("application");
        let initial_record =
            DurableCellApplicationRecord::new(application.clone(), initial.clone())
                .expect("record");
        let create_request = Uuid::now_v7();
        let create_idempotency = idempotency(
            "durable-cell-applications",
            "create",
            initial.definition.canonical_acl().as_bytes(),
        );
        let create = CreateDurableCellApplicationWrite {
            record: initial_record.clone(),
            event: DurableCellApplicationChanged::created(&application, &initial, create_request)
                .expect("event"),
            actor_principal_id: actor,
            request_id: create_request,
            idempotency: create_idempotency.clone(),
        };
        assert!(
            !repository
                .create(create.clone())
                .await
                .expect("create")
                .replayed
        );
        assert!(repository.create(create).await.expect("replay").replayed);

        let conflicting = CreateDurableCellApplicationWrite {
            idempotency: idempotency(
                &create_idempotency.scope,
                &create_idempotency.key,
                b"different request",
            ),
            event: DurableCellApplicationChanged::created(&application, &initial, Uuid::now_v7())
                .expect("event"),
            request_id: Uuid::now_v7(),
            record: initial_record.clone(),
            actor_principal_id: actor,
        };
        assert_eq!(
            repository.create(conflicting).await,
            Err(RepositoryError::IdempotencyConflict)
        );

        let stopped = application
            .request_state(
                1,
                DurableCellApplicationDesiredState::Stopped,
                initial.created_at + Duration::seconds(1),
            )
            .expect("stop");
        let stopped_record =
            DurableCellApplicationRecord::new(stopped.clone(), initial.clone()).expect("record");
        let stop_request = Uuid::now_v7();
        let stop = RequestDurableCellApplicationStateWrite {
            record: stopped_record.clone(),
            expected_version: 1,
            event: DurableCellApplicationChanged::state_requested(&stopped, &initial, stop_request)
                .expect("event"),
            actor_principal_id: actor,
            request_id: stop_request,
            idempotency: idempotency("durable-cell-application", "stop", b"stopped"),
        };
        assert!(
            !repository
                .request_state(stop.clone())
                .await
                .expect("stop")
                .replayed
        );

        let successor = DurableCellApplicationRevision::successor(
            &initial,
            DurableCellApplicationRevisionId::new(),
            definition('b', 2),
            actor,
            initial.created_at + Duration::seconds(2),
        )
        .expect("successor");
        let revised = stopped.advance(2, &successor).expect("advance");
        let revised_record =
            DurableCellApplicationRecord::new(revised.clone(), successor.clone()).expect("record");
        let revise_request = Uuid::now_v7();
        let revise = ReviseDurableCellApplicationWrite {
            record: revised_record.clone(),
            expected_version: 2,
            event: DurableCellApplicationChanged::revised(&revised, &successor, revise_request)
                .expect("event"),
            actor_principal_id: actor,
            request_id: revise_request,
            idempotency: idempotency(
                "durable-cell-revisions",
                "revise",
                successor.definition.canonical_acl().as_bytes(),
            ),
        };
        assert!(!repository.revise(revise).await.expect("revise").replayed);

        let replayed_stop = repository
            .request_state(stop)
            .await
            .expect("replay historical state write");
        assert!(replayed_stop.replayed);
        assert_eq!(replayed_stop.value, stopped_record);
        assert_eq!(
            repository
                .find(organization_id, project_id, environment_id, application_id)
                .await
                .expect("find"),
            Some(revised.clone())
        );
        assert!(repository
            .find(
                organization_id,
                project_id,
                EnvironmentId::new(),
                application_id,
            )
            .await
            .expect("tenant miss")
            .is_none());
        assert_eq!(
            repository
                .list_revisions(
                    organization_id,
                    project_id,
                    environment_id,
                    application_id,
                    10,
                )
                .await
                .expect("revisions"),
            vec![successor, initial]
        );
        let events = repository.outbox_events().await;
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_key, "durable-cell.application.created");
        assert_eq!(events[2].event_key, "durable-cell.application.revised");
    }
}
