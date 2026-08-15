use crate::modules::connectors::domain::{
    ConnectorExecutionEvidence, ConnectorExecutionEvidenceCursor,
    IConnectorExecutionEvidenceRepository, MAXIMUM_CONNECTOR_EXECUTION_EVIDENCE_PAGE_SIZE,
};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, IdempotentWrite, OrganizationId,
    ProjectId, RepositoryError,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;
use uuid::Uuid;

type EvidenceKey = (
    OrganizationId,
    ProjectId,
    EnvironmentId,
    ConnectorProfileId,
    ConnectorRevisionId,
    Uuid,
);

#[derive(Default)]
pub struct InMemoryConnectorExecutionEvidenceRepository {
    evidence: RwLock<BTreeMap<EvidenceKey, ConnectorExecutionEvidence>>,
}

impl InMemoryConnectorExecutionEvidenceRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IConnectorExecutionEvidenceRepository for InMemoryConnectorExecutionEvidenceRepository {
    async fn record(
        &self,
        evidence: ConnectorExecutionEvidence,
    ) -> Result<IdempotentWrite<ConnectorExecutionEvidence>, RepositoryError> {
        evidence.validate().map_err(RepositoryError::Storage)?;
        let key = evidence_key(&evidence);
        let mut stored = self.evidence.write().await;
        if let Some(existing) = stored.get(&key) {
            if existing == &evidence {
                return Ok(IdempotentWrite {
                    value: existing.clone(),
                    replayed: true,
                });
            }
            return Err(evidence_conflict());
        }
        stored.insert(key, evidence.clone());
        Ok(IdempotentWrite {
            value: evidence,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        attempt_id: Uuid,
    ) -> Result<Option<ConnectorExecutionEvidence>, RepositoryError> {
        Ok(self
            .evidence
            .read()
            .await
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

    async fn list_page(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        after: Option<ConnectorExecutionEvidenceCursor>,
        limit: usize,
    ) -> Result<Vec<ConnectorExecutionEvidence>, RepositoryError> {
        let after = after
            .map(ConnectorExecutionEvidenceCursor::validate)
            .transpose()
            .map_err(RepositoryError::Storage)?;
        let mut evidence = self
            .evidence
            .read()
            .await
            .values()
            .filter(|value| {
                value.organization_id() == organization_id
                    && value.project_id() == project_id
                    && value.environment_id() == environment_id
                    && value.profile_id() == profile_id
                    && value.revision_id() == revision_id
                    && after.is_none_or(|cursor| {
                        value.completed_at() < cursor.completed_at
                            || value.completed_at() == cursor.completed_at
                                && value.attempt_id() < cursor.attempt_id
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        evidence.sort_by(|left, right| {
            right
                .completed_at()
                .cmp(&left.completed_at())
                .then_with(|| right.attempt_id().cmp(&left.attempt_id()))
        });
        evidence.truncate(limit.clamp(1, MAXIMUM_CONNECTOR_EXECUTION_EVIDENCE_PAGE_SIZE + 1));
        Ok(evidence)
    }
}

fn evidence_key(evidence: &ConnectorExecutionEvidence) -> EvidenceKey {
    (
        evidence.organization_id(),
        evidence.project_id(),
        evidence.environment_id(),
        evidence.profile_id(),
        evidence.revision_id(),
        evidence.attempt_id(),
    )
}

fn evidence_conflict() -> RepositoryError {
    RepositoryError::Conflict(
        "Connector execution attempt already records a different terminal fact".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::connectors::domain::{
        ConnectorDefinition, ConnectorExecutionReceipt, ConnectorExecutionRequest,
        ConnectorHttpAuthentication, ConnectorHttpDefinition, ConnectorHttpDefinitionSpec,
        ConnectorHttpDestination, ConnectorHttpMethod, ConnectorHttpStatusPolicy,
        ConnectorRevision,
    };
    use crate::modules::shared_kernel::domain::PrincipalId;
    use chrono::{DateTime, Duration, Utc};
    use std::sync::Arc;

    fn revision(now: DateTime<Utc>) -> ConnectorRevision {
        ConnectorRevision::initial(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            ConnectorProfileId::new(),
            ConnectorRevisionId::new(),
            ConnectorDefinition::Http(
                ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
                    destination: ConnectorHttpDestination::LiteralHttps {
                        endpoint: "https://hooks.example.test/evidence".into(),
                    },
                    method: ConnectorHttpMethod::Post,
                    request_content_type: "application/json".into(),
                    maximum_request_bytes: 1024,
                    maximum_response_bytes: 1024,
                    timeout_milliseconds: 5_000,
                    status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
                    authentication: ConnectorHttpAuthentication::None,
                })
                .expect("definition"),
            ),
            PrincipalId::new(),
            now,
        )
        .expect("revision")
    }

    fn accepted(
        revision: &ConnectorRevision,
        attempt_id: Uuid,
        completed_at: DateTime<Utc>,
    ) -> ConnectorExecutionEvidence {
        let request = ConnectorExecutionRequest::new(
            revision.id,
            attempt_id,
            "application/json",
            b"request".to_vec(),
        )
        .expect("request");
        let receipt = ConnectorExecutionReceipt::accepted(
            revision.id,
            attempt_id,
            completed_at,
            200,
            None,
            b"response".to_vec(),
        )
        .expect("receipt");
        ConnectorExecutionEvidence::accepted(
            revision,
            &request,
            &receipt,
            completed_at - Duration::milliseconds(5),
        )
        .expect("evidence")
    }

    #[tokio::test]
    async fn concurrent_identical_records_converge_and_changed_replay_conflicts() {
        let repository = Arc::new(InMemoryConnectorExecutionEvidenceRepository::new());
        let revision = revision(Utc::now());
        let evidence = accepted(&revision, Uuid::now_v7(), Utc::now());
        let (left, right) = tokio::join!(
            repository.record(evidence.clone()),
            repository.record(evidence.clone())
        );
        let mut replayed = [left.expect("left").replayed, right.expect("right").replayed];
        replayed.sort_unstable();
        assert_eq!(replayed, [false, true]);

        let request = ConnectorExecutionRequest::new(
            revision.id,
            evidence.attempt_id(),
            "application/json",
            b"changed".to_vec(),
        )
        .expect("changed request");
        let changed = ConnectorExecutionEvidence::rejected(
            &revision,
            &request,
            Some(400),
            evidence.started_at(),
            evidence.completed_at(),
        )
        .expect("changed evidence");
        assert!(matches!(
            repository.record(changed).await,
            Err(RepositoryError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn exact_scope_and_keyset_pages_do_not_cross_tenants() {
        let repository = InMemoryConnectorExecutionEvidenceRepository::new();
        let now = Utc::now();
        let revision = revision(now);
        let mut expected = Vec::new();
        for seconds in 1..=3 {
            let evidence = accepted(&revision, Uuid::now_v7(), now + Duration::seconds(seconds));
            repository.record(evidence.clone()).await.expect("record");
            expected.push(evidence);
        }
        expected.sort_by(|left, right| {
            right
                .completed_at()
                .cmp(&left.completed_at())
                .then_with(|| right.attempt_id().cmp(&left.attempt_id()))
        });

        let first = repository
            .list_page(
                revision.organization_id,
                revision.project_id,
                revision.environment_id,
                revision.profile_id,
                revision.id,
                None,
                2,
            )
            .await
            .expect("first page");
        assert_eq!(first, expected[..2]);
        let second = repository
            .list_page(
                revision.organization_id,
                revision.project_id,
                revision.environment_id,
                revision.profile_id,
                revision.id,
                Some(ConnectorExecutionEvidenceCursor::after(&first[1])),
                2,
            )
            .await
            .expect("second page");
        assert_eq!(second, expected[2..]);
        assert!(repository
            .find(
                OrganizationId::new(),
                revision.project_id,
                revision.environment_id,
                revision.profile_id,
                revision.id,
                expected[0].attempt_id(),
            )
            .await
            .expect("foreign tenant")
            .is_none());
    }
}
