use super::attempt_in_memory::{binding_key, AttemptKey, InMemoryConnectorExecutionRepository};
use crate::modules::connectors::domain::{
    ConnectorExecutionAttempt, ConnectorExecutionAttemptRecord,
    ConnectorExecutionAttemptResolution, ConnectorExecutionAttemptResolutionReference,
    ConnectorExecutionAttemptState, IConnectorExecutionAttemptResolutionRepository,
    ResolveConnectorExecutionAttemptWrite,
};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, IdempotencyRequest, IdempotentWrite,
    OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Default)]
pub(super) struct AttemptResolutionState {
    resolutions: BTreeMap<AttemptKey, ConnectorExecutionAttemptResolution>,
    idempotency: BTreeMap<(String, String), (String, ConnectorExecutionAttemptResolutionReference)>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryConnectorExecutionRepository {
    pub async fn resolution_outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.resolution_authority.read().await.outbox.clone()
    }
}

#[async_trait]
impl IConnectorExecutionAttemptResolutionRepository for InMemoryConnectorExecutionRepository {
    async fn replay_resolution_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<ConnectorExecutionAttemptResolution>, RepositoryError> {
        let state = self.resolution_authority.read().await;
        replay_resolution(&state, idempotency)
    }

    async fn resolve_indeterminate(
        &self,
        write: ResolveConnectorExecutionAttemptWrite,
    ) -> Result<IdempotentWrite<ConnectorExecutionAttemptResolution>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut resolutions = self.resolution_authority.write().await;
        if let Some(resolution) = replay_resolution(&resolutions, &write.idempotency)? {
            return Ok(IdempotentWrite {
                value: resolution,
                replayed: true,
            });
        }
        let key = binding_key(write.resolution.binding());
        if resolutions.resolutions.contains_key(&key) {
            return Err(RepositoryError::Conflict(
                "Connector execution attempt is already resolved".into(),
            ));
        }
        let mut attempts = self.attempts.write().await;
        let current = attempts
            .get(&key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        write
            .validate_against(&current.attempt)
            .map_err(RepositoryError::Conflict)?;
        let terminal = ConnectorExecutionAttempt::restore(
            current.attempt.binding().clone(),
            ConnectorExecutionAttemptState::Terminal,
            current.attempt.fence_generation(),
            current.attempt.fence().token(),
            current.attempt.reserved_at(),
            current.attempt.lease_expires_at(),
            current.attempt.dispatch_started_at(),
            current.attempt.outcome_deadline_at(),
            Some(write.evidence.completed_at()),
            current.attempt.created_at(),
        )
        .map_err(RepositoryError::Conflict)?;
        let record = ConnectorExecutionAttemptRecord::new(terminal, Some(write.evidence))
            .map_err(RepositoryError::Storage)?;
        let reference = ConnectorExecutionAttemptResolutionReference::from(&write.resolution);
        attempts.insert(key, record);
        resolutions
            .resolutions
            .insert(key, write.resolution.clone());
        resolutions.idempotency.insert(
            (
                write.idempotency.storage_key().0.to_owned(),
                write.idempotency.storage_key().1.to_owned(),
            ),
            (write.idempotency.request_digest.clone(), reference),
        );
        resolutions.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.resolution,
            replayed: false,
        })
    }

    async fn find_resolution(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        attempt_id: Uuid,
    ) -> Result<Option<ConnectorExecutionAttemptResolution>, RepositoryError> {
        Ok(self
            .resolution_authority
            .read()
            .await
            .resolutions
            .get(&(
                organization_id,
                project_id,
                environment_id,
                profile_id,
                revision_id,
                attempt_id,
            ))
            .cloned())
    }
}

fn replay_resolution(
    state: &AttemptResolutionState,
    idempotency: &IdempotencyRequest,
) -> Result<Option<ConnectorExecutionAttemptResolution>, RepositoryError> {
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
    state
        .resolutions
        .get(&(
            reference.organization_id,
            reference.project_id,
            reference.environment_id,
            reference.profile_id,
            reference.revision_id,
            reference.attempt_id,
        ))
        .cloned()
        .map(Some)
        .ok_or_else(|| {
            RepositoryError::Storage(
                "Connector execution attempt resolution replay fact is missing".into(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::connectors::domain::{
        BeginConnectorExecutionDispatch, ConnectorDefinition, ConnectorExecutionAttemptBinding,
        ConnectorExecutionAttemptResolution, ConnectorExecutionAttemptResolved,
        ConnectorExecutionOutcome, ConnectorExecutionRequest, ConnectorExecutionReservation,
        ConnectorHttpAuthentication, ConnectorHttpDefinition, ConnectorHttpDefinitionSpec,
        ConnectorHttpDestination, ConnectorHttpMethod, ConnectorHttpStatusPolicy,
        ConnectorRevision, IConnectorExecutionAttemptRepository, ReserveConnectorExecutionAttempt,
        ResolveConnectorExecutionAttemptWrite,
    };
    use crate::modules::shared_kernel::domain::{
        canonical_timestamp, ConnectorProfileId, ConnectorRevisionId, EnvironmentId,
        IdempotencyRequest, OrganizationId, PrincipalId, ProjectId,
    };
    use chrono::{Duration, Utc};

    #[tokio::test]
    async fn resolution_is_atomic_idempotent_and_removes_unresolved_attempt() {
        let now = canonical_timestamp(Utc::now()) - Duration::seconds(30);
        let revision = ConnectorRevision::initial(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            ConnectorProfileId::new(),
            ConnectorRevisionId::new(),
            ConnectorDefinition::Http(
                ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
                    destination: ConnectorHttpDestination::LiteralHttps {
                        endpoint: "https://hooks.example.test/recovery".into(),
                    },
                    method: ConnectorHttpMethod::Post,
                    request_content_type: "application/json".into(),
                    maximum_request_bytes: 1024,
                    maximum_response_bytes: 1024,
                    timeout_milliseconds: 1_000,
                    status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
                    authentication: ConnectorHttpAuthentication::None,
                })
                .expect("definition"),
            ),
            PrincipalId::new(),
            now,
        )
        .expect("revision");
        let request = ConnectorExecutionRequest::new(
            revision.id,
            Uuid::now_v7(),
            "application/json",
            b"request".to_vec(),
        )
        .expect("request");
        let repository = InMemoryConnectorExecutionRepository::new();
        let fence = match repository
            .reserve(
                ReserveConnectorExecutionAttempt::new(
                    ConnectorExecutionAttemptBinding::from_exact(&revision, &request)
                        .expect("binding"),
                    Uuid::now_v7(),
                    now,
                    now + Duration::seconds(20),
                )
                .expect("reservation"),
            )
            .await
            .expect("reserve")
        {
            ConnectorExecutionReservation::Acquired { fence, .. } => fence,
            other => panic!("unexpected reservation: {other:?}"),
        };
        let dispatch_started_at = now + Duration::seconds(1);
        let outcome_deadline_at = now + Duration::seconds(5);
        let dispatch = repository
            .begin_dispatch(
                BeginConnectorExecutionDispatch::new(
                    fence,
                    dispatch_started_at,
                    outcome_deadline_at,
                )
                .expect("dispatch"),
            )
            .await
            .expect("begin dispatch");
        let resolved_at = outcome_deadline_at + Duration::seconds(1);
        let (resolution, evidence) = ConnectorExecutionAttemptResolution::new(
            &dispatch.attempt,
            "provider outcome unavailable",
            PrincipalId::new(),
            resolved_at,
        )
        .expect("resolution");
        let request_id = Uuid::now_v7();
        let idempotency = IdempotencyRequest::new(
            "connector-attempt-resolution-test",
            "resolve",
            b"exact-resolution",
        )
        .expect("idempotency");
        let write = ResolveConnectorExecutionAttemptWrite {
            event: ConnectorExecutionAttemptResolved::envelope(&resolution, request_id)
                .expect("event"),
            actor_principal_id: resolution.resolved_by(),
            request_id,
            idempotency,
            evidence,
            resolution: resolution.clone(),
        };
        let first = repository
            .resolve_indeterminate(write.clone())
            .await
            .expect("resolve");
        assert!(!first.replayed);
        let replay = repository
            .resolve_indeterminate(write)
            .await
            .expect("replay");
        assert!(replay.replayed);
        assert_eq!(replay.value, resolution);
        let record = repository
            .find(
                revision.organization_id,
                revision.project_id,
                revision.environment_id,
                revision.profile_id,
                revision.id,
                request.attempt_id(),
            )
            .await
            .expect("find")
            .expect("record");
        assert_eq!(
            record.attempt.state(),
            ConnectorExecutionAttemptState::Terminal
        );
        assert_eq!(
            record.evidence.expect("evidence").outcome(),
            ConnectorExecutionOutcome::Indeterminate
        );
        assert!(repository
            .list_unresolved_page(
                revision.organization_id,
                revision.project_id,
                revision.environment_id,
                revision.profile_id,
                revision.id,
                None,
                10,
            )
            .await
            .expect("unresolved")
            .is_empty());
        assert_eq!(repository.resolution_outbox_events().await.len(), 1);
    }
}
