use crate::modules::artifacts::application::{BuildOperationRequest, IBuildOperationScheduler};
use crate::modules::artifacts::domain::IBuildRunRepository;
use crate::modules::shared_kernel::domain::RepositoryError;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

pub const BUILD_WORKFLOW_NAME: &str = "cloud.build";
pub const BUILD_WORKFLOW_VERSION: &str = "5";
pub const RETIRED_BUILD_WORKFLOW_VERSIONS: &[&str] = &["1", "2", "3", "4"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildRunReconcileReport {
    pub reserved: usize,
    pub started: usize,
    pub replayed: usize,
    pub failures: Vec<String>,
}

pub struct BuildRunReconciler {
    builds: Arc<dyn IBuildRunRepository>,
    operation_scheduler: Arc<dyn IBuildOperationScheduler>,
    interval: Duration,
    batch_size: usize,
}

impl BuildRunReconciler {
    pub fn from_operation_scheduler(
        builds: Arc<dyn IBuildRunRepository>,
        operation_scheduler: Arc<dyn IBuildOperationScheduler>,
    ) -> Self {
        Self {
            builds,
            operation_scheduler,
            interval: Duration::from_secs(1),
            batch_size: 100,
        }
    }

    pub fn with_operation_scheduler_and_schedule(
        builds: Arc<dyn IBuildRunRepository>,
        operation_scheduler: Arc<dyn IBuildOperationScheduler>,
        interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if interval.is_zero() || batch_size == 0 {
            return Err(
                "build-run reconciliation requires a positive interval and batch size".into(),
            );
        }
        Ok(Self {
            builds,
            operation_scheduler,
            interval,
            batch_size,
        })
    }

    pub async fn run_once(&self, limit: usize) -> Result<BuildRunReconcileReport, RepositoryError> {
        let limit = limit.max(1);
        let reserved = self.builds.reserve_pending(limit).await?;
        let pending = self.builds.pending_operation_starts(limit).await?;
        let mut report = BuildRunReconcileReport {
            reserved: reserved.len(),
            ..BuildRunReconcileReport::default()
        };
        for build in pending {
            let operation = BuildOperationRequest::new(
                build.operation_id,
                build.organization_id,
                build.id,
                build.requested_at,
            );
            match self.operation_scheduler.schedule(operation).await {
                Ok(outcome) if outcome.replayed() => report.replayed += 1,
                Ok(_) => report.started += 1,
                Err(error) => report.failures.push(format!(
                    "could not enqueue build run {} operation: {error}",
                    build.id
                )),
            }
        }
        Ok(report)
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match self.run_once(self.batch_size).await {
                        Ok(report) => {
                            for error in report.failures {
                                tracing::warn!(error = %error, "build-run reconciliation failed");
                            }
                        }
                        Err(error) => tracing::error!(
                            error = %error,
                            "build-run reconciliation scan failed"
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
    use crate::modules::artifacts::application::BuildOperationScheduleOutcome;
    use crate::modules::artifacts::domain::BuildRun;
    use crate::modules::artifacts::infrastructure::InMemoryBuildRunRepository;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, OperationId, OrganizationId, ProjectId, SourceRevisionId,
    };
    use async_trait::async_trait;
    use chrono::Utc;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingBuildOperationScheduler {
        requests: Mutex<Vec<BuildOperationRequest>>,
    }

    #[async_trait]
    impl IBuildOperationScheduler for RecordingBuildOperationScheduler {
        async fn schedule(
            &self,
            request: BuildOperationRequest,
        ) -> Result<BuildOperationScheduleOutcome, RepositoryError> {
            self.requests
                .lock()
                .expect("record build operation")
                .push(request);
            Ok(BuildOperationScheduleOutcome::new(false))
        }
    }

    #[test]
    fn reconciliation_schedule_must_be_bounded() {
        let builds = Arc::new(InMemoryBuildRunRepository::new());
        let scheduler = Arc::new(RecordingBuildOperationScheduler::default());
        assert!(BuildRunReconciler::with_operation_scheduler_and_schedule(
            builds.clone(),
            scheduler.clone(),
            Duration::ZERO,
            10,
        )
        .is_err());
        assert!(BuildRunReconciler::with_operation_scheduler_and_schedule(
            builds,
            scheduler,
            Duration::from_millis(1),
            0,
        )
        .is_err());
    }

    #[tokio::test]
    async fn revision_to_operation_gap_is_repaired_without_duplicate_work() {
        let builds = Arc::new(InMemoryBuildRunRepository::new());
        let scheduler = Arc::new(RecordingBuildOperationScheduler::default());
        let organization_id = OrganizationId::new();
        let source_revision_id = SourceRevisionId::new();
        builds
            .add_source_revision(
                organization_id,
                ProjectId::new(),
                EnvironmentId::new(),
                source_revision_id,
                Utc::now(),
            )
            .await;
        let reconciler =
            BuildRunReconciler::from_operation_scheduler(builds.clone(), scheduler.clone());

        let first = reconciler.run_once(10).await.expect("first reconcile");
        assert_eq!(first.reserved, 1);
        assert_eq!(first.started, 1);
        assert!(first.failures.is_empty());
        let build_id = BuildRun::id_for(source_revision_id);
        {
            let requests = scheduler.requests.lock().expect("read build operations");
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].organization_id(), organization_id);
            assert_eq!(requests[0].build_run_id(), build_id);
            assert_eq!(
                requests[0].operation_id(),
                OperationId::from_uuid(build_id.as_uuid())
            );
        }

        builds.mark_operation_started(build_id).await;
        let replay = reconciler.run_once(10).await.expect("reconcile replay");
        assert_eq!(replay.reserved, 0);
        assert_eq!(replay.started, 0);
        assert_eq!(replay.replayed, 0);
        assert!(replay.failures.is_empty());
        assert_eq!(
            scheduler
                .requests
                .lock()
                .expect("read build operations")
                .len(),
            1
        );
    }
}
