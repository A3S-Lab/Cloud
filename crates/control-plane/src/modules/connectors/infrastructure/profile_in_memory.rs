use crate::modules::connectors::domain::{
    ConnectorProfile, ConnectorRecord, ConnectorRevision, ConnectorWriteReference,
    CreateConnectorProfileWrite, IConnectorProfileRepository, ReviseConnectorProfileWrite,
};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, IdempotencyRequest, IdempotentWrite,
    OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryConnectorProfileRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    profiles: BTreeMap<(OrganizationId, ConnectorProfileId), ConnectorProfile>,
    names: BTreeMap<(OrganizationId, ProjectId, EnvironmentId, String), ConnectorProfileId>,
    revisions:
        BTreeMap<(OrganizationId, ConnectorProfileId, ConnectorRevisionId), ConnectorRevision>,
    idempotency: BTreeMap<(String, String), (String, ConnectorWriteReference)>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryConnectorProfileRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IConnectorProfileRepository for InMemoryConnectorProfileRepository {
    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<ConnectorRecord>, RepositoryError> {
        let state = self.state.read().await;
        replay(&state, idempotency)
    }

    async fn create(
        &self,
        write: CreateConnectorProfileWrite,
    ) -> Result<IdempotentWrite<ConnectorRecord>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        if let Some(record) = replay(&state, &write.idempotency)? {
            return Ok(IdempotentWrite {
                value: record,
                replayed: true,
            });
        }
        let profile = &write.record.profile;
        let profile_key = (profile.organization_id, profile.id);
        let name_key = (
            profile.organization_id,
            profile.project_id,
            profile.environment_id,
            profile.name.key().to_owned(),
        );
        if state.profiles.contains_key(&profile_key) || state.names.contains_key(&name_key) {
            return Err(RepositoryError::Conflict(
                "Connector profile name is already in use in this environment".into(),
            ));
        }
        let revision_key = (
            profile.organization_id,
            profile.id,
            write.record.revision.id,
        );
        if state.revisions.contains_key(&revision_key) {
            return Err(RepositoryError::Conflict(
                "Connector revision identity is already in use".into(),
            ));
        }
        state.names.insert(name_key, profile.id);
        state.profiles.insert(profile_key, profile.clone());
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
        write: ReviseConnectorProfileWrite,
    ) -> Result<IdempotentWrite<ConnectorRecord>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        if let Some(record) = replay(&state, &write.idempotency)? {
            return Ok(IdempotentWrite {
                value: record,
                replayed: true,
            });
        }
        let profile_key = (
            write.record.profile.organization_id,
            write.record.profile.id,
        );
        let current = state
            .profiles
            .get(&profile_key)
            .ok_or(RepositoryError::NotFound)?
            .clone();
        write
            .validate_against(&current)
            .map_err(RepositoryError::Conflict)?;
        let revision_key = (
            write.record.profile.organization_id,
            write.record.profile.id,
            write.record.revision.id,
        );
        if state.revisions.contains_key(&revision_key) {
            return Err(RepositoryError::Conflict(
                "Connector revision identity is already in use".into(),
            ));
        }
        state
            .profiles
            .insert(profile_key, write.record.profile.clone());
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

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
    ) -> Result<Option<ConnectorProfile>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .profiles
            .get(&(organization_id, profile_id))
            .filter(|profile| {
                profile.project_id == project_id && profile.environment_id == environment_id
            })
            .cloned())
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        limit: usize,
    ) -> Result<Vec<ConnectorProfile>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut profiles = self
            .state
            .read()
            .await
            .profiles
            .values()
            .filter(|profile| {
                profile.organization_id == organization_id
                    && profile.project_id == project_id
                    && profile.environment_id == environment_id
            })
            .cloned()
            .collect::<Vec<_>>();
        profiles.sort_by(|left, right| {
            left.name
                .key()
                .cmp(right.name.key())
                .then_with(|| left.id.cmp(&right.id))
        });
        profiles.truncate(limit);
        Ok(profiles)
    }

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
    ) -> Result<Option<ConnectorRevision>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .revisions
            .get(&(organization_id, profile_id, revision_id))
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
        profile_id: ConnectorProfileId,
        limit: usize,
    ) -> Result<Vec<ConnectorRevision>, RepositoryError> {
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
                    && revision.profile_id == profile_id
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
) -> Result<Option<ConnectorRecord>, RepositoryError> {
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
        .profiles
        .get(&(reference.organization_id, reference.profile_id))
        .filter(|profile| {
            profile.project_id == reference.project_id
                && profile.environment_id == reference.environment_id
        })
        .cloned()
        .ok_or_else(|| RepositoryError::Storage("Connector replay profile is missing".into()))?;
    let revision = state
        .revisions
        .get(&(
            reference.organization_id,
            reference.profile_id,
            reference.revision_id,
        ))
        .filter(|revision| {
            revision.project_id == reference.project_id
                && revision.environment_id == reference.environment_id
        })
        .cloned()
        .ok_or_else(|| RepositoryError::Storage("Connector replay revision is missing".into()))?;
    let profile = head.at_revision(&revision).map_err(|error| {
        RepositoryError::Storage(format!("Connector replay target is invalid: {error}"))
    })?;
    ConnectorRecord::new(profile, revision)
        .map(Some)
        .map_err(RepositoryError::Storage)
}

fn store_replay(state: &mut State, idempotency: &IdempotencyRequest, record: &ConnectorRecord) {
    state.idempotency.insert(
        (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        ),
        (
            idempotency.request_digest.clone(),
            ConnectorWriteReference::from(record),
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::connectors::domain::{
        ConnectorDefinition, ConnectorHttpAuthentication, ConnectorHttpDefinition,
        ConnectorHttpDefinitionSpec, ConnectorHttpDestination, ConnectorHttpMethod,
        ConnectorHttpStatusPolicy, ConnectorProfile, ConnectorRevision, ConnectorRevisionPublished,
    };
    use crate::modules::shared_kernel::domain::{PrincipalId, ResourceName};
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    fn definition(path: &str) -> ConnectorDefinition {
        ConnectorDefinition::Http(
            ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
                destination: ConnectorHttpDestination::LiteralHttps {
                    endpoint: format!("https://hooks.example.test/{path}"),
                },
                method: ConnectorHttpMethod::Post,
                request_content_type: "application/json".into(),
                maximum_request_bytes: 16 * 1024,
                maximum_response_bytes: 16 * 1024,
                timeout_milliseconds: 5_000,
                status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
                authentication: ConnectorHttpAuthentication::None,
            })
            .expect("definition"),
        )
    }

    fn create_write(record: ConnectorRecord, key: &str) -> CreateConnectorProfileWrite {
        let request_id = Uuid::now_v7();
        CreateConnectorProfileWrite {
            event: ConnectorRevisionPublished::created(
                &record.profile,
                &record.revision,
                request_id,
            )
            .expect("event"),
            actor_principal_id: record.revision.created_by,
            request_id,
            idempotency: IdempotencyRequest::new(
                "connector-profile-tests",
                key,
                record.revision.definition.digest().as_str().as_bytes(),
            )
            .expect("idempotency"),
            record,
        }
    }

    #[tokio::test]
    async fn repository_preserves_exact_history_replay_and_environment_isolation() {
        let repository = InMemoryConnectorProfileRepository::new();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let profile_id = ConnectorProfileId::new();
        let created_at = Utc::now();
        let initial = ConnectorRevision::initial(
            organization_id,
            project_id,
            environment_id,
            profile_id,
            ConnectorRevisionId::new(),
            definition("initial"),
            PrincipalId::new(),
            created_at,
        )
        .expect("initial");
        let profile = ConnectorProfile::create(
            profile_id,
            ResourceName::parse("Pager").expect("name"),
            &initial,
        )
        .expect("profile");
        let record = ConnectorRecord::new(profile, initial.clone()).expect("record");
        let create = create_write(record.clone(), "create");
        assert!(
            !repository
                .create(create.clone())
                .await
                .expect("create")
                .replayed
        );
        assert!(repository.create(create).await.expect("replay").replayed);

        let successor = ConnectorRevision::successor(
            &initial,
            ConnectorRevisionId::new(),
            definition("revised"),
            PrincipalId::new(),
            created_at + Duration::seconds(1),
        )
        .expect("successor");
        let current = record
            .profile
            .advance(1, &successor)
            .expect("advance profile");
        let revised = ConnectorRecord::new(current.clone(), successor.clone()).expect("record");
        let request_id = Uuid::now_v7();
        let revise = ReviseConnectorProfileWrite {
            event: ConnectorRevisionPublished::revised(&current, &successor, request_id)
                .expect("event"),
            actor_principal_id: successor.created_by,
            request_id,
            expected_version: 1,
            idempotency: IdempotencyRequest::new(
                "connector-profile-tests",
                "revise",
                successor.definition.digest().as_str().as_bytes(),
            )
            .expect("idempotency"),
            record: revised,
        };
        assert!(
            !repository
                .revise(revise.clone())
                .await
                .expect("revise")
                .replayed
        );
        assert!(
            repository
                .revise(revise)
                .await
                .expect("replay revise")
                .replayed
        );
        assert_eq!(
            repository
                .find(organization_id, project_id, environment_id, profile_id)
                .await
                .expect("find"),
            Some(current)
        );
        assert!(repository
            .find(
                organization_id,
                project_id,
                EnvironmentId::new(),
                profile_id,
            )
            .await
            .expect("foreign environment")
            .is_none());
        assert_eq!(
            repository
                .list_revisions(organization_id, project_id, environment_id, profile_id, 50)
                .await
                .expect("history"),
            vec![successor, initial]
        );
        assert_eq!(repository.outbox_events().await.len(), 2);
    }
}
