use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::inference::domain::{IInferenceUsageRepository, InferenceUsageRequestFact};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GetUsageRequestFact {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub request_id: Uuid,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetUsageRequestFact {
    type Output = ApplicationResult<InferenceUsageRequestFact>;
}

pub struct GetUsageRequestFactHandler {
    usage: Arc<dyn IInferenceUsageRepository>,
}

impl GetUsageRequestFactHandler {
    pub fn new(usage: Arc<dyn IInferenceUsageRepository>) -> Self {
        Self { usage }
    }
}

impl QueryHandler<GetUsageRequestFact> for GetUsageRequestFactHandler {
    fn execute(
        &self,
        query: GetUsageRequestFact,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<InferenceUsageRequestFact>>>
    {
        let usage = Arc::clone(&self.usage);
        Box::pin(async move {
            if !query
                .resource_access
                .environment_is_visible(query.project_id, query.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "environment not found in organization".into(),
                )));
            }
            match usage
                .get_request_fact(query.organization_id, query.request_id)
                .await
            {
                Ok(Some(fact)) if fact.environment_id == query.environment_id.as_uuid() => {
                    Ok(Ok(fact))
                }
                Ok(_) => Ok(Err(ApplicationError::NotFound(
                    "inference usage request fact not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::value_objects::ResourceGrantScope;
    use crate::modules::inference::domain::AcceptInferenceUsageBatchWrite;
    use crate::modules::inference::InMemoryInferenceUsageRepository;
    use crate::modules::shared_kernel::domain::NodeId;
    use a3s_boot::ModuleRef;
    use a3s_cloud_contracts::{
        InferenceUsageBatchV1, InferenceUsageCursorV1, InferenceUsageEndpointV1,
        InferenceUsageLifecycleEventV1, InferenceUsageLifecycleKindV1,
        InferenceUsageMeasurementCompletenessV1, InferenceUsageRecordV1,
        InferenceUsageRequestEvidenceV1, InferenceUsageTerminalOutcomeV1,
    };
    use base64::Engine;
    use chrono::{DateTime, Utc};
    use sha2::{Digest, Sha256};

    const REQUEST_ID: u128 = 100;
    const ENVIRONMENT_ID: u128 = 101;

    fn org_wide() -> ResourceAccessEvaluator {
        ResourceAccessEvaluator::organization_wide()
    }

    fn lifecycle(kind: InferenceUsageLifecycleKindV1, at: &str, terminal: bool) -> Vec<u8> {
        let event = InferenceUsageLifecycleEventV1 {
            schema: InferenceUsageLifecycleEventV1::SCHEMA.into(),
            kind,
            occurred_at: DateTime::parse_from_rfc3339(at)
                .unwrap()
                .with_timezone(&Utc),
            request: InferenceUsageRequestEvidenceV1 {
                request_id: Uuid::from_u128(REQUEST_ID),
                correlation_id: "corr".into(),
                environment_id: Uuid::from_u128(ENVIRONMENT_ID),
                credential_id: Uuid::from_u128(102),
                credential_generation: 1,
                route_id: Uuid::from_u128(103),
                route_policy_revision: 1,
                endpoint: InferenceUsageEndpointV1::ChatCompletions,
                model_alias: "alias".into(),
                model_id: Uuid::from_u128(104),
            },
            attempt: None,
            outcome: terminal.then_some(InferenceUsageTerminalOutcomeV1::Succeeded),
            http_status: terminal.then_some(200),
            duration_ms: terminal.then_some(1),
            measurement_completeness: terminal
                .then_some(InferenceUsageMeasurementCompletenessV1::UpstreamUsage),
            total_tokens: terminal.then_some(7),
        };
        serde_json::to_vec(&event).unwrap()
    }

    fn record(
        cursor: InferenceUsageCursorV1,
        event_id: Uuid,
        payload: &[u8],
    ) -> InferenceUsageRecordV1 {
        InferenceUsageRecordV1 {
            cursor,
            event_id,
            payload_base64: base64::engine::general_purpose::STANDARD.encode(payload),
            payload_sha256: format!("{:x}", Sha256::digest(payload)),
        }
    }

    async fn seed_request_fact(
        usage: &InMemoryInferenceUsageRepository,
        organization_id: OrganizationId,
    ) {
        let started = lifecycle(
            InferenceUsageLifecycleKindV1::RequestStarted,
            "2026-01-02T10:00:00Z",
            false,
        );
        let finished = lifecycle(
            InferenceUsageLifecycleKindV1::RequestTerminal,
            "2026-01-02T10:00:01Z",
            true,
        );
        let epoch = Uuid::from_u128(9);
        usage
            .accept_usage_batch(
                AcceptInferenceUsageBatchWrite::new(
                    organization_id,
                    NodeId::from_uuid(Uuid::from_u128(2)),
                    InferenceUsageBatchV1 {
                        schema: InferenceUsageBatchV1::SCHEMA.into(),
                        gateway_id: Uuid::from_u128(3),
                        batch_id: Uuid::from_u128(4),
                        after: None,
                        records: vec![
                            record(
                                InferenceUsageCursorV1 {
                                    boot_epoch: epoch,
                                    sequence: 1,
                                },
                                Uuid::from_u128(10),
                                &started,
                            ),
                            record(
                                InferenceUsageCursorV1 {
                                    boot_epoch: epoch,
                                    sequence: 2,
                                },
                                Uuid::from_u128(11),
                                &finished,
                            ),
                        ],
                    },
                    Utc::now(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn returns_fact_when_environment_is_visible() {
        let usage = Arc::new(InMemoryInferenceUsageRepository::new());
        let organization_id = OrganizationId::from_uuid(Uuid::from_u128(1));
        let project_id = ProjectId::from_uuid(Uuid::from_u128(50));
        let environment_id = EnvironmentId::from_uuid(Uuid::from_u128(ENVIRONMENT_ID));
        seed_request_fact(usage.as_ref(), organization_id).await;
        let handler = GetUsageRequestFactHandler::new(usage);

        let fact = handler
            .execute(
                GetUsageRequestFact {
                    organization_id,
                    project_id,
                    environment_id,
                    request_id: Uuid::from_u128(REQUEST_ID),
                    resource_access: org_wide(),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fact.request_id, Uuid::from_u128(REQUEST_ID));
        assert_eq!(fact.environment_id, Uuid::from_u128(ENVIRONMENT_ID));
        assert_eq!(fact.total_tokens, Some(7));
    }

    #[tokio::test]
    async fn hides_environment_when_grant_is_absent() {
        let usage = Arc::new(InMemoryInferenceUsageRepository::new());
        let organization_id = OrganizationId::from_uuid(Uuid::from_u128(1));
        let project_id = ProjectId::from_uuid(Uuid::from_u128(50));
        let environment_id = EnvironmentId::from_uuid(Uuid::from_u128(ENVIRONMENT_ID));
        seed_request_fact(usage.as_ref(), organization_id).await;
        let handler = GetUsageRequestFactHandler::new(usage);

        let denied = handler
            .execute(
                GetUsageRequestFact {
                    organization_id,
                    project_id,
                    environment_id,
                    request_id: Uuid::from_u128(REQUEST_ID),
                    resource_access: ResourceAccessEvaluator::restricted(vec![
                        ResourceGrantScope::Environment {
                            project_id,
                            environment_id: EnvironmentId::from_uuid(Uuid::from_u128(999)),
                        },
                    ]),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .unwrap()
            .unwrap_err();
        assert!(matches!(denied, ApplicationError::NotFound(_)));
    }

    #[tokio::test]
    async fn hides_fact_from_a_different_environment() {
        let usage = Arc::new(InMemoryInferenceUsageRepository::new());
        let organization_id = OrganizationId::from_uuid(Uuid::from_u128(1));
        let project_id = ProjectId::from_uuid(Uuid::from_u128(50));
        let other_environment = EnvironmentId::from_uuid(Uuid::from_u128(202));
        seed_request_fact(usage.as_ref(), organization_id).await;
        let handler = GetUsageRequestFactHandler::new(usage);

        let denied = handler
            .execute(
                GetUsageRequestFact {
                    organization_id,
                    project_id,
                    environment_id: other_environment,
                    request_id: Uuid::from_u128(REQUEST_ID),
                    resource_access: org_wide(),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .unwrap()
            .unwrap_err();
        assert!(matches!(denied, ApplicationError::NotFound(_)));
    }

    #[tokio::test]
    async fn hides_missing_request_as_not_found() {
        let usage = Arc::new(InMemoryInferenceUsageRepository::new());
        let organization_id = OrganizationId::from_uuid(Uuid::from_u128(1));
        let project_id = ProjectId::from_uuid(Uuid::from_u128(50));
        let environment_id = EnvironmentId::from_uuid(Uuid::from_u128(ENVIRONMENT_ID));
        let handler = GetUsageRequestFactHandler::new(usage);

        let denied = handler
            .execute(
                GetUsageRequestFact {
                    organization_id,
                    project_id,
                    environment_id,
                    request_id: Uuid::from_u128(999),
                    resource_access: org_wide(),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .unwrap()
            .unwrap_err();
        assert!(matches!(denied, ApplicationError::NotFound(_)));
    }

    #[tokio::test]
    async fn hides_purged_fact_after_retention_sweep() {
        use crate::modules::inference::domain::{
            InferenceUsageRetentionPolicy, InferenceUsageRetentionSweep,
        };
        use std::time::Duration;

        let usage = Arc::new(InMemoryInferenceUsageRepository::new());
        let organization_id = OrganizationId::from_uuid(Uuid::from_u128(1));
        let project_id = ProjectId::from_uuid(Uuid::from_u128(50));
        let environment_id = EnvironmentId::from_uuid(Uuid::from_u128(ENVIRONMENT_ID));
        seed_request_fact(usage.as_ref(), organization_id).await;

        let policy =
            InferenceUsageRetentionPolicy::new(Duration::from_millis(86_400_000)).expect("policy");
        let cutoff = DateTime::parse_from_rfc3339("2026-01-03T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let swept_at = DateTime::parse_from_rfc3339("2026-01-03T01:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let next_scan_at = DateTime::parse_from_rfc3339("2026-01-03T02:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        usage
            .sweep_retention(InferenceUsageRetentionSweep {
                cutoff,
                swept_at,
                next_scan_at,
                policy_digest: policy.digest().clone(),
                organization_batch_size: 10,
                record_batch_size: 100,
            })
            .await
            .expect("sweep");

        let handler = GetUsageRequestFactHandler::new(usage);
        let denied = handler
            .execute(
                GetUsageRequestFact {
                    organization_id,
                    project_id,
                    environment_id,
                    request_id: Uuid::from_u128(REQUEST_ID),
                    resource_access: org_wide(),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .unwrap()
            .unwrap_err();
        assert!(
            matches!(denied, ApplicationError::NotFound(_)),
            "purged showback facts must hide as NotFound, got {denied:?}"
        );
    }
}
