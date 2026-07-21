use crate::modules::artifacts::domain::IBuildRunRepository;
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::RepositoryError;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

pub const BUILD_WORKFLOW_NAME: &str = "cloud.build";
pub const BUILD_WORKFLOW_VERSION: &str = "1";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildRunReconcileReport {
    pub reserved: usize,
    pub started: usize,
    pub replayed: usize,
    pub failures: Vec<String>,
}

pub struct BuildRunReconciler {
    builds: Arc<dyn IBuildRunRepository>,
    operations: Arc<dyn IOperationRepository>,
    interval: Duration,
    batch_size: usize,
}

impl BuildRunReconciler {
    pub fn new(
        builds: Arc<dyn IBuildRunRepository>,
        operations: Arc<dyn IOperationRepository>,
    ) -> Self {
        Self {
            builds,
            operations,
            interval: Duration::from_secs(1),
            batch_size: 100,
        }
    }

    pub fn with_schedule(
        builds: Arc<dyn IBuildRunRepository>,
        operations: Arc<dyn IOperationRepository>,
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
            operations,
            interval,
            batch_size,
        })
    }

    pub async fn run_once(&self, limit: usize) -> Result<BuildRunReconcileReport, RepositoryError> {
        let limit = limit.max(1);
        let reserved = self
            .builds
            .reserve_pending(limit, chrono::Utc::now())
            .await?;
        let pending = self.builds.pending_operation_starts(limit).await?;
        let mut report = BuildRunReconcileReport {
            reserved: reserved.len(),
            ..BuildRunReconcileReport::default()
        };
        for build in pending {
            let subject = OperationSubject::new("build_run", build.id.as_uuid())
                .map_err(RepositoryError::Storage)?;
            let workflow = WorkflowIdentity::new(BUILD_WORKFLOW_NAME, BUILD_WORKFLOW_VERSION)
                .map_err(RepositoryError::Storage)?;
            let input = json!({
                "organizationId": build.organization_id,
                "buildRunId": build.id,
            });
            let operation = OperationRequest::new(
                build.operation_id,
                build.organization_id,
                subject,
                workflow,
                input,
                build.requested_at,
            );
            match self.operations.enqueue(operation).await {
                Ok(write) if write.replayed => report.replayed += 1,
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
    use crate::modules::artifacts::domain::BuildRun;
    use crate::modules::artifacts::infrastructure::InMemoryBuildRunRepository;
    use crate::modules::operations::domain::repositories::IOperationRepository;
    use crate::modules::operations::infrastructure::persistence::InMemoryOperationRepository;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, OrganizationId, ProjectId, SourceRevisionId,
    };
    use chrono::Utc;

    #[test]
    fn reconciliation_schedule_must_be_bounded() {
        let builds = Arc::new(InMemoryBuildRunRepository::new());
        let operations = Arc::new(InMemoryOperationRepository::new());
        assert!(BuildRunReconciler::with_schedule(
            builds.clone(),
            operations.clone(),
            Duration::ZERO,
            10,
        )
        .is_err());
        assert!(
            BuildRunReconciler::with_schedule(builds, operations, Duration::from_millis(1), 0,)
                .is_err()
        );
    }

    #[tokio::test]
    async fn revision_to_operation_gap_is_repaired_without_duplicate_work() {
        let builds = Arc::new(InMemoryBuildRunRepository::new());
        let operations = Arc::new(InMemoryOperationRepository::new());
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
        let reconciler = BuildRunReconciler::new(builds.clone(), operations.clone());

        let first = reconciler.run_once(10).await.expect("first reconcile");
        assert_eq!(first.reserved, 1);
        assert_eq!(first.started, 1);
        assert!(first.failures.is_empty());
        let build_id = BuildRun::id_for(source_revision_id);
        let operation = operations
            .find_request(
                crate::modules::shared_kernel::domain::OperationId::from_uuid(build_id.as_uuid()),
            )
            .await
            .expect("find operation")
            .expect("operation");
        assert_eq!(operation.organization_id, organization_id);
        assert_eq!(operation.workflow.name(), BUILD_WORKFLOW_NAME);
        assert_eq!(operation.subject.kind(), "build_run");

        builds.mark_operation_started(build_id).await;
        let replay = reconciler.run_once(10).await.expect("reconcile replay");
        assert_eq!(replay.reserved, 0);
        assert_eq!(replay.started, 0);
        assert_eq!(replay.replayed, 0);
        assert!(replay.failures.is_empty());
    }
}
