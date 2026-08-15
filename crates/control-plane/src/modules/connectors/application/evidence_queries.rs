use super::resource_access::{environment, evidence_not_found, revision_not_found};
use crate::modules::connectors::domain::{
    ConnectorExecutionEvidence, ConnectorExecutionEvidenceCursor, ConnectorExecutionEvidencePage,
    IConnectorExecutionEvidenceRepository, IConnectorProfileRepository,
    MAXIMUM_CONNECTOR_EXECUTION_EVIDENCE_PAGE_SIZE,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, OrganizationId, ProjectId,
    RepositoryError,
};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GetConnectorExecutionEvidence {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub revision_id: ConnectorRevisionId,
    pub attempt_id: Uuid,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetConnectorExecutionEvidence {
    type Output = ApplicationResult<ConnectorExecutionEvidence>;
}

pub struct GetConnectorExecutionEvidenceHandler {
    evidence: Arc<dyn IConnectorExecutionEvidenceRepository>,
}

impl GetConnectorExecutionEvidenceHandler {
    pub fn new(evidence: Arc<dyn IConnectorExecutionEvidenceRepository>) -> Self {
        Self { evidence }
    }
}

impl QueryHandler<GetConnectorExecutionEvidence> for GetConnectorExecutionEvidenceHandler {
    fn execute(
        &self,
        query: GetConnectorExecutionEvidence,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<ConnectorExecutionEvidence>>>
    {
        let evidence = Arc::clone(&self.evidence);
        Box::pin(async move {
            if let Err(error) = authorize_and_validate(
                query.organization_id,
                query.project_id,
                query.environment_id,
                query.profile_id,
                query.revision_id,
                Some(query.attempt_id),
                &query.resource_access,
            ) {
                return Ok(Err(error));
            }
            Ok(
                match evidence
                    .find(
                        query.organization_id,
                        query.project_id,
                        query.environment_id,
                        query.profile_id,
                        query.revision_id,
                        query.attempt_id,
                    )
                    .await
                {
                    Ok(Some(value)) => Ok(value),
                    Ok(None) | Err(RepositoryError::NotFound) => Err(evidence_not_found()),
                    Err(error) => Err(error.into()),
                },
            )
        })
    }
}

#[derive(Debug, Clone)]
pub struct ListConnectorExecutionEvidence {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub revision_id: ConnectorRevisionId,
    pub after: Option<ConnectorExecutionEvidenceCursor>,
    pub limit: usize,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for ListConnectorExecutionEvidence {
    type Output = ApplicationResult<ConnectorExecutionEvidencePage>;
}

pub struct ListConnectorExecutionEvidenceHandler {
    connectors: Arc<dyn IConnectorProfileRepository>,
    evidence: Arc<dyn IConnectorExecutionEvidenceRepository>,
}

impl ListConnectorExecutionEvidenceHandler {
    pub fn new(
        connectors: Arc<dyn IConnectorProfileRepository>,
        evidence: Arc<dyn IConnectorExecutionEvidenceRepository>,
    ) -> Self {
        Self {
            connectors,
            evidence,
        }
    }
}

impl QueryHandler<ListConnectorExecutionEvidence> for ListConnectorExecutionEvidenceHandler {
    fn execute(
        &self,
        query: ListConnectorExecutionEvidence,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ConnectorExecutionEvidencePage>>,
    > {
        let connectors = Arc::clone(&self.connectors);
        let evidence = Arc::clone(&self.evidence);
        Box::pin(async move {
            if let Err(error) = authorize_and_validate(
                query.organization_id,
                query.project_id,
                query.environment_id,
                query.profile_id,
                query.revision_id,
                None,
                &query.resource_access,
            ) {
                return Ok(Err(error));
            }
            if query.limit == 0 || query.limit > MAXIMUM_CONNECTOR_EXECUTION_EVIDENCE_PAGE_SIZE {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "Connector execution evidence limit must be between 1 and {MAXIMUM_CONNECTOR_EXECUTION_EVIDENCE_PAGE_SIZE}"
                ))));
            }
            let after = match query
                .after
                .map(ConnectorExecutionEvidenceCursor::validate)
                .transpose()
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match connectors
                .find_revision(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                    query.profile_id,
                    query.revision_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) | Err(RepositoryError::NotFound) => return Ok(Err(revision_not_found())),
                Err(error) => return Ok(Err(error.into())),
            }
            let mut page = match evidence
                .list_page(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                    query.profile_id,
                    query.revision_id,
                    after,
                    query.limit + 1,
                )
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            let next_cursor = (page.len() > query.limit)
                .then(|| ConnectorExecutionEvidenceCursor::after(&page[query.limit - 1]));
            page.truncate(query.limit);
            Ok(Ok(ConnectorExecutionEvidencePage {
                evidence: page,
                next_cursor,
            }))
        })
    }
}

fn authorize_and_validate(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    profile_id: ConnectorProfileId,
    revision_id: ConnectorRevisionId,
    attempt_id: Option<Uuid>,
    evaluator: &ResourceAccessEvaluator,
) -> ApplicationResult<()> {
    environment(project_id, environment_id, evaluator)?;
    if organization_id.as_uuid().is_nil()
        || project_id.as_uuid().is_nil()
        || environment_id.as_uuid().is_nil()
        || profile_id.as_uuid().is_nil()
        || revision_id.as_uuid().is_nil()
        || attempt_id.is_some_and(|value| value.is_nil())
    {
        return Err(ApplicationError::Invalid(
            "Connector execution evidence identity is invalid".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::connectors::domain::{
        BeginConnectorExecutionDispatch, ConnectorDefinition, ConnectorExecutionAttemptBinding,
        ConnectorExecutionReceipt, ConnectorExecutionRequest, ConnectorExecutionReservation,
        ConnectorHttpAuthentication, ConnectorHttpDefinition, ConnectorHttpDefinitionSpec,
        ConnectorHttpDestination, ConnectorHttpMethod, ConnectorHttpStatusPolicy, ConnectorProfile,
        ConnectorRecord, ConnectorRevision, ConnectorRevisionPublished,
        CreateConnectorProfileWrite, IConnectorExecutionAttemptRepository,
        ReserveConnectorExecutionAttempt, SettleConnectorExecutionAttempt,
    };
    use crate::modules::connectors::infrastructure::{
        InMemoryConnectorExecutionEvidenceRepository, InMemoryConnectorProfileRepository,
    };
    use crate::modules::identity::domain::value_objects::ResourceGrantScope;
    use crate::modules::shared_kernel::domain::{IdempotencyRequest, PrincipalId, ResourceName};
    use a3s_boot::{ModuleRef, QueryHandler};
    use chrono::{Duration, Utc};

    #[tokio::test]
    async fn queries_are_authorized_exact_and_keyset_bounded() {
        let now = Utc::now();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let profile_id = ConnectorProfileId::new();
        let revision = ConnectorRevision::initial(
            organization_id,
            project_id,
            environment_id,
            profile_id,
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
        .expect("revision");
        let profile = ConnectorProfile::create(
            profile_id,
            ResourceName::parse("Evidence Connector").expect("name"),
            &revision,
        )
        .expect("profile");
        let record = ConnectorRecord::new(profile, revision.clone()).expect("record");
        let request_id = Uuid::now_v7();
        let profiles = Arc::new(InMemoryConnectorProfileRepository::new());
        profiles
            .create(CreateConnectorProfileWrite {
                event: ConnectorRevisionPublished::created(
                    &record.profile,
                    &record.revision,
                    request_id,
                )
                .expect("event"),
                actor_principal_id: revision.created_by,
                request_id,
                idempotency: IdempotencyRequest::new(
                    "connector-evidence-query-test",
                    "profile",
                    revision.definition.digest().as_str().as_bytes(),
                )
                .expect("idempotency"),
                record,
            })
            .await
            .expect("store profile");
        let evidence = Arc::new(InMemoryConnectorExecutionEvidenceRepository::new());
        let mut expected = Vec::new();
        for seconds in 1..=3 {
            let attempt_id = Uuid::now_v7();
            let request = ConnectorExecutionRequest::new(
                revision.id,
                attempt_id,
                "application/json",
                format!("request-{seconds}").into_bytes(),
            )
            .expect("request");
            let completed_at = now + Duration::seconds(seconds);
            let receipt = ConnectorExecutionReceipt::accepted(
                revision.id,
                attempt_id,
                completed_at,
                200,
                None,
                format!("response-{seconds}").into_bytes(),
            )
            .expect("receipt");
            let value = ConnectorExecutionEvidence::accepted(
                &revision,
                &request,
                &receipt,
                completed_at - Duration::milliseconds(5),
            )
            .expect("evidence");
            let dispatch_started_at = value.started_at();
            let reserved_at = dispatch_started_at - Duration::milliseconds(1);
            let fence = match evidence
                .reserve(
                    ReserveConnectorExecutionAttempt::new(
                        ConnectorExecutionAttemptBinding::from_exact(&revision, &request)
                            .expect("binding"),
                        Uuid::now_v7(),
                        reserved_at,
                        reserved_at + Duration::seconds(30),
                    )
                    .expect("reservation"),
                )
                .await
                .expect("reserve")
            {
                ConnectorExecutionReservation::Acquired { fence, .. } => fence,
                other => panic!("unexpected reservation: {other:?}"),
            };
            evidence
                .begin_dispatch(
                    BeginConnectorExecutionDispatch::new(
                        fence.clone(),
                        dispatch_started_at,
                        dispatch_started_at + Duration::seconds(10),
                    )
                    .expect("dispatch"),
                )
                .await
                .expect("begin dispatch");
            evidence
                .settle(
                    SettleConnectorExecutionAttempt::new(fence, value.clone()).expect("settlement"),
                )
                .await
                .expect("settle");
            expected.push(value);
        }
        expected.sort_by(|left, right| {
            right
                .completed_at()
                .cmp(&left.completed_at())
                .then_with(|| right.attempt_id().cmp(&left.attempt_id()))
        });

        let handler = ListConnectorExecutionEvidenceHandler::new(profiles, evidence.clone());
        let query = ListConnectorExecutionEvidence {
            organization_id,
            project_id,
            environment_id,
            profile_id,
            revision_id: revision.id,
            after: None,
            limit: 2,
            resource_access: ResourceAccessEvaluator::organization_wide(),
        };
        let first = handler
            .execute(query.clone(), context())
            .await
            .expect("query framework")
            .expect("first page");
        assert_eq!(first.evidence, expected[..2]);
        assert_eq!(
            first.next_cursor,
            Some(ConnectorExecutionEvidenceCursor::after(&expected[1]))
        );
        let second = handler
            .execute(
                ListConnectorExecutionEvidence {
                    after: first.next_cursor,
                    ..query
                },
                context(),
            )
            .await
            .expect("query framework")
            .expect("second page");
        assert_eq!(second.evidence, expected[2..]);
        assert!(second.next_cursor.is_none());

        let loaded = GetConnectorExecutionEvidenceHandler::new(evidence.clone())
            .execute(
                GetConnectorExecutionEvidence {
                    organization_id,
                    project_id,
                    environment_id,
                    profile_id,
                    revision_id: revision.id,
                    attempt_id: expected[0].attempt_id(),
                    resource_access: ResourceAccessEvaluator::organization_wide(),
                },
                context(),
            )
            .await
            .expect("query framework")
            .expect("get evidence");
        assert_eq!(loaded, expected[0]);
        let denied = GetConnectorExecutionEvidenceHandler::new(evidence)
            .execute(
                GetConnectorExecutionEvidence {
                    organization_id,
                    project_id,
                    environment_id,
                    profile_id,
                    revision_id: revision.id,
                    attempt_id: expected[0].attempt_id(),
                    resource_access: ResourceAccessEvaluator::restricted([
                        ResourceGrantScope::Environment {
                            project_id,
                            environment_id: EnvironmentId::new(),
                        },
                    ]),
                },
                context(),
            )
            .await
            .expect("query framework");
        assert!(matches!(denied, Err(ApplicationError::NotFound(_))));
    }

    fn context() -> CqrsContext {
        CqrsContext::new(ModuleRef::new())
    }
}
