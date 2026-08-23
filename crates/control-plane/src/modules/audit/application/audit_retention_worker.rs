use crate::modules::audit::domain::{
    AuditRetentionPolicy, AuditRetentionReport, AuditRetentionSweep, IAuditRecordRepository,
    MAXIMUM_AUDIT_RETENTION_BATCH_SIZE,
};
use crate::modules::shared_kernel::domain::{canonical_timestamp, RepositoryError};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

pub struct AuditRetentionWorker {
    repository: Arc<dyn IAuditRecordRepository>,
    policy: AuditRetentionPolicy,
    poll_interval: Duration,
    organization_batch_size: usize,
    record_batch_size: usize,
}

impl AuditRetentionWorker {
    pub fn new(
        repository: Arc<dyn IAuditRecordRepository>,
        retention: Duration,
        poll_interval: Duration,
        organization_batch_size: usize,
        record_batch_size: usize,
    ) -> Result<Self, String> {
        let policy = AuditRetentionPolicy::new(retention)?;
        if poll_interval.is_zero()
            || poll_interval > Duration::from_secs(24 * 60 * 60)
            || poll_interval > retention
            || organization_batch_size == 0
            || organization_batch_size > MAXIMUM_AUDIT_RETENTION_BATCH_SIZE
            || record_batch_size == 0
            || record_batch_size > MAXIMUM_AUDIT_RETENTION_BATCH_SIZE
        {
            return Err("audit retention requires a bounded poll interval and batches".into());
        }
        Ok(Self {
            repository,
            policy,
            poll_interval,
            organization_batch_size,
            record_batch_size,
        })
    }

    pub fn policy(&self) -> &AuditRetentionPolicy {
        &self.policy
    }

    pub async fn run_once(
        &self,
        now: DateTime<Utc>,
    ) -> Result<AuditRetentionReport, RepositoryError> {
        let swept_at = canonical_timestamp(now);
        let cutoff = self
            .policy
            .cutoff(swept_at)
            .map_err(RepositoryError::Storage)?;
        let interval = chrono::Duration::from_std(self.poll_interval)
            .map_err(|_| RepositoryError::Storage("audit retention interval overflowed".into()))?;
        let next_scan_at = swept_at
            .checked_add_signed(interval)
            .map(canonical_timestamp)
            .ok_or_else(|| {
                RepositoryError::Storage("audit retention schedule overflowed".into())
            })?;
        self.repository
            .sweep_retention(AuditRetentionSweep {
                cutoff,
                swept_at,
                next_scan_at,
                policy_digest: self.policy.digest().clone(),
                organization_batch_size: self.organization_batch_size,
                record_batch_size: self.record_batch_size,
            })
            .await
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.poll_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match self.run_once(Utc::now()).await {
                        Ok(report) => tracing::debug!(
                            inspected_organizations = report.inspected_organizations,
                            completed_organizations = report.completed_organizations,
                            deleted_records = report.deleted_records,
                            "audit retention cycle completed"
                        ),
                        Err(error) => tracing::error!(
                            error = %error,
                            "audit retention cycle failed"
                        ),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::audit::{
        AuditAttributionStatus, AuditRecord, AuditRecordCursor, AuditRecordFilter,
        AuditRetentionStatus, InMemoryAuditRecordRepository,
    };
    use crate::modules::shared_kernel::domain::{OrganizationId, RepositoryError};
    use chrono::{Duration as ChronoDuration, TimeZone};
    use uuid::Uuid;

    #[tokio::test]
    async fn sweep_is_bounded_monotonic_and_closes_query_and_late_write_gaps() {
        let repository = Arc::new(InMemoryAuditRecordRepository::new());
        let organization_id = OrganizationId::new();
        repository.register_organization(organization_id).await;
        let now = Utc
            .with_ymd_and_hms(2026, 8, 23, 12, 0, 0)
            .single()
            .expect("now");
        for occurred_at in [
            now - ChronoDuration::days(3),
            now - ChronoDuration::days(2),
            now - ChronoDuration::hours(1),
        ] {
            repository
                .register(record(organization_id, occurred_at))
                .await
                .expect("audit record");
        }

        let worker = AuditRetentionWorker::new(
            repository.clone(),
            Duration::from_secs(24 * 60 * 60),
            Duration::from_secs(1),
            1,
            1,
        )
        .expect("worker");
        let first = worker.run_once(now).await.expect("first sweep");
        assert_eq!(first.inspected_organizations, 1);
        assert_eq!(first.deleted_records, 1);
        assert_eq!(first.completed_organizations, 0);

        let first_boundary = now - ChronoDuration::days(1);
        let state = repository
            .retention_state(organization_id)
            .await
            .expect("retention state");
        assert_eq!(state.records_available_from, Some(first_boundary));
        assert_eq!(state.records_deleted_before, None);
        assert_eq!(state.total_deleted_records, 1);
        assert_eq!(state.version, 1);
        assert_eq!(
            state.applied_policy_digest.as_ref(),
            Some(worker.policy().digest())
        );

        let visible = repository
            .list_page(organization_id, &AuditRecordFilter::default(), None, 10)
            .await
            .expect("retained page");
        assert_eq!(visible.len(), 1, "physical backlog must remain hidden");
        assert_eq!(visible[0].occurred_at, now - ChronoDuration::hours(1));

        let unavailable = repository
            .list_page(
                organization_id,
                &AuditRecordFilter {
                    from: Some(first_boundary - ChronoDuration::seconds(1)),
                    ..AuditRecordFilter::default()
                },
                None,
                10,
            )
            .await;
        assert!(matches!(unavailable, Err(RepositoryError::Conflict(_))));
        let stale_cursor = repository
            .list_page(
                organization_id,
                &AuditRecordFilter::default(),
                Some(AuditRecordCursor {
                    occurred_at: first_boundary - ChronoDuration::seconds(1),
                    audit_id: Uuid::now_v7(),
                }),
                10,
            )
            .await;
        assert!(matches!(stale_cursor, Err(RepositoryError::Conflict(_))));
        let late_write = repository
            .register(record(
                organization_id,
                first_boundary - ChronoDuration::seconds(1),
            ))
            .await;
        assert!(matches!(late_write, Err(RepositoryError::Conflict(_))));

        let second_now = now + ChronoDuration::seconds(2);
        let second = worker.run_once(second_now).await.expect("second sweep");
        assert_eq!(second.deleted_records, 1);
        assert_eq!(second.completed_organizations, 1);
        let second_boundary = second_now - ChronoDuration::days(1);
        let state = repository
            .retention_state(organization_id)
            .await
            .expect("completed state");
        assert_eq!(state.records_available_from, Some(second_boundary));
        assert_eq!(state.records_deleted_before, Some(second_boundary));
        assert_eq!(state.total_deleted_records, 2);
        assert_eq!(state.version, 2);

        let relaxed = AuditRetentionWorker::new(
            repository.clone(),
            Duration::from_secs(2 * 24 * 60 * 60),
            Duration::from_secs(1),
            1,
            1,
        )
        .expect("relaxed worker");
        relaxed
            .run_once(now + ChronoDuration::seconds(4))
            .await
            .expect("relaxed sweep");
        let state = repository
            .retention_state(organization_id)
            .await
            .expect("relaxed state");
        assert_eq!(state.records_available_from, Some(second_boundary));
        assert_eq!(state.records_deleted_before, Some(second_boundary));
        assert_eq!(state.total_deleted_records, 2);
        assert_eq!(state.version, 3);
        let status = AuditRetentionStatus::from_state(relaxed.policy(), state).expect("status");
        assert!(status.current_policy_applied);
    }

    #[tokio::test]
    async fn record_batch_is_a_global_cycle_budget_and_due_tenants_remain_fair() {
        let repository = Arc::new(InMemoryAuditRecordRepository::new());
        let organizations = [OrganizationId::new(), OrganizationId::new()];
        let now = Utc
            .with_ymd_and_hms(2026, 8, 23, 12, 0, 0)
            .single()
            .expect("now");
        for organization_id in organizations {
            repository.register_organization(organization_id).await;
            repository
                .register(record(organization_id, now - ChronoDuration::days(2)))
                .await
                .expect("audit record");
        }
        let worker = AuditRetentionWorker::new(
            repository.clone(),
            Duration::from_secs(24 * 60 * 60),
            Duration::from_secs(1),
            2,
            1,
        )
        .expect("worker");

        let first = worker.run_once(now).await.expect("first cycle");
        assert_eq!(first.inspected_organizations, 1);
        assert_eq!(first.completed_organizations, 1);
        assert_eq!(first.deleted_records, 1);
        let first_states = futures_util::future::join_all(
            organizations
                .into_iter()
                .map(|organization_id| repository.retention_state(organization_id)),
        )
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .expect("first states");
        assert_eq!(
            first_states.iter().map(|state| state.version).sum::<u64>(),
            1
        );
        assert_eq!(
            first_states
                .iter()
                .map(|state| state.total_deleted_records)
                .sum::<u64>(),
            1
        );

        let second = worker.run_once(now).await.expect("second cycle");
        assert_eq!(second.inspected_organizations, 1);
        assert_eq!(second.completed_organizations, 1);
        assert_eq!(second.deleted_records, 1);
        for organization_id in organizations {
            let state = repository
                .retention_state(organization_id)
                .await
                .expect("completed state");
            assert_eq!(state.version, 1);
            assert_eq!(state.total_deleted_records, 1);
        }
    }

    fn record(organization_id: OrganizationId, occurred_at: DateTime<Utc>) -> AuditRecord {
        AuditRecord {
            id: Uuid::now_v7(),
            organization_id,
            actor_principal_id: None,
            action: "identity.membership.created".into(),
            aggregate_id: Uuid::now_v7(),
            occurred_at,
            request_id: Uuid::now_v7(),
            project_id: None,
            environment_id: None,
            attribution_profile_id: None,
            attribution_status: AuditAttributionStatus::NotApplicable,
        }
    }
}
