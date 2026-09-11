use crate::modules::inference::application::{
    IInferenceEnvironmentAccess, InferenceEnvironmentScope,
};
use crate::modules::inference::domain::{IInferenceUsageRepository, InferenceUsageDailyRollup};
use crate::modules::inference::{InferenceAccess, InferenceAccessScope};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use chrono::NaiveDate;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListDailyUsageRollups {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub from_day: NaiveDate,
    pub to_day: NaiveDate,
    pub access: InferenceAccess,
}

impl Query for ListDailyUsageRollups {
    type Output = ApplicationResult<Vec<InferenceUsageDailyRollup>>;
}

pub struct ListDailyUsageRollupsHandler {
    environments: Arc<dyn IInferenceEnvironmentAccess>,
    usage: Arc<dyn IInferenceUsageRepository>,
}

impl ListDailyUsageRollupsHandler {
    pub fn new(
        environments: Arc<dyn IInferenceEnvironmentAccess>,
        usage: Arc<dyn IInferenceUsageRepository>,
    ) -> Self {
        Self {
            environments,
            usage,
        }
    }
}

impl QueryHandler<ListDailyUsageRollups> for ListDailyUsageRollupsHandler {
    fn execute(
        &self,
        query: ListDailyUsageRollups,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<InferenceUsageDailyRollup>>>,
    > {
        let environments = Arc::clone(&self.environments);
        let usage = Arc::clone(&self.usage);
        Box::pin(async move {
            if !query
                .access
                .environment_is_visible(query.project_id, query.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "environment not found in organization".into(),
                )));
            }
            let scope = match InferenceEnvironmentScope::new(
                query.organization_id,
                query.project_id,
                query.environment_id,
            ) {
                Ok(scope) => scope,
                Err(error) => return Ok(Err(ApplicationError::Forbidden(error))),
            };
            match environments.environment_exists(scope).await {
                Ok(true) => {}
                Ok(false) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "environment not found in organization and project".into(),
                    )));
                }
                Err(error) => return Ok(Err(error.into())),
            }
            if query.from_day > query.to_day {
                return Ok(Err(ApplicationError::Invalid(
                    "inference usage rollup from_day must be <= to_day".into(),
                )));
            }
            Ok(usage
                .list_daily_rollups(
                    query.organization_id,
                    query.environment_id,
                    query.from_day,
                    query.to_day,
                )
                .await
                .map_err(ApplicationError::from))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::inference::InMemoryInferenceUsageRepository;
    use crate::modules::inference::domain::AcceptInferenceUsageBatchWrite;
    use crate::modules::shared_kernel::domain::{NodeId, RepositoryError};
    use a3s_cloud_contracts::{
        InferenceUsageBatchV1, InferenceUsageCursorV1, InferenceUsageEndpointV1,
        InferenceUsageLifecycleEventV1, InferenceUsageLifecycleKindV1,
        InferenceUsageMeasurementCompletenessV1, InferenceUsageRecordV1,
        InferenceUsageRequestEvidenceV1, InferenceUsageTerminalOutcomeV1,
    };
    use async_trait::async_trait;
    use base64::Engine;
    use chrono::{DateTime, Utc};
    use sha2::{Digest, Sha256};
    use uuid::Uuid;

    struct AlwaysPresentEnvironmentRepository;

    #[async_trait]
    impl IInferenceEnvironmentAccess for AlwaysPresentEnvironmentRepository {
        async fn environment_exists(
            &self,
            _scope: InferenceEnvironmentScope,
        ) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    struct MissingEnvironmentRepository;

    #[async_trait]
    impl IInferenceEnvironmentAccess for MissingEnvironmentRepository {
        async fn environment_exists(
            &self,
            _scope: InferenceEnvironmentScope,
        ) -> Result<bool, RepositoryError> {
            Ok(false)
        }
    }

    fn org_wide() -> InferenceAccess {
        InferenceAccess::organization_wide()
    }

    fn lifecycle(kind: InferenceUsageLifecycleKindV1, at: &str, terminal: bool) -> Vec<u8> {
        let event = InferenceUsageLifecycleEventV1 {
            schema: InferenceUsageLifecycleEventV1::SCHEMA.into(),
            kind,
            occurred_at: DateTime::parse_from_rfc3339(at)
                .unwrap()
                .with_timezone(&Utc),
            request: InferenceUsageRequestEvidenceV1 {
                request_id: Uuid::from_u128(100),
                correlation_id: "corr".into(),
                environment_id: Uuid::from_u128(101),
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

    #[tokio::test]
    async fn lists_only_requested_environment_rollups() {
        let usage = Arc::new(InMemoryInferenceUsageRepository::new());
        let organization_id = OrganizationId::from_uuid(Uuid::from_u128(1));
        let project_id = ProjectId::from_uuid(Uuid::from_u128(50));
        let environment_id = EnvironmentId::from_uuid(Uuid::from_u128(101));
        let handler = ListDailyUsageRollupsHandler::new(
            Arc::new(AlwaysPresentEnvironmentRepository),
            usage.clone(),
        );
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

        let day = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
        let listed = handler
            .execute(
                ListDailyUsageRollups {
                    organization_id,
                    project_id,
                    environment_id,
                    from_day: day,
                    to_day: day,
                    access: org_wide(),
                },
                CqrsContext::new(a3s_boot::ModuleRef::new()),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].total_tokens, 7);

        let denied = handler
            .execute(
                ListDailyUsageRollups {
                    organization_id,
                    project_id,
                    environment_id,
                    from_day: day,
                    to_day: day,
                    access: InferenceAccess::restricted([InferenceAccessScope::Environment {
                        project_id: ProjectId::from_uuid(Uuid::from_u128(50)),
                        environment_id: EnvironmentId::from_uuid(Uuid::from_u128(999)),
                    }]),
                },
                CqrsContext::new(a3s_boot::ModuleRef::new()),
            )
            .await
            .unwrap()
            .unwrap_err();
        assert!(matches!(denied, ApplicationError::NotFound(_)));
    }

    #[tokio::test]
    async fn rejects_inverted_day_window() {
        let usage = Arc::new(InMemoryInferenceUsageRepository::new());
        let handler =
            ListDailyUsageRollupsHandler::new(Arc::new(AlwaysPresentEnvironmentRepository), usage);
        let from = NaiveDate::from_ymd_opt(2026, 1, 3).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
        let error = handler
            .execute(
                ListDailyUsageRollups {
                    organization_id: OrganizationId::from_uuid(Uuid::from_u128(1)),
                    project_id: ProjectId::from_uuid(Uuid::from_u128(50)),
                    environment_id: EnvironmentId::from_uuid(Uuid::from_u128(101)),
                    from_day: from,
                    to_day: to,
                    access: org_wide(),
                },
                CqrsContext::new(a3s_boot::ModuleRef::new()),
            )
            .await
            .unwrap()
            .unwrap_err();
        assert!(matches!(error, ApplicationError::Invalid(_)));
    }

    #[tokio::test]
    async fn fails_closed_when_window_precedes_retention_boundary() {
        use crate::modules::inference::domain::{
            InferenceUsageRetentionPolicy, InferenceUsageRetentionSweep,
        };
        use std::time::Duration;

        let usage = Arc::new(InMemoryInferenceUsageRepository::new());
        let organization_id = OrganizationId::from_uuid(Uuid::from_u128(1));
        let project_id = ProjectId::from_uuid(Uuid::from_u128(50));
        let environment_id = EnvironmentId::from_uuid(Uuid::from_u128(101));
        let handler = ListDailyUsageRollupsHandler::new(
            Arc::new(AlwaysPresentEnvironmentRepository),
            usage.clone(),
        );
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

        let day = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
        let denied = handler
            .execute(
                ListDailyUsageRollups {
                    organization_id,
                    project_id,
                    environment_id,
                    from_day: day,
                    to_day: day,
                    access: org_wide(),
                },
                CqrsContext::new(a3s_boot::ModuleRef::new()),
            )
            .await
            .unwrap()
            .unwrap_err();
        assert!(
            matches!(denied, ApplicationError::Conflict(_)),
            "showback before records_available_from must fail closed, got {denied:?}"
        );
    }

    #[tokio::test]
    async fn missing_environment_fails_closed_as_not_found() {
        let usage = Arc::new(InMemoryInferenceUsageRepository::new());
        let handler =
            ListDailyUsageRollupsHandler::new(Arc::new(MissingEnvironmentRepository), usage);
        let day = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
        let denied = handler
            .execute(
                ListDailyUsageRollups {
                    organization_id: OrganizationId::from_uuid(Uuid::from_u128(1)),
                    project_id: ProjectId::from_uuid(Uuid::from_u128(50)),
                    environment_id: EnvironmentId::from_uuid(Uuid::from_u128(101)),
                    from_day: day,
                    to_day: day,
                    access: org_wide(),
                },
                CqrsContext::new(a3s_boot::ModuleRef::new()),
            )
            .await
            .unwrap()
            .unwrap_err();
        assert!(matches!(denied, ApplicationError::NotFound(_)));
    }
}
